"""npm packages are deterministic, match the checksum-verified archives, and reject foreign or dynamic executables."""
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import struct
import tarfile
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


def module(name, path):
    """Load a tooling script without invoking its command-line entry point."""
    spec = importlib.util.spec_from_file_location(name, ROOT / path)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


npm = module("npm_packages", "scripts/npm-packages.py")
verify = module("verify_npm_packages", "scripts/verify-npm-packages.py")
install = module("verify_npm_install", "scripts/verify-npm-install.py")

X86_LINUX = 'x86_64-unknown-linux-musl'
ARM_LINUX = 'aarch64-unknown-linux-musl'
X86_DARWIN = 'x86_64-apple-darwin'
ARM_DARWIN = 'aarch64-apple-darwin'
VERSION = '1.2.3'
PT_LOAD, PT_DYNAMIC, PT_INTERP = 1, 2, 3
DT_NULL, DT_NEEDED = 0, 1


def elf(machine, segments=(), kind=2):
    """A little-endian ELF64 image: one PT_LOAD plus SEGMENTS, each (type, payload). Never executed."""
    count = len(segments) + 1
    body = bytearray(64 + 56 * count)
    body[0:7] = b'\x7fELF\x02\x01\x01'
    struct.pack_into('<HHI', body, 16, kind, machine, 1)
    struct.pack_into('<Q', body, 32, 64)
    struct.pack_into('<HH', body, 54, 56, count)
    for index, (kind_of_segment, payload) in enumerate([(PT_LOAD, b''), *segments]):
        offset = len(body)
        body += payload
        entry = 64 + 56 * index
        struct.pack_into('<II', body, entry, kind_of_segment, 4)
        struct.pack_into('<QQQQQQ', body, entry + 8, offset, 0, 0, len(payload), len(payload), 8)
    return bytes(body)


def static_linux(machine):
    return elf(machine)


def macho(cputype, filetype=2):
    """A little-endian 64-bit Mach-O executable header with the given CPU type."""
    return struct.pack('<IIIII', 0xFEEDFACF, cputype, 0, filetype, 0) + b'\0' * 32


def archive(target, binary, member=None):
    """Bytes of a release archive in the cargo-dist layout, with its checksum sidecar contents."""
    buffer = io.BytesIO()
    name = member or npm.member_name(target)
    with tarfile.open(fileobj=buffer, mode='w:xz') as bundle:
        for path, data, mode in [(name, binary, 0o755), (f'dotnet-crap-{target}/LICENSE', b'notice', 0o644)]:
            info = tarfile.TarInfo(path)
            info.size, info.mode = len(data), mode
            bundle.addfile(info, io.BytesIO(data))
    return buffer.getvalue()


def valid_binaries():
    return {
        X86_LINUX: static_linux(62),
        ARM_LINUX: static_linux(183),
        X86_DARWIN: macho(0x01000007),
        ARM_DARWIN: macho(0x0100000C),
    }


class PackageTestCase(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='npm-packages-test-')
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.assets = self.root / 'assets'
        self.assets.mkdir()
        for target, binary in valid_binaries().items():
            self.write_archive(target, archive(target, binary))

    def write_archive(self, target, data, checksum=None):
        path = self.assets / npm.archive_name(target)
        path.write_bytes(data)
        digest = checksum or hashlib.sha256(data).hexdigest()
        (self.assets / (path.name + '.sha256')).write_text(f'{digest}  {path.name}\n')
        return path

    def build(self, output_name='out'):
        return npm.build(self.assets, VERSION, self.root / output_name, require_static=True)

    def packed(self, name):
        """Path of a tarball in the built 'out' directory, building it on first use."""
        if not (self.root / 'out').exists():
            self.build()
        return self.root / 'out' / name

    def tarball_files(self, path):
        return verify.read_tarball(path)


