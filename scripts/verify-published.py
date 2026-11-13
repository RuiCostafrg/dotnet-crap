#!/usr/bin/env python3
"""Verify a published dotnet-crap GitHub release and the npm packages built from it.

This is a read-only backend helper for release workflows. It never rebuilds
anything: the npm command downloads the retained, attested tarballs and compares
those exact bytes with the public npm registry.

Commands (both print one JSON evidence object on stdout and create a new output
directory only after every check succeeds):

  fetch --version V --source-sha SHA --output DIR
      Resolve tag vV to SHA, download every asset of the published GitHub
      release, enforce prepare-release.py's exact asset set, verify every
      checksum and every build-provenance attestation, and record SHA-256 hashes.

  npm --version V --source-sha SHA --assets DIR --output DIR
      Find the successful push-triggered release.yml run for SHA and tag vV,
      download its unexpired npm-packages artifact, verify all five tarball
      attestations, download the same versions from registry.npmjs.org over
      TLS, compare both hashes and bytes, then run verify-npm-packages.py.

The runner and registry transport are injectable so the offline tests can
exercise every failure path without a network or a real `gh`.
"""
import argparse
import base64
import binascii
import contextlib
import hashlib
import importlib.util
import io
import json
from pathlib import Path, PurePosixPath
import re
import secrets
import shutil
import ssl
import subprocess
import sys
import urllib.error
import urllib.parse
import urllib.request

ROOT = Path(__file__).resolve().parents[1]


def _load(name, relative):
    spec = importlib.util.spec_from_file_location(name, ROOT / relative)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


prepare = _load('verify_published_prepare_release', 'scripts/prepare-release.py')
npm = _load('verify_published_npm_packages', 'scripts/npm-packages.py')
npm_verify = _load('verify_published_verify_npm_packages', 'scripts/verify-npm-packages.py')

REPOSITORY = 'RuiCostafrg/dotnet-crap'
WORKFLOW = 'release.yml'
REGISTRY = 'registry.npmjs.org'
USER_AGENT = 'dotnet-crap-release-verifier/1'
# Same canonical shape as scripts/npm-packages.py and the tag checks in release.yml.
VERSION = re.compile(r'(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)'
                     r'(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?')
SHA = re.compile(r'[0-9a-f]{40}')
DIGEST = re.compile(r'[0-9a-f]{64}')
INTEGRITY = re.compile(r'sha512-([A-Za-z0-9+/]+={0,2})')
MAX_CHECKSUM = 4096
MAX_METADATA = 1024 * 1024
MAX_TARBALL = 128 * 1024 * 1024
TIMEOUT = 60
CHUNK = 1024 * 1024


def canonical_version(value):
    """Reject anything that is not the exact SemVer text used in a release tag and asset names."""
    if not isinstance(value, str) or not VERSION.fullmatch(value):
        raise ValueError(f'invalid version: {value!r}')
    prerelease = value.partition('-')[2]
    if any(part.isdigit() and len(part) > 1 and part.startswith('0')
           for part in prerelease.split('.')):
        raise ValueError(f'invalid version: {value!r}')
    return value


def canonical_sha(value):
    """A source digest is exactly 40 lowercase hexadecimal characters, never stripped or normalized."""
    if not isinstance(value, str) or not SHA.fullmatch(value):
        raise ValueError('source SHA must be a full lowercase 40-character commit SHA')
    return value


def _finite_size(path, maximum, description):
    try:
        size = path.stat().st_size
    except OSError as error:
        raise ValueError(f'{description}: cannot stat {path.name}') from error
    if size > maximum:
        raise ValueError(f'{description}: {path.name} is larger than {maximum} bytes')
    return size


def _sha256_file(path):
    digest = hashlib.sha256()
    with path.open('rb') as stream:
        while chunk := stream.read(CHUNK):
            digest.update(chunk)
    return digest.hexdigest()


def _fresh_directory(output):
    path = Path(output).absolute()
    if path.exists() or path.is_symlink():
        raise FileExistsError(f'output already exists: {path}')
    return path


def _staging_directory(output, prefix):
    # output.parent may not exist yet; create it only after all input validation.
    output.parent.mkdir(parents=True, exist_ok=True)
    staging = output.parent / f'.{prefix}-{secrets.token_hex(8)}'
    staging.mkdir()
    return staging


