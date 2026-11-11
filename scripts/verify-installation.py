#!/usr/bin/env python3
"""Install the candidate dotnet-crap release assets through the generated
shell installer and Homebrew formula in isolated prefixes.

Staged assets are served over loopback HTTP for the shell installer and
referenced through file:// URLs for Homebrew, so nothing has to be published
first. Every installed executable is compared with the binary inside the
checksum-verified candidate archive, and the archive the installer or formula
selected (GNU or musl on Linux) is confirmed by byte identity.

Simplified from ts-crap's scripts/verify-installation.py: no Windows/PowerShell,
no coverage helpers, and the previous-version upgrade path is optional and
currently unused because dotnet-crap has never been released.
"""
import argparse
from functools import partial
import hashlib
import http.server
import io
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tarfile
import tempfile
import threading

OWNER = 'RuiCostafrg'
PACKAGE = 'dotnet-crap'
BINARY = 'dotnet-crap'
FORMULA = f'{PACKAGE}.rb'
INSTALLER = f'{PACKAGE}-installer.sh'
DOWNLOAD_PREFIX = f'https://github.com/{OWNER}/{PACKAGE}/releases/download/v'

# Platforms a generated dotnet-crap installer or formula may install. There is
# no Windows channel: the Windows zip exists only for the .NET tool's win-x64
# RID and no installer targets it.
TARGETS = {
    'aarch64-apple-darwin', 'x86_64-apple-darwin',
    'aarch64-unknown-linux-gnu', 'x86_64-unknown-linux-gnu',
    'aarch64-unknown-linux-musl', 'x86_64-unknown-linux-musl',
}
VERSION_PATTERN = r'[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?'


def validate_version(version):
    if not re.fullmatch(VERSION_PATTERN, version):
        raise ValueError(f'Invalid release version: {version}')
    return version


def archive_name(target):
    validate_target(target)
    return f'{PACKAGE}-{target}.tar.xz'


def validate_target(target):
    if target not in TARGETS:
        raise ValueError(f'Unsupported target: {target}')
    return target


def parse_checksum(text, name):
    """Validate a `.sha256` file body for `name` and return the expected digest."""
    fields = text.strip().split()
    if len(fields) != 2 or fields[1].lstrip('*') != name or \
            not re.fullmatch(r'[0-9a-f]{64}', fields[0]):
        raise ValueError(f'Invalid checksum file for {name}: {text.strip()!r}')
    return fields[0]


def verify_checksum(data, name, expected):
    if hashlib.sha256(data).hexdigest() != expected:
        raise ValueError(f'Checksum mismatch: {name}')


def extract_binary(archive, binary=BINARY):
    """Extract the single regular `binary` member from a tar.xz archive."""
    with tarfile.open(fileobj=io.BytesIO(archive), mode='r:xz') as bundle:
        matches = [item for item in bundle.getmembers()
                   if Path(item.name).name == binary and item.isfile()]
        if len(matches) != 1:
            raise ValueError(f'Expected exactly one {binary} executable in the archive')
        return bundle.extractfile(matches[0]).read()


def installer_metadata(text, version):
    """Check the generated installer's identity and return its env-var prefix."""
    name = re.search(r'APP_NAME="([^"]+)"', text)
    app_version = re.search(r'APP_VERSION="([^"]+)"', text)
    if not name or not app_version:
        raise ValueError('Installer does not declare APP_NAME/APP_VERSION')
    if name[1] != PACKAGE:
        raise ValueError(f'Installer is for {name[1]}, not {PACKAGE}')
    if app_version[1] != version:
        raise ValueError('Installer version does not match the expected release')
    return name[1].upper().replace('-', '_')


def download_override(prefix, text):
    """Pick the installer's product-specific download URL override, with the
    cargo-dist generic override as fallback."""
    specific = f'{prefix}_DOWNLOAD_URL'
    return specific if re.search(rf'\b{specific}:-', text) else 'INSTALLER_DOWNLOAD_URL'


def formula_version(text):
    version = re.search(r'^  version "([^"]+)"$', text, re.M)
    if not version:
        raise ValueError('Formula does not declare a version')
    return version[1]


def formula_targets(text):
    """Return the archive targets referenced by the formula's download URLs."""
    targets = set()
    for url in re.findall(r'url "([^"]+)"', text):
        match = re.search(rf'/{re.escape(PACKAGE)}-([^/]+)\.tar\.xz$', url)
        if match:
            targets.add(match[1])
    return targets


