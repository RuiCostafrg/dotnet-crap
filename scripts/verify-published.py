#!/usr/bin/env python3
"""Download and authenticate immutable GitHub release assets without rebuilding.

fetch --version V --source-sha SHA --output DIR verifies tag binding, the exact
asset set, checksums and every build-provenance attestation, then emits JSON.
The gh runner is injectable for offline failure-path tests.
"""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path, PurePosixPath
import re
import secrets
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]


def _load(name, relative):
    spec = importlib.util.spec_from_file_location(name, ROOT / relative)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


prepare = _load('verify_published_prepare_release', 'scripts/prepare-release.py')

REPOSITORY = 'RuiCostafrg/dotnet-crap'
WORKFLOW = 'release.yml'
# Same canonical version shape as the tag checks in release.yml.
VERSION = re.compile(r'(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)'
                     r'(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?')
SHA = re.compile(r'[0-9a-f]{40}')
DIGEST = re.compile(r'[0-9a-f]{64}')
MAX_CHECKSUM = 4096
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


def build_parser():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest='command', required=True)

    fetch_parser = commands.add_parser('fetch', help='download and verify the GitHub release assets')
    fetch_parser.add_argument('--version', required=True)
    fetch_parser.add_argument('--source-sha', required=True)
    fetch_parser.add_argument('--output', required=True, type=Path, help='new directory for the assets')

    return parser


def main(argv=None):
    arguments = build_parser().parse_args(argv)
    try:
        if arguments.command == 'fetch':
            evidence = fetch(arguments.version, arguments.source_sha, arguments.output)
    except (ValueError, RuntimeError, OSError) as error:
        print(f'error: {error}', file=sys.stderr)
        return 2
    print(json.dumps(evidence, indent=2, sort_keys=True))
    return 0


if __name__ == '__main__':
    sys.exit(main())
