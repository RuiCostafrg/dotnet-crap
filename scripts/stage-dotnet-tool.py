#!/usr/bin/env python3
"""Stage the .NET tool's native executables from checksum-verified release archives.

Writes <stage>/<rid>/dotnet-crap[.exe] for every release RID, in the layout
scripts/pack-dotnet-tool.sh expects, so the NuGet package carries the same
executables as the GitHub release archives. Linux RIDs use the static musl
builds. Archives are data: only the single expected regular file is read.
"""
import argparse
import hashlib
import io
from pathlib import Path
import re
import tarfile
import zipfile

APP = 'dotnet-crap'
RIDS = {
    'linux-x64': 'x86_64-unknown-linux-musl',
    'linux-arm64': 'aarch64-unknown-linux-musl',
    'osx-x64': 'x86_64-apple-darwin',
    'osx-arm64': 'aarch64-apple-darwin',
    'win-x64': 'x86_64-pc-windows-msvc',
}
MAX_BINARY = 128 * 1024 * 1024


def verified(path):
    data = path.read_bytes()
    fields = path.with_name(path.name + '.sha256').read_text(encoding='utf-8').split()
    if (len(fields) != 2 or fields[1].lstrip('*') != path.name
            or not re.fullmatch(r'[0-9a-f]{64}', fields[0])):
        raise ValueError(f'Invalid checksum file for {path.name}')
    if hashlib.sha256(data).hexdigest() != fields[0]:
        raise ValueError(f'Checksum mismatch: {path.name}')
    return data


def executable(assets, target):
    windows = 'windows' in target
    archive = assets / f"{APP}-{target}.{'zip' if windows else 'tar.xz'}"
    data = verified(archive)
    binary = f'{APP}.exe' if windows else APP
    if windows:
        with zipfile.ZipFile(io.BytesIO(data)) as source:
            matches = [e for e in source.infolist()
                       if e.filename.replace('\\', '/').split('/')[-1] == binary]
            if len(matches) != 1 or matches[0].is_dir() or matches[0].file_size > MAX_BINARY:
                raise ValueError(f'Expected one bounded {binary} in {archive.name}')
            return source.read(matches[0])
    with tarfile.open(fileobj=io.BytesIO(data), mode='r:xz') as source:
        matches = [m for m in source.getmembers() if Path(m.name).name == binary]
        if len(matches) != 1 or not matches[0].isfile() or matches[0].size > MAX_BINARY:
            raise ValueError(f'Expected one bounded regular {binary} in {archive.name}')
        return source.extractfile(matches[0]).read()


def stage(assets, destination):
    if destination.exists() or destination.is_symlink():
        raise FileExistsError(f'Stage already exists: {destination}')
    staged = {rid: executable(assets, target) for rid, target in RIDS.items()}
    destination.mkdir(parents=True)
    for rid, data in staged.items():
        directory = destination / rid
        directory.mkdir()
        path = directory / (f'{APP}.exe' if rid.startswith('win-') else APP)
        path.write_bytes(data)
        path.chmod(0o755)
    return sorted(staged)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--assets', type=Path, required=True)
    parser.add_argument('--stage', type=Path, required=True, help='new directory')
    arguments = parser.parse_args()
    print(' '.join(stage(arguments.assets, arguments.stage)))