def rewrite_formula(text, version, base_url):
    """Redirect the formula's release download URLs to staged candidate assets.

    Only the URL prefix changes: the declared version, SHA-256 lines, and all
    installation logic stay exactly as cargo-dist generated them, so the
    checksums must still match the local archives byte for byte.
    """
    validate_version(version)
    if formula_version(text) != version:
        raise ValueError('Formula version does not match the expected release')
    original = f'{DOWNLOAD_PREFIX}{version}/'
    if original not in text:
        raise ValueError('Generated formula has an unexpected release URL')
    checksums = re.findall(r'sha256 "([0-9a-f]{64})"', text)
    if not checksums:
        raise ValueError('Generated formula has no sha256 checksums')
    rewritten = text.replace(original, base_url.rstrip('/') + '/')
    if re.findall(r'sha256 "([0-9a-f]{64})"', rewritten) != checksums:
        raise ValueError('Formula rewrite changed the sha256 checksums')
    for target in formula_targets(rewritten):
        validate_target(target)
    return rewritten


def candidate_targets(target, digest_map):
    """Archives of the same machine a target's installer may have downloaded."""
    validate_target(target)
    darwin = 'apple-darwin' in target
    cpu = target.split('-')[0]
    return {t for t, _ in digest_map.items()
            if t.split('-')[0] == cpu and ('apple-darwin' in t) == darwin}


def confirm_installed_target(digest_map, installed_digest, expected_target, allowed=None):
    """Identify which candidate archive's executable was actually installed.

    `digest_map` maps archive target -> sha256 hex digest of its executable.
    """
    validate_target(expected_target)
    allowed = candidate_targets(expected_target, digest_map) if allowed is None else allowed
    matching = sorted(t for t in allowed if digest_map[t] == installed_digest)
    if len(matching) != 1:
        raise ValueError('Installed executable does not match exactly one candidate '
                         f'archive for {expected_target}: matched {matching or "nothing"}')
    return matching[0]


def run(command, *, env=None, expected=0, timeout=180):
    try:
        result = subprocess.run([str(item) for item in command], env=env,
                                capture_output=True, text=True, encoding='utf-8',
                                errors='replace', timeout=timeout)
    except subprocess.TimeoutExpired as error:
        def tail(output):
            if isinstance(output, bytes):
                output = output.decode('utf-8', errors='replace')
            return (output or '')[-16000:]
        raise RuntimeError(f'{command!r}: timed out after {timeout} seconds\n'
                           f'{tail(error.stdout)}\n{tail(error.stderr)}') from error
    if result.returncode != expected:
        raise RuntimeError(f'{command!r}: expected {expected}, got {result.returncode}\n'
                           f'{result.stdout}\n{result.stderr}')
    return result.stdout


def regular(path):
    if path.is_symlink() or not path.is_file():
        raise ValueError(f'Expected regular release asset: {path}')
    return path.read_bytes()


def stage(source, destination, version):
    """Verify every candidate archive and stage it (plus installer and formula)
    for serving. Returns the archive name -> binary bytes map."""
    validate_version(version)
    script = regular(source / INSTALLER)
    script_text = script.decode()
    prefix = installer_metadata(script_text, version)
    if not re.search(rf'\b{prefix}_DOWNLOAD_URL:-', script_text) and \
            not re.search(r'\bINSTALLER_DOWNLOAD_URL:-', script_text):
        raise ValueError('Installer has no download URL override to redirect')
    binary_data = {}
    destination.mkdir()
    (destination / INSTALLER).write_bytes(script)
    if (source / FORMULA).exists():
        formula_text = regular(source / FORMULA).decode()
        rewrite_formula(formula_text, version, 'unused')  # syntax/identity check
        (destination / FORMULA).write_bytes(regular(source / FORMULA))
    for target in sorted(TARGETS):
        name = archive_name(target)
        archive = regular(source / name)
        expected = parse_checksum(regular(source / (name + '.sha256')).decode(), name)
        verify_checksum(archive, name, expected)
        binary_data[target] = extract_binary(archive)
        (destination / name).write_bytes(archive)
        (destination / (name + '.sha256')).write_bytes(regular(source / (name + '.sha256')))
    return prefix, binary_data