def _publish_directory(staging, output):
    if output.exists() or output.is_symlink():
        raise FileExistsError(f'output already exists: {output}')
    staging.rename(output)


def _safe_checksum_name(name):
    """A checksum may name only one plain basename from its own directory."""
    if (not name or name in ('.', '..') or '/' in name or '\\' in name
            or name.startswith('/') or PurePosixPath(name).name != name
            or re.match(r'^[A-Za-z]:', name) or any(ord(char) < 32 for char in name)):
        raise ValueError(f'unsafe checksum filename: {name!r}')
    return name


def read_checksum(path, expected_name):
    """Parse one sha256 sidecar: exactly one lowercase digest and one safe basename."""
    if path.is_symlink() or not path.is_file():
        raise ValueError(f'invalid checksum file for {expected_name}: missing regular sidecar')
    if _finite_size(path, MAX_CHECKSUM, 'invalid checksum file') <= 0:
        raise ValueError(f'invalid checksum file for {expected_name}: empty sidecar')
    try:
        text = path.read_text(encoding='utf-8')
    except UnicodeDecodeError as error:
        raise ValueError(f'invalid checksum file for {expected_name}: not UTF-8') from error
    fields = text.split()
    if len(fields) != 2:
        raise ValueError(f'invalid checksum file for {expected_name}: expected one digest and filename')
    digest, listed = fields
    if listed.startswith('*'):
        listed = listed[1:]
    _safe_checksum_name(listed)
    if listed != expected_name:
        raise ValueError(f'invalid checksum file for {expected_name}: names {listed!r}')
    if not DIGEST.fullmatch(digest):
        raise ValueError(f'invalid checksum file for {expected_name}: digest is not lowercase SHA-256')
    return digest


def verify_assets(directory, version):
    """Enforce the prepare-release.py asset set, then verify every sidecar in Python."""
    directory = Path(directory)
    if directory.is_symlink() or not directory.is_dir():
        raise ValueError(f'release asset directory is missing or not a directory: {directory}')
    expected = prepare.asset_names(version)
    present = {}
    for entry in directory.iterdir():
        if entry.is_symlink() or not entry.is_file():
            raise ValueError(f'release asset is not a regular file: {entry.name}')
        present[entry.name] = entry
    missing = sorted(set(expected) - set(present))
    extra = sorted(set(present) - set(expected))
    if missing or extra:
        details = []
        if missing:
            details.append(f'missing {missing}')
        if extra:
            details.append(f'unexpected {extra}')
        raise ValueError('release asset set is not exact: ' + '; '.join(details))
    hashes = {name: _sha256_file(present[name]) for name in expected}
    _finite_size(present['sha256.sum'], MAX_CHECKSUM, 'invalid checksum file')
    for name in prepare.checksummed_assets(version):
        if read_checksum(present[name + '.sha256'], name) != hashes[name]:
            raise ValueError(f'checksum mismatch: {name}')
    # Also enforces cargo-dist's combined sha256.sum agreement.
    prepare.validate(directory, version)
    return hashes


def _cert_identity(tag):
    return f'https://github.com/{REPOSITORY}/.github/workflows/{WORKFLOW}@refs/tags/{tag}'


def verify_attestation(path, tag, source_sha, runner):
    identity = _cert_identity(tag)
    runner([
        'attestation', 'verify', str(Path(path).resolve()),
        '--repo', REPOSITORY,
        '--cert-identity', identity,
        '--source-ref', f'refs/tags/{tag}',
        '--source-digest', source_sha,
        '--deny-self-hosted-runners',
    ])


def run_gh(arguments, *, cwd=None):
    """The real gh runner. Tests inject a recording/failing callable instead."""
    try:
        # Fixed gh argument vectors; no shell or externally supplied command text.
        # nosemgrep: python.lang.security.audit.dangerous-subprocess-use-audit
        result = subprocess.run(['gh', *arguments], cwd=cwd, check=True, text=True,
                                encoding='utf-8', capture_output=True)
    except FileNotFoundError as error:
        raise RuntimeError('the gh CLI is required to verify published releases') from error
    except subprocess.CalledProcessError as error:
        message = (error.stderr or error.stdout or str(error)).strip()
        if len(message) > 2000:
            message = message[:2000] + '...'
        raise RuntimeError(f'gh {arguments[0]} failed: {message}') from error
    return result.stdout