class BuildTest(PackageTestCase):
    def test_build_is_deterministic(self):
        first = self.build('first')
        second = self.build('second')
        self.assertEqual([p.name for p in first], [p.name for p in second])
        for a, b in zip(first, second):
            self.assertEqual(a.read_bytes(), b.read_bytes(), a.name)

    def test_tarball_names_and_layout(self):
        paths = self.build()
        self.assertEqual(sorted(p.name for p in paths), sorted([
            f'dotnet-crap-linux-x64-{VERSION}.tgz', f'dotnet-crap-linux-arm64-{VERSION}.tgz',
            f'dotnet-crap-darwin-x64-{VERSION}.tgz', f'dotnet-crap-darwin-arm64-{VERSION}.tgz',
            f'dotnet-crap-{VERSION}.tgz']))

    def test_platform_manifests(self):
        expected = {
            X86_LINUX: ('dotnet-crap-linux-x64', 'linux', 'x64'),
            ARM_LINUX: ('dotnet-crap-linux-arm64', 'linux', 'arm64'),
            X86_DARWIN: ('dotnet-crap-darwin-x64', 'darwin', 'x64'),
            ARM_DARWIN: ('dotnet-crap-darwin-arm64', 'darwin', 'arm64'),
        }
        for target, (name, os_name, cpu) in expected.items():
            path = self.packed(npm.tarball_name(name, VERSION))
            files = self.tarball_files(path)
            fields = json.loads(files['package.json'][0])
            self.assertEqual((fields['name'], fields['version'], fields['os'], fields['cpu']),
                             (name, VERSION, [os_name], [cpu]))
            self.assertTrue(fields['preferUnplugged'])
            self.assertNotIn('bin', fields)
            for key in ('scripts', 'dependencies', 'optionalDependencies'):
                self.assertNotIn(key, fields)
            self.assertEqual(files[f'bin/{npm.BINARY}'][1], 0o755)
            self.assertEqual(('musl-COPYRIGHT' in files), 'linux' in target)

    def test_main_manifest(self):
        files = self.tarball_files(self.packed(npm.tarball_name(npm.MAIN, VERSION)))
        fields = json.loads(files['package.json'][0])
        self.assertEqual(fields['name'], 'dotnet-crap')
        self.assertEqual(fields['bin'], {'dotnet-crap': 'bin/dotnet-crap.js'})
        self.assertEqual(fields['main'], 'lib/index.js')
        self.assertEqual(fields['engines'], {'node': '>=18'})
        self.assertEqual(fields['optionalDependencies'], {
            'dotnet-crap-linux-x64': VERSION, 'dotnet-crap-linux-arm64': VERSION,
            'dotnet-crap-darwin-x64': VERSION, 'dotnet-crap-darwin-arm64': VERSION})
        for key in verify.FORBIDDEN:
            self.assertNotIn(key, fields)
        self.assertEqual(files['bin/dotnet-crap.js'][1], 0o755)
        self.assertEqual(files['lib/index.js'][1], 0o644)
        self.assertEqual(set(files), {'package.json', 'README.md', 'LICENSE', 'bin/dotnet-crap.js', 'lib/index.js'})

    def test_binary_bytes_match_archive(self):
        files = self.tarball_files(self.packed(npm.tarball_name(npm.platform_name(ARM_LINUX), VERSION)))
        self.assertEqual(files[f'bin/{npm.BINARY}'][0], valid_binaries()[ARM_LINUX])

    def test_rejects_checksum_mismatch(self):
        self.write_archive(ARM_LINUX, archive(ARM_LINUX, valid_binaries()[ARM_LINUX]), checksum='0' * 64)
        with self.assertRaisesRegex(ValueError, 'checksum'):
            self.build()

    def test_rejects_missing_archive(self):
        (self.assets / npm.archive_name(X86_DARWIN)).unlink()
        with self.assertRaisesRegex(ValueError, 'regular asset'):
            self.build()

    def test_rejects_dynamic_elf(self):
        interpreter = elf(183, segments=[(PT_INTERP, b'/lib/ld-linux-aarch64.so.1\0')])
        self.write_archive(ARM_LINUX, archive(ARM_LINUX, interpreter))
        with self.assertRaisesRegex(ValueError, 'not static'):
            self.build()

    def test_rejects_dynamic_library_dependency(self):
        needed = struct.pack('<qQ', DT_NEEDED, 0) + struct.pack('<qQ', DT_NULL, 0)
        self.write_archive(X86_LINUX, archive(X86_LINUX, elf(62, segments=[(PT_DYNAMIC, needed)])))
        with self.assertRaisesRegex(ValueError, 'not static'):
            self.build()

    def test_dynamic_elf_only_with_the_internal_seam(self):
        interpreter = elf(183, segments=[(PT_INTERP, b'/lib/ld-linux-aarch64.so.1\0')])
        self.write_archive(ARM_LINUX, archive(ARM_LINUX, interpreter))
        self.assertIn(npm.tarball_name(npm.platform_name(ARM_LINUX), VERSION),
                      npm.packages(self.assets, VERSION, require_static=False))

    def test_rejects_wrong_linux_architecture(self):
        self.write_archive(ARM_LINUX, archive(ARM_LINUX, static_linux(62)))
        with self.assertRaisesRegex(ValueError, 'invalid ELF architecture'):
            self.build()

    def test_rejects_wrong_macho_cpu(self):
        self.write_archive(X86_DARWIN, archive(X86_DARWIN, macho(0x0100000C)))
        with self.assertRaisesRegex(ValueError, 'Mach-O'):
            self.build()

    def test_rejects_macho_that_is_not_an_executable(self):
        self.write_archive(ARM_DARWIN, archive(ARM_DARWIN, macho(0x0100000C, filetype=6)))
        with self.assertRaisesRegex(ValueError, 'Mach-O'):
            self.build()

    def test_rejects_wrong_member_layout(self):
        self.write_archive(X86_LINUX, archive(X86_LINUX, static_linux(62), member=f'dotnet-crap-{X86_LINUX}/other/dotnet-crap'))
        with self.assertRaisesRegex(ValueError, 'expected one bounded regular'):
            self.build()

    def test_rejects_invalid_version(self):
        for version in ('v1.2.3', '01.2.3', '1.2', '1.2.3+build'):
            with self.subTest(version=version), self.assertRaisesRegex(ValueError, 'SemVer'):
                npm.packages(self.assets, version)

    def test_output_must_be_new(self):
        (self.root / 'out').mkdir()
        with self.assertRaises(FileExistsError):
            self.build()