def smoke(binary, version, fixture):
    """Exercise documented CLI behavior of the installed executable."""
    if run([binary, '--version']).strip() != f'{PACKAGE} {version}':
        raise ValueError('Installed version does not match the expected release')
    run([binary, '--help'])
    scan = [binary, '--root', fixture, '--fail-on-empty']
    run([*scan, '--threshold', '100'])
    # The strictest gate must fail: no CRAP score may survive a zero threshold.
    run([*scan, '--threshold', '0'], expected=1)


class QuietHandler(http.server.SimpleHTTPRequestHandler):
    def log_message(self, *_args):
        pass


def installer_env(prefix, script_text, base_url, prefix_dir, config_dir):
    env = {**os.environ, 'XDG_CONFIG_HOME': str(config_dir),
           'NO_PROXY': os.environ.get('NO_PROXY', '') + ',127.0.0.1,localhost'}
    env[download_override(prefix, script_text)] = base_url
    # The generic names keep working if cargo-dist renames the product prefix.
    env[f'{prefix}_INSTALL_DIR'] = str(prefix_dir)
    env['CARGO_DIST_FORCE_INSTALL_DIR'] = str(prefix_dir)
    env[f'{prefix}_NO_MODIFY_PATH'] = '1'
    env['INSTALLER_NO_MODIFY_PATH'] = '1'
    env.pop(f'{prefix}_UNMANAGED_INSTALL', None)
    env.pop(f'{prefix}_DISABLE_UPDATE', None)
    return env


def verify_shell(args, root, served, base, digest_map, prefix):
    target = None
    for scenario in (['fresh'] if not args.previous_assets else ['upgrade', 'fresh']):
        install_dir = root / scenario / 'bin with spaces'
        config = root / scenario / 'config'
        env = installer_env(prefix, args.script_text, base, install_dir, config)
        receipt = config / PACKAGE / f'{PACKAGE}-receipt.json'

        def install(expected_version, expected_digests):
            run(['sh', served / INSTALLER], env=env)
            executable = install_dir / BINARY
            digest = hashlib.sha256(regular(executable)).hexdigest()
            selected = confirm_installed_target(expected_digests, digest, args.target)
            parsed = json.loads(regular(receipt))
            if parsed['version'] != expected_version or \
                    Path(parsed['install_prefix']).resolve() != install_dir.resolve():
                raise ValueError('Installer receipt has incorrect version or prefix')
            smoke(executable, expected_version, args.fixture)
            return selected

        if scenario == 'upgrade':
            previous_digests = {
                t: hashlib.sha256(digest_for(args.previous_assets, args.previous_version, t)).hexdigest()
                for t in candidate_targets(args.target, digest_map)}
            install(args.previous_version, previous_digests)
        target = install(args.version, digest_map)
        if not args.previous_assets:
            # A fresh reinstall must succeed over an existing installation.
            install(args.version, digest_map)
    print(f'Installed archive target confirmed by byte identity: {target}')


def digest_for(assets, version, target):
    """Digest of the binary inside a previous release's verified archive."""
    validate_version(version)
    name = archive_name(target)
    archive = regular(assets / name)
    verify_checksum(archive, name, parse_checksum(regular(assets / (name + '.sha256')).decode(), name))
    return extract_binary(archive)