def _api_endpoint(arguments):
    for argument in arguments:
        if isinstance(argument, str) and argument.startswith('repos/'):
            return argument
    raise ValueError(f'gh api invocation has no repository endpoint: {arguments!r}')


def _json_output(text, description):
    try:
        value = json.loads(text)
    except json.JSONDecodeError as error:
        raise ValueError(f'{description}: gh returned invalid JSON') from error
    if not isinstance(value, list):
        raise ValueError(f'{description}: expected a JSON array')
    return value


def _paged_items(endpoint, key, description, runner):
    # gh rejects --slurp combined with --jq/--template. Collect its raw pages
    # and flatten the endpoint's collection here, retaining every page.
    pages = _json_output(runner(['api', '--paginate', '--slurp', endpoint]), description)
    items = []
    for page in pages:
        if not isinstance(page, dict) or not isinstance(page.get(key), list):
            raise ValueError(f'{description}: expected each page to contain a {key} array')
        items.extend(page[key])
    return items


def resolve_tag(tag, runner):
    output = runner(['api', f'repos/{REPOSITORY}/commits/{tag}', '--jq', '.sha'])
    commit = output.strip()
    if not SHA.fullmatch(commit):
        raise ValueError(f'tag {tag} did not resolve to a full commit SHA: {commit!r}')
    return commit


def published_release(tag, runner):
    output = runner(['api', f'repos/{REPOSITORY}/releases/tags/{tag}'])
    try:
        release = json.loads(output)
    except json.JSONDecodeError as error:
        raise ValueError(f'release {tag}: gh returned invalid JSON') from error
    if not isinstance(release, dict):
        raise ValueError(f'release {tag}: expected a JSON object')
    if release.get('tag_name') != tag:
        raise ValueError(f'release {tag}: API returned tag {release.get("tag_name")!r}')
    if release.get('draft') is not False:
        raise ValueError(f'release {tag}: the GitHub release is still a draft')
    if not release.get('published_at'):
        raise ValueError(f'release {tag}: the GitHub release has no published_at timestamp')
    if release.get('immutable') is not True:
        raise ValueError(f'release {tag}: published release assets are not immutable')
    return release


def fetch(version, source_sha, output, *, runner=run_gh):
    """Download and verify the complete published GitHub release."""
    version = canonical_version(version)
    source_sha = canonical_sha(source_sha)
    tag = f'v{version}'
    output = _fresh_directory(output)

    commit = resolve_tag(tag, runner)
    if commit != source_sha:
        raise ValueError(f'tag {tag} resolves to {commit}, expected {source_sha}')
    release = published_release(tag, runner)

    staging = _staging_directory(output, 'verify-published-fetch')
    try:
        runner(['release', 'download', tag, '--repo', REPOSITORY, '--dir', str(staging)])
        hashes = verify_assets(staging, version)
        names = prepare.asset_names(version)
        for name in names:
            verify_attestation(staging / name, tag, source_sha, runner)
        evidence = {
            'version': version,
            'source_sha': source_sha,
            'tag': tag,
            'repository': REPOSITORY,
            'release_id': release.get('id'),
            'published_at': release.get('published_at'),
            'attested_assets': names,
            'assets': hashes,
        }
        _publish_directory(staging, output)
        return evidence
    finally:
        if staging.exists():
            shutil.rmtree(staging, ignore_errors=True)


def _positive_int(value, description):
    if not isinstance(value, int) or isinstance(value, bool) or value <= 0:
        raise ValueError(f'{description}: expected a positive integer, got {value!r}')
    return value