class VerifyTest(PackageTestCase):
    def setUp(self):
        super().setUp()
        self.packages = self.build()
        self.directory = self.root / 'out'

    def test_fresh_build_verifies(self):
        verify.verify(self.assets, self.directory, VERSION)

    def test_detects_tampered_platform_binary(self):
        target = npm.platform_name(ARM_LINUX)
        path = self.directory / npm.tarball_name(target, VERSION)
        files = self.tarball_files(path)
        binary = bytearray(files[f'bin/{npm.BINARY}'][0])
        binary[-1] ^= 0xFF
        files[f'bin/{npm.BINARY}'] = (bytes(binary), 0o755)
        path.write_bytes(npm.tarball(files))
        with self.assertRaisesRegex(ValueError, 'differs from'):
            verify.verify(self.assets, self.directory, VERSION)

    def test_detects_tampered_launcher(self):
        path = self.directory / npm.tarball_name(npm.MAIN, VERSION)
        files = self.tarball_files(path)
        files['lib/index.js'] = (b"process.exit(0);\n", 0o644)
        path.write_bytes(npm.tarball(files))
        with self.assertRaisesRegex(ValueError, 'differ from the generated package'):
            verify.verify(self.assets, self.directory, VERSION)

    def test_detects_forbidden_install_script(self):
        path = self.directory / npm.tarball_name(npm.MAIN, VERSION)
        files = self.tarball_files(path)
        fields = json.loads(files['package.json'][0])
        fields['scripts'] = {'postinstall': 'node evil.js'}
        files['package.json'] = (npm.manifest(fields), 0o644)
        path.write_bytes(npm.tarball(files))
        with self.assertRaisesRegex(ValueError, 'must not declare scripts'):
            verify.verify(self.assets, self.directory, VERSION)

    def test_detects_extra_package(self):
        (self.directory / 'dotnet-crap-win32-x64-1.2.3.tgz').write_bytes(b'')
        with self.assertRaisesRegex(ValueError, 'expected exactly'):
            verify.verify(self.assets, self.directory, VERSION)

    def test_detects_missing_package(self):
        (self.directory / npm.tarball_name(npm.platform_name(X86_DARWIN), VERSION)).unlink()
        with self.assertRaisesRegex(ValueError, 'expected exactly'):
            verify.verify(self.assets, self.directory, VERSION)


class InstallCheckTest(unittest.TestCase):
    def test_host_targets(self):
        self.assertEqual(install.host_target('Linux', 'x86_64'), X86_LINUX)
        self.assertEqual(install.host_target('Linux', 'aarch64'), ARM_LINUX)
        self.assertEqual(install.host_target('Darwin', 'arm64'), ARM_DARWIN)
        self.assertEqual(install.host_target('Darwin', 'x86_64'), X86_DARWIN)
        with self.assertRaises(ValueError):
            install.host_target('Windows', 'AMD64')

    def test_manager_overrides_pin_every_platform_tarball(self):
        packages = Path('/tmp/packed')
        for manager in ('npm', 'yarn', 'yarn-node-modules', 'bun'):
            fields = install.manifest(manager, VERSION, packages)
            pinned = fields.get('optionalDependencies') or fields.get('resolutions') or fields.get('overrides')
            self.assertEqual(set(pinned), {npm.platform_name(t) for t in npm.TARGETS}, manager)
            self.assertTrue(all(v.startswith('file:') for v in pinned.values()), manager)

    def test_yarn_node_modules_linker_is_configured(self):
        files = install.workspace_files('yarn-node-modules', VERSION, Path('/tmp/packed'))
        self.assertEqual(files['.yarnrc.yml'], 'nodeLinker: node-modules\n')
        self.assertNotIn('.yarnrc.yml', install.workspace_files('yarn', VERSION, Path('/tmp/packed')))

    def test_pnpm_overrides_live_in_workspace_file(self):
        files = install.workspace_files('pnpm', VERSION, Path('/tmp/packed'))
        overrides = json.loads(files['pnpm-workspace.yaml'])['overrides']
        self.assertEqual(set(overrides), {npm.platform_name(t) for t in npm.TARGETS})


if __name__ == '__main__':
    unittest.main()
