"""Offline tests for scripts/prepare-release.py and scripts/stage-dotnet-tool.py."""
import hashlib
import importlib.util
import io
from pathlib import Path
import tarfile
import tempfile
import unittest
import zipfile

ROOT = Path(__file__).resolve().parents[1]


def load(name):
    spec = importlib.util.spec_from_file_location(name.replace('-', '_'), ROOT / 'scripts' / f'{name}.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


prepare = load('prepare-release')
stage = load('stage-dotnet-tool')
VERSION = '1.2.3'


def checksum(directory, name):
    digest = hashlib.sha256((directory / name).read_bytes()).hexdigest()
    (directory / f'{name}.sha256').write_text(f'{digest} *{name}\n')
    return f'{digest} *{name}'


def tar_xz(path, target, payload):
    with tarfile.open(path, 'w:xz') as archive:
        for name, data in ((f'dotnet-crap-{target}/dotnet-crap', payload), (f'dotnet-crap-{target}/LICENSE', b'MIT')):
            entry = tarfile.TarInfo(name)
            entry.size = len(data)
            entry.mode = 0o755
            archive.addfile(entry, io.BytesIO(data))


def release_dir(directory):
    lines = []
    for target in prepare.UNIX_TARGETS:
        name = f'dotnet-crap-{target}.tar.xz'
        tar_xz(directory / name, target, f'binary for {target}'.encode())
        lines.append(checksum(directory, name))
    name = 'dotnet-crap-x86_64-pc-windows-msvc.zip'
    with zipfile.ZipFile(directory / name, 'w') as archive:
        archive.writestr('dotnet-crap.exe', b'MZ windows')
        archive.writestr('LICENSE', b'MIT')
    lines.append(checksum(directory, name))
    (directory / f'dotnet-crap.{VERSION}.nupkg').write_bytes(b'nupkg')
    checksum(directory, f'dotnet-crap.{VERSION}.nupkg')
    (directory / 'sha256.sum').write_text('\n'.join(lines) + '\n')
    for name in ('dotnet-crap-installer.sh', 'dotnet-crap.rb', 'dist-manifest.json'):
        (directory / name).write_text(name)


class PrepareReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.source = self.root / 'source'
        self.source.mkdir()
        release_dir(self.source)

    def tearDown(self):
        self.temporary.cleanup()

    def test_copies_exact_asset_set(self):
        names = prepare.prepare(self.source, self.root / 'out', VERSION)
        self.assertEqual(sorted(p.name for p in (self.root / 'out').iterdir()), sorted(names))
        self.assertEqual(len(names), 2 * 8 + 4)

    def test_rejects_missing_asset(self):
        (self.source / 'dotnet-crap.rb').unlink()
        with self.assertRaisesRegex(ValueError, 'dotnet-crap.rb'):
            prepare.prepare(self.source, self.root / 'out', VERSION)
        self.assertFalse((self.root / 'out').exists())

    def test_rejects_checksum_mismatch(self):
        (self.source / f'dotnet-crap.{VERSION}.nupkg').write_bytes(b'tampered')
        with self.assertRaisesRegex(ValueError, 'Checksum mismatch'):
            prepare.prepare(self.source, self.root / 'out', VERSION)

    def test_rejects_checksum_for_another_file(self):
        name = 'dotnet-crap-x86_64-apple-darwin.tar.xz'
        text = (self.source / f'{name}.sha256').read_text().replace(name, 'other.tar.xz')
        (self.source / f'{name}.sha256').write_text(text)
        with self.assertRaisesRegex(ValueError, 'Invalid checksum file'):
            prepare.prepare(self.source, self.root / 'out', VERSION)

    def test_rejects_disagreeing_combined_list(self):
        (self.source / 'sha256.sum').write_text('0' * 64 + ' *dotnet-crap-x86_64-apple-darwin.tar.xz\n')
        with self.assertRaisesRegex(ValueError, 'sha256.sum disagrees'):
            prepare.prepare(self.source, self.root / 'out', VERSION)

    def test_rejects_symlinked_asset(self):
        manifest = self.source / 'dist-manifest.json'
        manifest.unlink()
        manifest.symlink_to(self.source / 'dotnet-crap.rb')
        with self.assertRaisesRegex(ValueError, 'non-regular'):
            prepare.prepare(self.source, self.root / 'out', VERSION)

    def test_rejects_invalid_version(self):
        with self.assertRaisesRegex(ValueError, 'Invalid version'):
            prepare.prepare(self.source, self.root / 'out', '1.2')


class StageDotnetToolTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.assets = self.root / 'assets'
        self.assets.mkdir()
        release_dir(self.assets)

    def tearDown(self):
        self.temporary.cleanup()

    def test_stages_musl_linux_and_every_rid(self):
        rids = stage.stage(self.assets, self.root / 'stage')
        self.assertEqual(rids, sorted(stage.RIDS))
        self.assertEqual((self.root / 'stage/linux-x64/dotnet-crap').read_bytes(),
                         b'binary for x86_64-unknown-linux-musl')
        self.assertEqual((self.root / 'stage/osx-arm64/dotnet-crap').read_bytes(),
                         b'binary for aarch64-apple-darwin')
        self.assertEqual((self.root / 'stage/win-x64/dotnet-crap.exe').read_bytes(), b'MZ windows')
        self.assertEqual((self.root / 'stage/linux-arm64/dotnet-crap').stat().st_mode & 0o777, 0o755)

    def test_rejects_checksum_mismatch_without_partial_stage(self):
        (self.assets / 'dotnet-crap-aarch64-apple-darwin.tar.xz.sha256').write_text(
            '0' * 64 + ' *dotnet-crap-aarch64-apple-darwin.tar.xz\n')
        with self.assertRaisesRegex(ValueError, 'Checksum mismatch'):
            stage.stage(self.assets, self.root / 'stage')
        self.assertFalse((self.root / 'stage').exists())

    def test_rejects_ambiguous_archive(self):
        name = 'dotnet-crap-x86_64-unknown-linux-musl.tar.xz'
        with tarfile.open(self.assets / name, 'w:xz') as archive:
            for prefix in ('a', 'b'):
                entry = tarfile.TarInfo(f'{prefix}/dotnet-crap')
                entry.size = 1
                archive.addfile(entry, io.BytesIO(b'x'))
        checksum(self.assets, name)
        with self.assertRaisesRegex(ValueError, 'Expected one'):
            stage.stage(self.assets, self.root / 'stage')

    def test_refuses_existing_stage(self):
        (self.root / 'stage').mkdir()
        with self.assertRaises(FileExistsError):
            stage.stage(self.assets, self.root / 'stage')


if __name__ == '__main__':
    unittest.main()