def find_release_run(source_sha, tag, runner):
    """Find exactly one successful push run of release.yml at this commit and tag."""
    endpoint = (f'repos/{REPOSITORY}/actions/workflows/{WORKFLOW}/runs'
                f'?head_sha={source_sha}&event=push&per_page=100')
    runs = _paged_items(endpoint, 'workflow_runs', f'{WORKFLOW} runs', runner)
    eligible = {}
    for run in runs:
        if not isinstance(run, dict):
            raise ValueError(f'{WORKFLOW} runs: expected objects')
        path = str(run.get('path') or '')
        if (run.get('head_sha') == source_sha and run.get('head_branch') == tag
                and run.get('event') == 'push' and run.get('status') == 'completed'
                and run.get('conclusion') == 'success' and path.rsplit('/', 1)[-1] == WORKFLOW):
            identifier = _positive_int(run.get('id'), f'{WORKFLOW} run id')
            attempt = run.get('run_attempt', 1)
            if not isinstance(attempt, int) or isinstance(attempt, bool) or attempt <= 0:
                raise ValueError(f'{WORKFLOW} run {identifier}: invalid run_attempt {attempt!r}')
            previous = eligible.get(identifier)
            if previous is None or attempt > previous.get('run_attempt', 1):
                eligible[identifier] = run
    if not eligible:
        raise ValueError(f'no successful {WORKFLOW} push run for {tag} at {source_sha}')
    if len(eligible) != 1:
        raise ValueError(f'expected exactly one successful {WORKFLOW} push run for {tag} '
                         f'at {source_sha}, found {len(eligible)}')
    return next(iter(eligible.values()))


def find_npm_artifact(run, source_sha, tag, runner):
    """Require exactly one unexpired npm-packages artifact on the selected run."""
    run_id = _positive_int(run.get('id'), f'{WORKFLOW} run id')
    endpoint = (f'repos/{REPOSITORY}/actions/runs/{run_id}/artifacts?per_page=100')
    artifacts = _paged_items(endpoint, 'artifacts', f'run {run_id} artifacts', runner)
    named = []
    for artifact in artifacts:
        if not isinstance(artifact, dict):
            raise ValueError(f'run {run_id} artifacts: expected objects')
        if artifact.get('name') == 'npm-packages':
            named.append(artifact)
    if not named:
        raise ValueError(f'run {run_id} has no npm-packages artifact')
    unexpired = []
    for artifact in named:
        expired = artifact.get('expired')
        if expired is True:
            continue
        if expired is not False:
            raise ValueError(f'run {run_id} npm-packages artifact has invalid expired={expired!r}')
        workflow_run = artifact.get('workflow_run')
        if isinstance(workflow_run, dict):
            related = workflow_run.get('id')
            if related is not None and related != run_id:
                raise ValueError(f'run {run_id} npm-packages artifact belongs to run {related}')
            head_sha = workflow_run.get('head_sha')
            head_branch = workflow_run.get('head_branch')
            if head_sha is not None and head_sha != source_sha:
                raise ValueError(f'run {run_id} npm-packages artifact has head_sha {head_sha!r}')
            if head_branch is not None and head_branch != tag:
                raise ValueError(f'run {run_id} npm-packages artifact has head_branch {head_branch!r}')
        unexpired.append(artifact)
    if not unexpired:
        raise ValueError(f'run {run_id} npm-packages artifact has expired')
    if len(unexpired) != 1:
        raise ValueError(f'expected exactly one unexpired npm-packages artifact on run {run_id}, '
                         f'found {len(unexpired)}')
    artifact = unexpired[0]
    _positive_int(artifact.get('id'), f'run {run_id} npm-packages artifact id')
    return artifact


def package_names():
    return [npm.platform_name(target) for target in npm.TARGETS] + [npm.MAIN]


def verify_npm_tarballs(directory, version):
    """Require exactly the five retained tarballs and return {filename: path}."""
    directory = Path(directory)
    if directory.is_symlink() or not directory.is_dir():
        raise ValueError(f'npm artifact directory is missing or not a directory: {directory}')
    expected = {npm.tarball_name(name, version) for name in package_names()}
    present = {}
    for entry in directory.iterdir():
        if entry.is_symlink() or not entry.is_file():
            raise ValueError(f'npm artifact is not a regular file: {entry.name}')
        _finite_size(entry, MAX_TARBALL, 'npm artifact')
        present[entry.name] = entry
    missing = sorted(expected - set(present))
    extra = sorted(set(present) - expected)
    if missing or extra:
        details = []
        if missing:
            details.append(f'missing {missing}')
        if extra:
            details.append(f'unexpected {extra}')
        raise ValueError('npm artifact set is not exact: ' + '; '.join(details))
    return present


