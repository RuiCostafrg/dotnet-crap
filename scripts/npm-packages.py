#!/usr/bin/env python3
"""Build deterministic npm package tarballs from verified release archives (artifacts are data)."""
import argparse
import gzip
import hashlib
import io
import json
from pathlib import Path
import re
import secrets
import shutil
import struct
import tarfile

ROOT = Path(__file__).resolve().parents[1]

MAIN = 'dotnet-crap'
BINARY = 'dotnet-crap'
# Release target -> (npm platform suffix, os, cpu). Linux uses the static musl
# build, so there is one package per CPU and no libc field. Windows is not an
# npm channel: the .NET tool and the GitHub archive serve it.
TARGETS = {
    'x86_64-unknown-linux-musl': ('linux-x64', 'linux', 'x64'),
    'aarch64-unknown-linux-musl': ('linux-arm64', 'linux', 'arm64'),
    'x86_64-apple-darwin': ('darwin-x64', 'darwin', 'x64'),
    'aarch64-apple-darwin': ('darwin-arm64', 'darwin', 'arm64'),
}
# ELF e_machine values and Mach-O cputype values, per release target.
ELF_MACHINE = {'x86_64-unknown-linux-musl': 62, 'aarch64-unknown-linux-musl': 183}
MACHO = {'x86_64-apple-darwin': 0x01000007, 'aarch64-apple-darwin': 0x0100000C}
MACHO_EXECUTE = 2
REPOSITORY = {'type': 'git', 'url': 'git+https://github.com/RuiCostafrg/dotnet-crap.git'}
HOMEPAGE = 'https://github.com/RuiCostafrg/dotnet-crap#readme'
BUGS = {'url': 'https://github.com/RuiCostafrg/dotnet-crap/issues'}
LAUNCHER = {'bin/dotnet-crap.js': 0o755, 'lib/index.js': 0o644}
BIN = {BINARY: 'bin/dotnet-crap.js'}
# npm pack's fixed timestamp (1985-10-26T08:15:00Z) keeps tarballs reproducible.
MTIME = 499162500
MAX_BINARY = 128 * 1024 * 1024
SEMVER = re.compile(r'(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)'
                    r'(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?')


def platform_name(target):
    return f'{MAIN}-{TARGETS[target][0]}'


def archive_name(target):
    return f'{MAIN}-{target}.tar.xz'


def member_name(target):
    """The one executable path inside a release archive (cargo-dist layout)."""
    return f'{MAIN}-{target}/{BINARY}'


def tarball_name(name, version):
    return f'{name}-{version}.tgz'


def verified(path):
    """Read a regular asset only after its filename-bound SHA-256 sidecar matches."""
    checksum = path.with_name(path.name + '.sha256')
    if path.is_symlink() or not path.is_file() or checksum.is_symlink() or not checksum.is_file():
        raise ValueError(f'expected regular asset and checksum: {path.name}')
    fields = checksum.read_text(encoding='utf-8').split()
    data = path.read_bytes()
    if (len(fields) != 2 or fields[1].lstrip('*') != path.name
            or fields[0] != hashlib.sha256(data).hexdigest()):
        raise ValueError(f'asset checksum or filename mismatch: {path.name}')
    return data


def static_elf(data, machine, *, require_static=True):
    """Reject wrong architectures and, unless disabled by a test, ELF interpreters and needed libraries."""
    if len(data) < 64 or data[:7] != b'\x7fELF\x02\x01\x01':
        raise ValueError('expected little-endian ELF64')
    kind, actual_machine = struct.unpack_from('<HH', data, 16)
    offset = struct.unpack_from('<Q', data, 32)[0]
    size, count = struct.unpack_from('<HH', data, 54)
    if kind not in (2, 3) or actual_machine != machine or size != 56 or not count or offset + size * count > len(data):
        raise ValueError('invalid ELF architecture or program headers')
    if not require_static:
        return
    for index in range(count):
        entry = offset + index * size
        category = struct.unpack_from('<I', data, entry)[0]
        start, length = struct.unpack_from('<Q', data, entry + 8)[0], struct.unpack_from('<Q', data, entry + 32)[0]
        if start + length > len(data) or category == 3:
            raise ValueError('ELF interpreter or invalid segment bounds: the binary is not static')
        if category == 2:
            if length % 16:
                raise ValueError('invalid dynamic segment')
            for position in range(start, start + length, 16):
                tag = struct.unpack_from('<q', data, position)[0]
                if tag == 0:
                    break
                if tag == 1:
                    raise ValueError('ELF needs a dynamic library: the binary is not static')


def check_executable(data, target, *, require_static=True):
    """Reject an executable built for another platform or architecture."""
    if 'linux' in target:
        static_elf(data, ELF_MACHINE[target], require_static=require_static)
    else:
        if len(data) < 16 or data[:4] != b'\xcf\xfa\xed\xfe':
            raise ValueError(f'expected a 64-bit little-endian Mach-O image for {target}')
        cputype = struct.unpack_from('<I', data, 4)[0]
        filetype = struct.unpack_from('<I', data, 12)[0]
        if cputype != MACHO[target] or filetype != MACHO_EXECUTE:
            raise ValueError(f'expected a Mach-O executable for {target}')


