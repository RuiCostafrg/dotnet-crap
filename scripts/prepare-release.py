#!/usr/bin/env python3
"""Validate the exact release asset set and its checksums, then copy it to a new directory.

Runs before any privileged job sees the assets: a missing, extra-named,
non-regular or checksum-mismatched file stops the release.
"""
import argparse
import hashlib
from pathlib import Path
import re
import shutil

APP = 'dotnet-crap'
UNIX_TARGETS = (
    'aarch64-apple-darwin', 'x86_64-apple-darwin',
    'aarch64-unknown-linux-gnu', 'x86_64-unknown-linux-gnu',
    'aarch64-unknown-linux-musl', 'x86_64-unknown-linux-musl',
)
WINDOWS_TARGET = 'x86_64-pc-windows-msvc'
VERSION = re.compile(r'[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?')


def checksummed_assets(version):
    if not VERSION.fullmatch(version):
        raise ValueError(f'Invalid version: {version}')
    return ([f'{APP}-{target}.tar.xz' for target in UNIX_TARGETS]
            + [f'{APP}-{WINDOWS_TARGET}.zip', f'{APP}.{version}.nupkg'])


def asset_names(version):
    archives = checksummed_assets(version)
    return archives + [name + '.sha256' for name in archives] + [
        f'{APP}-installer.sh', f'{APP}.rb', 'sha256.sum', 'dist-manifest.json']


def expected_digest(path, name):
    fields = path.read_text(encoding='utf-8').split()
    if (len(fields) != 2 or fields[1].lstrip('*') != name
            or not re.fullmatch(r'[0-9a-f]{64}', fields[0])):
        raise ValueError(f'Invalid checksum file for {name}')
    return fields[0]


def validate(source, version):
    names = asset_names(version)
    for name in names:
        path = source / name
        if path.is_symlink() or not path.is_file():
            raise ValueError(f'Missing or non-regular release asset: {name}')
    for name in checksummed_assets(version):
        digest = hashlib.sha256((source / name).read_bytes()).hexdigest()
        if digest != expected_digest(source / (name + '.sha256'), name):
            raise ValueError(f'Checksum mismatch: {name}')
    # cargo-dist's combined list must agree with every archive it covers.
    listed = {}
    for line in (source / 'sha256.sum').read_text(encoding='utf-8').splitlines():
        fields = line.split()
        if len(fields) == 2:
            listed[fields[1].lstrip('*')] = fields[0]
    for name in checksummed_assets(version)[:-1]:
        if listed.get(name) != expected_digest(source / (name + '.sha256'), name):
            raise ValueError(f'sha256.sum disagrees for {name}')
    return names


def prepare(source, destination, version):
    names = validate(source, version)
    destination.mkdir()
    for name in names:
        shutil.copyfile(source / name, destination / name)
    return names


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('destination', type=Path)
    parser.add_argument('--version', required=True)
    arguments = parser.parse_args()
    for asset in prepare(arguments.source, arguments.destination, arguments.version):
        print(asset)
