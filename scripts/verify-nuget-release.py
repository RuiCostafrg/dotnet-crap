#!/usr/bin/env python3
"""Download and verify the exact immutable release package; never build or execute it."""
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import tomllib
import xml.etree.ElementTree as ET
import zipfile

REPOSITORY = 'RuiCostafrg/dotnet-crap'


def command(*args):
    return subprocess.check_output(args, text=True).strip()


def identity(version, sha, environ):
    # Canonical stable versions only; prereleases need a separately reviewed policy.
    if not re.fullmatch(r'(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)', version):
        raise ValueError('Expected canonical stable version')
    if version == '0.1.0':
        raise ValueError('v0.1.0 is excluded; never modify or republish it')
    if not re.fullmatch(r'[0-9a-f]{40}', sha):
        raise ValueError('Expected full lowercase source SHA')
    if (environ['GITHUB_REPOSITORY'] != REPOSITORY
            or environ['GITHUB_REF'] != f'refs/tags/v{version}'
            or environ['GITHUB_SHA'] != sha):
        raise ValueError('Repository, dispatch tag, version or source SHA mismatch')


def release_metadata(release, version):
    if (release.get('immutable') is not True or release.get('draft') is not False
            or release.get('prerelease') is not False or release.get('tag_name') != f'v{version}'):
        raise ValueError('Expected exact immutable, published stable release')
    package = f'dotnet-crap.{version}.nupkg'
    for name in (package, package + '.sha256'):
        if sum(asset.get('name') == name for asset in release['assets']) != 1:
            raise ValueError(f'Expected exactly one release asset: {name}')
    return package


def package_metadata(path, version):
    digest = hashlib.sha256(path.read_bytes()).hexdigest()
    checksum = path.with_name(path.name + '.sha256').read_text().split()
    if checksum != [digest, path.name]:
        raise ValueError('Package SHA256 mismatch or noncanonical checksum')
    with zipfile.ZipFile(path) as archive:
        manifests = [entry for entry in archive.infolist() if entry.filename.endswith('.nuspec')]
        if len(manifests) != 1 or manifests[0].file_size > 1024 * 1024:
            raise ValueError('Expected one bounded nuspec')
        root = ET.fromstring(archive.read(manifests[0]))
        # NuGet manifests may use the default XML namespace.
        for element in root.iter():
            element.tag = element.tag.rsplit('}', 1)[-1]
        metadata = root.find('metadata')
        if (metadata is None or metadata.findtext('id') != 'dotnet-crap'
                or metadata.findtext('version') != version
                or not any(p.get('name') == 'DotnetTool'
                           for p in metadata.findall('packageTypes/packageType'))):
            raise ValueError('Package ID, version or DotnetTool type mismatch')
    return digest


def verify(environ, destination=Path('verified-nuget')):
    version, sha = environ['VERSION'], environ['SOURCE_SHA']
    identity(version, sha, environ)
    tag = f'v{version}'
    if command('git', 'rev-parse', 'HEAD') != sha:
        raise ValueError('Checkout does not match source SHA')
    command('git', 'merge-base', '--is-ancestor', sha, 'origin/main')
    if command('gh', 'api', f'repos/{REPOSITORY}/commits/{tag}', '--jq', '.sha') != sha:
        raise ValueError('Remote tag does not match source SHA')
    manifest = tomllib.loads(command('git', 'show', f'{sha}:Cargo.toml'))
    if manifest['package']['version'] != version:
        raise ValueError('Cargo source version does not match release version')
    release = json.loads(command('gh', 'api', f'repos/{REPOSITORY}/releases/tags/{tag}'))
    package = release_metadata(release, version)
    command('gh', 'release', 'verify', tag, '--repo', REPOSITORY)
    # Refuse to reuse a directory (including a dangling symlink) on retry.
    destination.mkdir()
    for name in (package, package + '.sha256'):
        command('gh', 'release', 'download', tag, '--repo', REPOSITORY,
                '--pattern', name, '--dir', str(destination))
        path = destination / name
        command('gh', 'release', 'verify-asset', tag, str(path), '--repo', REPOSITORY)
        command('gh', 'attestation', 'verify', str(path), '--repo', REPOSITORY,
                '--cert-identity', f'https://github.com/{REPOSITORY}/.github/workflows/release.yml@refs/tags/{tag}',
                '--cert-oidc-issuer', 'https://token.actions.githubusercontent.com',
                '--source-ref', f'refs/tags/{tag}', '--source-digest', sha,
                '--deny-self-hosted-runners')
    digest = package_metadata(destination / package, version)
    print(f'Verified {package}, source {sha}, SHA256 {digest}')


if __name__ == '__main__':
    verify(os.environ)
