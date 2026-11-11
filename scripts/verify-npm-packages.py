#!/usr/bin/env python3
"""Check npm tarballs against the release archives they claim to repackage, byte for byte."""
import argparse
import importlib.util
import json
from pathlib import Path
import tarfile

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('npm_packages', ROOT / 'scripts/npm-packages.py')
npm = importlib.util.module_from_spec(spec)
spec.loader.exec_module(npm)
# Fields that could run code at install time or pull in anything beyond the platform packages.
FORBIDDEN = ('scripts', 'dependencies', 'peerDependencies', 'bundleDependencies', 'bundledDependencies',
             'gypfile', 'binding')


def read_tarball(path):
    """Return {relative path: (bytes, mode)} for a package tarball that holds only regular files."""
    files = {}
    with tarfile.open(path, mode='r:gz') as source:
        for member in source.getmembers():
            parts = member.name.split('/')
            if (not member.isfile() or len(parts) < 2 or parts[0] != 'package'
                    or any(p in ('', '.', '..') for p in parts[1:]) or '\\' in member.name):
                raise ValueError(f'{path.name}: unexpected tarball entry {member.name!r}')
            relative = '/'.join(parts[1:])
            if relative in files:
                raise ValueError(f'{path.name}: duplicate entry {relative}')
            files[relative] = (source.extractfile(member).read(), member.mode & 0o777)
    return files


def check_manifest(path, files, expected):
    fields = json.loads(files['package.json'][0])
    for key in FORBIDDEN:
        if key in fields:
            raise ValueError(f'{path.name}: package.json must not declare {key}')
    for key, value in expected.items():
        if fields.get(key) != value:
            raise ValueError(f'{path.name}: package.json {key} is {fields.get(key)!r}, expected {value!r}')


def same(path, files, expected):
    """Every file, including launcher, notices and manifest, is exactly what this checkout generates."""
    changed = sorted(name for name in files.keys() | expected.keys() if files.get(name) != expected.get(name))
    if changed:
        raise ValueError(f'{path.name}: files differ from the generated package: {changed}')


def verify(assets, packages, version, *, require_static=True):
    """Rebuild nothing: compare every shipped byte with its source in the verified release assets.

    REQUIRE_STATIC mirrors npm-packages.py's internal test seam; the CLI never sets it.
    """
    names = {npm.tarball_name(npm.platform_name(t), version) for t in npm.TARGETS} | {npm.tarball_name(npm.MAIN, version)}
    present = {p.name for p in packages.iterdir()}
    if present != names:
        raise ValueError(f'expected exactly {sorted(names)}, found {sorted(present)}')
    for target, (suffix, os_name, cpu) in npm.TARGETS.items():
        name = npm.platform_name(target)
        path = packages / npm.tarball_name(name, version)
        files = read_tarball(path)
        check_manifest(path, files, {'name': name, 'version': version, 'os': [os_name], 'cpu': [cpu],
                                     'repository': npm.REPOSITORY, 'preferUnplugged': True})
        if 'bin' in json.loads(files['package.json'][0]):
            raise ValueError(f'{path.name}: platform packages must not declare bin')
        binary = f'bin/{npm.BINARY}'
        if binary not in files or files[binary][0] != npm.executable(assets, target, require_static=require_static):
            raise ValueError(f'{path.name}: {binary} differs from {npm.archive_name(target)}')
        if files[binary][1] != 0o755:
            raise ValueError(f'{path.name}: {binary} is not executable')
        allowed = {'package.json', 'README.md', 'LICENSE', binary}
        if 'linux' in target:
            allowed.add('musl-COPYRIGHT')
        extra = set(files) - allowed
        if extra:
            raise ValueError(f'{path.name}: unexpected files {sorted(extra)}')
        same(path, files, npm.platform_files(target, version, files[binary][0])[1])
    path = packages / npm.tarball_name(npm.MAIN, version)
    files = read_tarball(path)
    check_manifest(path, files, {
        'name': npm.MAIN, 'version': version, 'bin': npm.BIN, 'main': 'lib/index.js',
        'repository': npm.REPOSITORY,
        'optionalDependencies': {npm.platform_name(t): version for t in npm.TARGETS}})
    for relative, mode in npm.LAUNCHER.items():
        if relative not in files or files[relative][1] != mode:
            raise ValueError(f'{path.name}: missing launcher file {relative} or wrong mode')
    extra = set(files) - {'package.json', 'README.md', 'LICENSE', *npm.LAUNCHER}
    if extra:
        raise ValueError(f'{path.name}: unexpected files {sorted(extra)}')
    same(path, files, npm.main_files(version))
    print(f'npm packages {version}: executables match the release archives')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--assets', type=Path, required=True, help='release assets with .sha256 sidecars')
    parser.add_argument('--packages', type=Path, required=True, help='directory holding only the npm tarballs')
    parser.add_argument('--version', required=True)
    args = parser.parse_args()
    verify(args.assets, args.packages, args.version)