def verify_homebrew(args, root, served, digest_map):
    brew = shutil.which('brew')
    if not brew:
        raise ValueError('Homebrew must be installed before running formula verification')
    env = {**os.environ, 'HOMEBREW_NO_AUTO_UPDATE': '1', 'HOMEBREW_NO_INSTALL_FROM_API': '1',
           'HOMEBREW_NO_ANALYTICS': '1', 'HOMEBREW_NO_INSTALL_CLEANUP': '1',
           'HOMEBREW_NO_ENV_HINTS': '1'}
    existing = run([brew, 'list', '--formula', '--versions'], env=env)
    if any(line.split()[0] == PACKAGE for line in existing.splitlines() if line.strip()):
        raise ValueError(f'Refusing to replace an existing Homebrew {PACKAGE} installation')
    tap = f'{PACKAGE}-test/run-{os.getpid()}'
    formula = f'{tap}/{PACKAGE}'
    # The disposable formula directory needs no repository or template commit.
    run([brew, 'tap-new', '--no-git', tap], env=env)
    tap_root = Path(run([brew, '--repository', tap], env=env).strip())
    try:
        text = (served / FORMULA).read_text(encoding='utf-8')
        # file:// keeps everything offline; the sha256 lines still verify the
        # same candidate archives they were generated for.
        base = Path(served).resolve().as_uri()
        rewritten = rewrite_formula(text, args.version, base)
        for target in formula_targets(rewritten):
            if not (served / archive_name(target)).exists():
                raise ValueError(f'Formula references unstaged archive: {target}')
        (tap_root / 'Formula').mkdir(parents=True, exist_ok=True)
        (tap_root / 'Formula' / FORMULA).write_text(rewritten, encoding='utf-8')
        run([brew, 'install', '--formula', formula], env=env, timeout=600)
        prefix = Path(run([brew, '--prefix', formula], env=env).strip())
        binary = prefix / 'bin' / BINARY
        digest = hashlib.sha256(regular(binary)).hexdigest()
        allowed = {t for t in candidate_targets(args.target, digest_map)
                   if t in formula_targets(rewritten)}
        if not allowed:
            raise ValueError('The rewritten formula has no archive for this platform')
        matched = confirm_installed_target(digest_map, digest, args.target, allowed)
        smoke(binary, args.version, args.fixture)
        if args.brew_test:
            # Best effort: the generated formula carries no test block, and
            # audit needs network access, so neither may work offline.
            try:
                run([brew, 'test', '--formula', formula], env=env)
                print('brew test passed')
            except Exception as error:  # noqa: BLE001
                print(f'note: brew test is not usable offline here: {error}')
        print(f'Homebrew formula installed from the rewritten tap '
              f'(archive target confirmed by byte identity: {matched})')
    finally:
        try:
            remaining = run([brew, 'list', '--formula', '--versions'], env=env)
            if any(line.split()[0] == PACKAGE for line in remaining.splitlines() if line.strip()):
                run([brew, 'uninstall', '--force', formula], env=env)
        finally:
            run([brew, 'untap', tap], env=env)


def verify(args):
    with tempfile.TemporaryDirectory(prefix=f'{PACKAGE}-install-') as temporary:
        root = Path(temporary).resolve()
        served = root / 'assets'
        prefix, binary_data = stage(args.assets, served, args.version)
        digest_map = {t: hashlib.sha256(b).hexdigest() for t, b in binary_data.items()}
        args.script_text = (served / INSTALLER).read_text(encoding='utf-8')
        if args.installer == 'homebrew':
            verify_homebrew(args, root, served, digest_map)
        else:
            server = http.server.ThreadingHTTPServer(
                ('127.0.0.1', 0), partial(QuietHandler, directory=str(served)))
            worker = threading.Thread(target=server.serve_forever, daemon=True)
            worker.start()
            try:
                base = f'http://127.0.0.1:{server.server_port}'
                verify_shell(args, root, served, base, digest_map, prefix)
            finally:
                server.shutdown()
                server.server_close()
                worker.join()
    print(f'PASS {args.installer}/{args.target}: checksum-verified candidate install, '
          'archive identity, version/help/scan gates')


def build_parser():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--assets', type=Path, required=True,
                        help='directory with the candidate release assets')
    parser.add_argument('--version', required=True, help='candidate version, e.g. 0.1.0')
    parser.add_argument('--target', choices=sorted(TARGETS), required=True)
    parser.add_argument('--installer', choices=['shell', 'homebrew'], required=True)
    parser.add_argument('--fixture', type=Path, default=Path('examples/Fixture'),
                        help='C# fixture scanned by the installed CLI (default: examples/Fixture)')
    parser.add_argument('--previous-assets', type=Path,
                        help='optional: assets of a previously released version to upgrade from')
    parser.add_argument('--previous-version',
                        help='optional: version of --previous-assets (unused today)')
    parser.add_argument('--brew-test', action='store_true',
                        help='also attempt brew test (best effort, no generated test block)')
    return parser


def main():
    parser = build_parser()
    args = parser.parse_args()
    validate_version(args.version)
    if bool(args.previous_assets) != bool(args.previous_version):
        parser.error('--previous-assets and --previous-version must be given together')
    if args.previous_assets and args.previous_version == args.version:
        raise ValueError('Upgrade verification needs two different release versions')
    verify(args)


if __name__ == '__main__':
    main()