def http_get(url, *, max_bytes):
    """TLS GET with the default certificate verification; returns (final URL, bounded bytes)."""
    parsed = urllib.parse.urlsplit(url)
    if parsed.scheme != 'https':
        raise ValueError(f'refusing a non-HTTPS registry URL: {url}')
    request = urllib.request.Request(url, headers={'User-Agent': USER_AGENT})
    context = ssl.create_default_context()
    try:
        with urllib.request.urlopen(request, timeout=TIMEOUT, context=context) as response:
            final_url = response.geturl()
            length = response.headers.get('Content-Length')
            if length is not None:
                try:
                    length = int(length)
                except ValueError as error:
                    raise ValueError(f'registry response has an invalid Content-Length: {length!r}') from error
                if length > max_bytes:
                    raise ValueError(f'registry response is larger than {max_bytes} bytes')
            data = response.read(max_bytes + 1)
    except urllib.error.HTTPError as error:
        raise ValueError(f'registry returned HTTP {error.code} for {url}') from error
    except urllib.error.URLError as error:
        raise ValueError(f'registry request failed for {url}: {error.reason}') from error
    if len(data) > max_bytes:
        raise ValueError(f'registry response is larger than {max_bytes} bytes')
    return final_url, data


def _registry_url(url, *, expected_file=None):
    """Only https://registry.npmjs.org URLs are acceptable, including after redirects."""
    try:
        parsed = urllib.parse.urlsplit(url)
    except ValueError as error:
        raise ValueError(f'invalid registry URL: {url!r}') from error
    if parsed.scheme != 'https' or parsed.netloc != REGISTRY:
        raise ValueError(f'registry URL is not https://{REGISTRY}: {url!r}')
    if expected_file is not None and PurePosixPath(parsed.path).name != expected_file:
        raise ValueError(f'registry tarball URL does not name {expected_file}: {url!r}')
    return url


def _integrity_digest(integrity):
    match = INTEGRITY.fullmatch(integrity) if isinstance(integrity, str) else None
    if match is None:
        raise ValueError(f'registry dist.integrity is not one sha512 value: {integrity!r}')
    try:
        digest = base64.b64decode(match.group(1), validate=True)
    except (ValueError, binascii.Error) as error:
        raise ValueError(f'registry dist.integrity is not valid base64: {integrity!r}') from error
    if len(digest) != 64:
        raise ValueError(f'registry dist.integrity is not a SHA-512 digest: {integrity!r}')
    return digest


def _sha512_integrity(data):
    return 'sha512-' + base64.b64encode(hashlib.sha512(data).digest()).decode('ascii')


def _registry_metadata(name, version, network):
    url = f'https://{REGISTRY}/{name}/{version}'
    final_url, data = network(url, max_bytes=MAX_METADATA)
    _registry_url(final_url)
    try:
        metadata = json.loads(data.decode('utf-8'))
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise ValueError(f'npm registry metadata for {name}@{version} is not UTF-8 JSON') from error
    if not isinstance(metadata, dict):
        raise ValueError(f'npm registry metadata for {name}@{version} is not an object')
    if metadata.get('name') != name:
        raise ValueError(f'npm registry metadata name is {metadata.get("name")!r}, expected {name!r}')
    if metadata.get('version') != version:
        raise ValueError(f'npm registry metadata version is {metadata.get("version")!r}, expected {version!r}')
    dist = metadata.get('dist')
    if not isinstance(dist, dict):
        raise ValueError(f'npm registry metadata for {name}@{version} has no dist object')
    integrity = dist.get('integrity')
    _integrity_digest(integrity)
    tarball = dist.get('tarball')
    if not isinstance(tarball, str):
        raise ValueError(f'npm registry metadata for {name}@{version} has no tarball URL')
    _registry_url(tarball, expected_file=npm.tarball_name(name, version))
    return integrity, tarball


def download_registry_packages(version, retained, registry_directory, network):
    """Download each exact version, prove it matches the attested bytes, and save a copy."""
    registry_directory.mkdir()
    evidence = {}
    for name in package_names():
        filename = npm.tarball_name(name, version)
        path = retained / filename
        if _finite_size(path, MAX_TARBALL, 'npm artifact') <= 0:
            raise ValueError(f'npm artifact is empty: {filename}')
        retained_bytes = path.read_bytes()
        integrity, tarball_url = _registry_metadata(name, version, network)
        expected_integrity = _sha512_integrity(retained_bytes)
        if integrity != expected_integrity:
            raise ValueError(f'{name}@{version}: registry dist.integrity does not match the attested tarball')
        final_url, downloaded = network(tarball_url, max_bytes=MAX_TARBALL)
        _registry_url(final_url, expected_file=filename)
        if _sha512_integrity(downloaded) != integrity:
            raise ValueError(f'{name}@{version}: registry tarball does not match dist.integrity')
        if downloaded != retained_bytes:
            raise ValueError(f'{name}@{version}: registry tarball bytes differ from the attested artifact')
        (registry_directory / filename).write_bytes(downloaded)
        evidence[name] = {
            'file': filename,
            'sha256': hashlib.sha256(downloaded).hexdigest(),
            'sha512': integrity,
            'registry_integrity': integrity,
            'registry_tarball': tarball_url,
            'bytes_match': True,
        }
    return evidence