def executable(assets, target, *, require_static=True):
    """Read exactly one bounded regular executable from a checksum-verified archive, without extracting."""
    data = verified(assets / archive_name(target))
    wanted = member_name(target)
    with tarfile.open(fileobj=io.BytesIO(data), mode='r:xz') as source:
        matches = [m for m in source.getmembers() if m.name == wanted]
        if len(matches) != 1 or not matches[0].isfile() or matches[0].size > MAX_BINARY:
            raise ValueError(f'{archive_name(target)}: expected one bounded regular {wanted}')
        binary = source.extractfile(matches[0]).read()
    check_executable(binary, target, require_static=require_static)
    return binary


def manifest(fields):
    return (json.dumps(fields, indent=2) + '\n').encode()


def common(name, version, description):
    return {'name': name, 'version': version, 'description': description, 'license': 'MIT',
            'homepage': HOMEPAGE, 'bugs': BUGS, 'repository': REPOSITORY}


def platform_files(target, version, binary):
    suffix, os_name, cpu = TARGETS[target]
    name = platform_name(target)
    fields = common(name, version, f'The dotnet-crap scanner executable for {suffix}. Install {MAIN} instead.')
    # preferUnplugged lets Yarn Plug'n'Play run the executable from disk.
    fields.update({'os': [os_name], 'cpu': [cpu], 'preferUnplugged': True})
    readme = (f'# {name}\n\nThe prebuilt `{BINARY}` {version} executable for {suffix}, copied byte for byte\n'
              f'from the checksum-verified `{archive_name(target)}` release asset.\n\n'
              f'Do not depend on this package directly: install [`{MAIN}`](https://www.npmjs.com/package/{MAIN}),\n'
              f'which selects the right platform package and provides the `{BINARY}` command.\n')
    files = {'package.json': (manifest(fields), 0o644), 'README.md': (readme.encode(), 0o644),
             'LICENSE': ((ROOT / 'LICENSE').read_bytes(), 0o644),
             f'bin/{BINARY}': (binary, 0o755)}
    if 'linux' in target:
        files['musl-COPYRIGHT'] = ((ROOT / 'packaging/licenses/musl-COPYRIGHT').read_bytes(), 0o644)
    return name, files


def main_fields(version):
    fields = common(MAIN, version, 'CRAP scores for C# methods, from cyclomatic complexity and '
                    'source-bound coverage: the dotnet-crap scanner')
    fields.update({
        'keywords': ['csharp', 'dotnet', 'crap', 'complexity', 'coverage', 'quality-gate'],
        'type': 'commonjs', 'main': 'lib/index.js', 'bin': BIN,
        # The launcher runs on any maintained Node.js.
        'engines': {'node': '>=18'},
        'optionalDependencies': {platform_name(t): version for t in TARGETS},
    })
    return fields


def main_files(version):
    files = {'package.json': (manifest(main_fields(version)), 0o644),
             'README.md': ((ROOT / 'packaging/npm/README.md').read_bytes(), 0o644),
             'LICENSE': ((ROOT / 'LICENSE').read_bytes(), 0o644)}
    for relative, mode in LAUNCHER.items():
        files[relative] = ((ROOT / 'packaging/npm' / relative).read_bytes(), mode)
    return files


def tarball(files):
    """Serialize files under package/ with fixed metadata: identical inputs give identical bytes."""
    raw = io.BytesIO()
    with tarfile.open(fileobj=raw, mode='w', format=tarfile.USTAR_FORMAT) as output:
        for relative in sorted(files):
            data, mode = files[relative]
            entry = tarfile.TarInfo('package/' + relative)
            entry.size, entry.mode, entry.mtime = len(data), mode, MTIME
            entry.uid = entry.gid = 0
            entry.uname = entry.gname = ''
            output.addfile(entry, io.BytesIO(data))
    compressed = io.BytesIO()
    with gzip.GzipFile(fileobj=compressed, mode='wb', mtime=0, compresslevel=9) as stream:
        stream.write(raw.getvalue())
    return compressed.getvalue()


def packages(assets, version, *, require_static=True):
    """Return {tarball filename: bytes}, platform packages first and the main package last.

    REQUIRE_STATIC is an internal test seam, never exposed on the command line:
    the end-to-end check on a glibc host packs a dynamically linked binary.
    """
    if not SEMVER.fullmatch(version):
        raise ValueError('invalid release version: SemVer required')
    result = {}
    for target in TARGETS:
        name, files = platform_files(target, version,
                                     executable(assets, target, require_static=require_static))
        result[tarball_name(name, version)] = tarball(files)
    result[tarball_name(MAIN, version)] = tarball(main_files(version))
    return result


def build(assets, version, output, *, require_static=True):
    """Write the tarballs to a new OUTPUT directory, all or nothing."""
    built = packages(assets, version, require_static=require_static)
    if output.exists() or output.is_symlink():
        raise FileExistsError(f'output already exists: {output}')
    # A plain mkdir under a random name: the kernel applies the umask and any
    # default ACL, exactly as for output.mkdir(), and the rename stays atomic.
    staging = output.parent / f'.npm-packages-{secrets.token_hex(8)}'
    staging.mkdir()
    try:
        for name, data in built.items():
            (staging / name).write_bytes(data)
        staging.rename(output)
    finally:
        shutil.rmtree(staging, ignore_errors=True)
    return [output / name for name in built]


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--assets', type=Path, required=True)
    parser.add_argument('--version', required=True)
    parser.add_argument('--output', type=Path, required=True, help='new directory for the tarballs')
    args = parser.parse_args()
    for path in build(args.assets, args.version, args.output):
        print(path)