def _inside(path, parent):
    try:
        path.relative_to(parent)
    except ValueError:
        return False
    return True


def fetch_npm(version, source_sha, assets, output, *, runner=run_gh, network=http_get,
              verify_packages=None):
    """Verify the retained npm artifact and the same exact versions on the public registry."""
    version = canonical_version(version)
    source_sha = canonical_sha(source_sha)
    tag = f'v{version}'
    assets = Path(assets)
    output = _fresh_directory(output)
    if assets.is_symlink() or not assets.is_dir():
        raise ValueError(f'release asset directory is missing or not a directory: {assets}')
    if _inside(output.resolve(), assets.resolve()):
        raise ValueError('output must not be inside the verified release assets directory')
    asset_hashes = verify_assets(assets, version)

    run = find_release_run(source_sha, tag, runner)
    run_id = _positive_int(run.get('id'), f'{WORKFLOW} run id')
    artifact = find_npm_artifact(run, source_sha, tag, runner)
    artifact_id = _positive_int(artifact.get('id'), f'run {run_id} npm-packages artifact id')
    verify = verify_packages or npm_verify.verify

    staging = _staging_directory(output, 'verify-published-npm')
    retained = staging / 'npm-packages'
    registry_directory = staging / 'registry'
    try:
        runner(['run', 'download', str(run_id), '--repo', REPOSITORY,
                '--name', 'npm-packages', '--dir', str(retained)])
        tarballs = verify_npm_tarballs(retained, version)
        for name in package_names():
            verify_attestation(tarballs[npm.tarball_name(name, version)], tag, source_sha, runner)
        packages = download_registry_packages(version, retained, registry_directory, network)
        # The existing verifier checks executable bytes, archive identity and metadata. It prints
        # one status line, so keep stdout clean for the JSON evidence below.
        with contextlib.redirect_stdout(io.StringIO()):
            verify(assets, retained, version)
        evidence = {
            'version': version,
            'source_sha': source_sha,
            'tag': tag,
            'repository': REPOSITORY,
            'run_id': run_id,
            'run_attempt': run.get('run_attempt', 1),
            'artifact_id': artifact_id,
            'artifact_name': 'npm-packages',
            'assets': asset_hashes,
            'attested_packages': package_names(),
            'packages': packages,
        }
        _publish_directory(staging, output)
        return evidence
    finally:
        if staging.exists():
            shutil.rmtree(staging, ignore_errors=True)


def build_parser():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest='command', required=True)

    fetch_parser = commands.add_parser('fetch', help='download and verify the GitHub release assets')
    fetch_parser.add_argument('--version', required=True)
    fetch_parser.add_argument('--source-sha', required=True)
    fetch_parser.add_argument('--output', required=True, type=Path, help='new directory for the assets')

    npm_parser = commands.add_parser('npm', help='verify the retained npm artifact and registry versions')
    npm_parser.add_argument('--version', required=True)
    npm_parser.add_argument('--source-sha', required=True)
    npm_parser.add_argument('--assets', required=True, type=Path,
                            help='release assets already downloaded by fetch')
    npm_parser.add_argument('--output', required=True, type=Path, help='new directory for npm evidence files')
    return parser


def main(argv=None):
    arguments = build_parser().parse_args(argv)
    try:
        if arguments.command == 'fetch':
            evidence = fetch(arguments.version, arguments.source_sha, arguments.output)
        else:
            evidence = fetch_npm(arguments.version, arguments.source_sha,
                                 arguments.assets, arguments.output)
    except (ValueError, RuntimeError, OSError) as error:
        print(f'error: {error}', file=sys.stderr)
        return 2
    print(json.dumps(evidence, indent=2, sort_keys=True))
    return 0


if __name__ == '__main__':
    sys.exit(main())
