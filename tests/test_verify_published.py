"""Offline tests for scripts/verify-published.py.

Every gh call is faked. The release fixture uses the real prepare-release
asset conventions with valid ELF/Mach-O headers.
"""
import contextlib
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import shutil
import struct
import tarfile
import tempfile
import unittest
import zipfile

ROOT = Path(__file__).resolve().parents[1]


def module(name, relative):
    spec = importlib.util.spec_from_file_location(name, ROOT / relative)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


script = module('verify_published', 'scripts/verify-published.py')
prepare = script.prepare

VERSION = '0.1.0'
TAG = 'v0.1.0'
SHA = 'a' * 40
OTHER_SHA = 'b' * 40
REPOSITORY = script.REPOSITORY


def static_elf(machine):
    body = bytearray(64)
    body[0:7] = b'\x7fELF\x02\x01\x01'
    struct.pack_into('<HHI', body, 16, 2, machine, 1)
    struct.pack_into('<Q', body, 32, 64)
    struct.pack_into('<HH', body, 54, 56, 1)
    body += b'\0' * 56
    struct.pack_into('<II', body, 64, 1, 4)
    struct.pack_into('<QQQQQQ', body, 72, 120, 0, 0, 0, 0, 8)
    return bytes(body)


def macho(cputype):
    return struct.pack('<IIIII', 0xFEEDFACF, cputype, 0, 2, 0) + b'\0' * 32


def binary_for(target):
    if 'linux' in target:
        return static_elf(62 if 'x86_64' in target else 183)
    return macho(0x01000007 if 'x86_64' in target else 0x0100000C)


def checksum(directory, name):
    digest = hashlib.sha256((directory / name).read_bytes()).hexdigest()
    (directory / f'{name}.sha256').write_text(f'{digest} *{name}\n')
    return f'{digest} *{name}'


def tar_xz(path, target, payload):
    with tarfile.open(path, 'w:xz') as archive:
        for name, data in ((f'dotnet-crap-{target}/dotnet-crap', payload),
                           (f'dotnet-crap-{target}/LICENSE', b'MIT')):
            entry = tarfile.TarInfo(name)
            entry.size, entry.mode = len(data), 0o755
            archive.addfile(entry, io.BytesIO(data))


def release_dir(directory):
    """The complete prepare-release.py asset set, with valid archive executables."""
    lines = []
    for target in prepare.UNIX_TARGETS:
        name = f'dotnet-crap-{target}.tar.xz'
        tar_xz(directory / name, target, binary_for(target))
        lines.append(checksum(directory, name))
    name = 'dotnet-crap-x86_64-pc-windows-msvc.zip'
    with zipfile.ZipFile(directory / name, 'w') as archive:
        archive.writestr('dotnet-crap.exe', b'MZ windows')
        archive.writestr('LICENSE', b'MIT')
    lines.append(checksum(directory, name))
    (directory / f'dotnet-crap.{VERSION}.nupkg').write_bytes(b'nupkg')
    lines.append(checksum(directory, f'dotnet-crap.{VERSION}.nupkg'))
    (directory / 'sha256.sum').write_text('\n'.join(lines) + '\n')
    for name in ('dotnet-crap-installer.sh', 'dotnet-crap.rb', 'dist-manifest.json'):
        (directory / name).write_text(name)


class FetchGh:
    """A recording fake for gh api, gh release download and gh attestation verify."""

    def __init__(self, source, *, commit=SHA, tag=TAG, draft=False, immutable=True, reject=None, tamper=None):
        self.source = source
        self.commit = commit
        self.tag = tag
        self.draft = draft
        self.immutable = immutable
        self.reject = reject
        self.tamper = tamper
        self.calls = []
        self.downloads = 0

    def __call__(self, arguments):
        self.calls.append(list(arguments))
        if arguments[0] == 'api':
            endpoint = next(a for a in arguments if a.startswith('repos/'))
            if '/commits/' in endpoint:
                return self.commit + '\n'
            if '/releases/tags/' in endpoint:
                return json.dumps({
                    'id': 77,
                    'tag_name': self.tag,
                    'draft': self.draft,
                    'immutable': self.immutable,
                    'published_at': '2026-11-12T00:00:00Z',
                })
            raise AssertionError(f'unexpected api endpoint: {endpoint}')
        if arguments[0:2] == ['release', 'download']:
            directory = Path(arguments[arguments.index('--dir') + 1])
            directory.mkdir(parents=True, exist_ok=True)
            for entry in self.source.iterdir():
                shutil.copyfile(entry, directory / entry.name)
            self.downloads += 1
            if self.tamper:
                self.tamper(directory)
            return ''
        if arguments[0:2] == ['attestation', 'verify']:
            if self.reject and self.reject(arguments):
                raise RuntimeError('attestation rejected by the fake verifier')
            return ''
        raise AssertionError(f'unexpected gh call: {arguments}')


class PublishedTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.fixture_temporary = tempfile.TemporaryDirectory()
        cls.fixture_assets = Path(cls.fixture_temporary.name) / 'assets'
        cls.fixture_assets.mkdir()
        release_dir(cls.fixture_assets)

    @classmethod
    def tearDownClass(cls):
        cls.fixture_temporary.cleanup()

    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.assets = self.root / 'assets'
        shutil.copytree(self.fixture_assets, self.assets)
        self.out = self.root / 'out'

    def tearDown(self):
        self.temporary.cleanup()

    def fetch(self, **kwargs):
        gh = FetchGh(self.assets, **kwargs)
        evidence = script.fetch(VERSION, SHA, self.out, runner=gh)
        return evidence, gh


class FetchTests(PublishedTest):
    def test_fetches_every_asset_with_hashes(self):
        evidence, gh = self.fetch()
        names = prepare.asset_names(VERSION)
        self.assertEqual(sorted(p.name for p in self.out.iterdir()), sorted(names))
        self.assertEqual(sorted(evidence['assets']), sorted(names))
        for name in names:
            self.assertEqual(evidence['assets'][name],
                             hashlib.sha256((self.out / name).read_bytes()).hexdigest())
        self.assertEqual(evidence['version'], VERSION)
        self.assertEqual(evidence['source_sha'], SHA)
        self.assertEqual(evidence['tag'], TAG)
        downloads = [call for call in gh.calls if call[0:2] == ['release', 'download']]
        self.assertEqual(len(downloads), 1)
        self.assertIn(TAG, downloads[0])
        self.assertIn(REPOSITORY, downloads[0])
        self.assertNotIn('--pattern', downloads[0])

    def test_attestation_commands_use_the_exact_release_identity(self):
        _, gh = self.fetch()
        attestations = [call for call in gh.calls if call[0:2] == ['attestation', 'verify']]
        self.assertEqual(len(attestations), len(prepare.asset_names(VERSION)))
        for call in attestations:
            self.assertTrue(Path(call[2]).is_absolute())
            self.assertEqual(call[call.index('--repo') + 1], REPOSITORY)
            self.assertEqual(call[call.index('--cert-identity') + 1],
                             f'https://github.com/{REPOSITORY}/.github/workflows/release.yml@refs/tags/{TAG}')
            self.assertEqual(call[call.index('--source-ref') + 1], f'refs/tags/{TAG}')
            self.assertEqual(call[call.index('--source-digest') + 1], SHA)
            self.assertIn('--deny-self-hosted-runners', call)

    def test_attestation_rejection_leaves_no_output(self):
        def reject(call):
            return Path(call[2]).name == f'dotnet-crap-{prepare.UNIX_TARGETS[0]}.tar.xz'

        gh = FetchGh(self.assets, reject=reject)
        with self.assertRaisesRegex(RuntimeError, 'attestation rejected'):
            script.fetch(VERSION, SHA, self.out, runner=gh)
        self.assertFalse(self.out.exists())
        self.assertTrue(any(call[0:2] == ['attestation', 'verify'] for call in gh.calls))

    def test_tag_must_resolve_to_the_expected_commit(self):
        gh = FetchGh(self.assets, commit=OTHER_SHA)
        with self.assertRaisesRegex(ValueError, 'resolves to'):
            script.fetch(VERSION, SHA, self.out, runner=gh)
        self.assertFalse(self.out.exists())
        self.assertFalse(any(call[0:2] == ['release', 'download'] for call in gh.calls))

    def test_draft_release_is_rejected_before_download(self):
        gh = FetchGh(self.assets, draft=True)
        with self.assertRaisesRegex(ValueError, 'draft'):
            script.fetch(VERSION, SHA, self.out, runner=gh)
        self.assertFalse(self.out.exists())
        self.assertFalse(any(call[0:2] == ['release', 'download'] for call in gh.calls))

    def test_mutable_release_is_rejected_before_download(self):
        gh = FetchGh(self.assets, immutable=False)
        with self.assertRaisesRegex(ValueError, 'not immutable'):
            script.fetch(VERSION, SHA, self.out, runner=gh)
        self.assertFalse(self.out.exists())
        self.assertFalse(any(call[0:2] == ['release', 'download'] for call in gh.calls))

    def test_tampered_checksum_payload_is_rejected(self):
        name = 'dotnet-crap-x86_64-apple-darwin.tar.xz'

        def tamper(directory):
            (directory / f'{name}.sha256').write_text('0' * 64 + f' *{name}\n')

        with self.assertRaisesRegex(ValueError, 'checksum mismatch'):
            script.fetch(VERSION, SHA, self.out, runner=FetchGh(self.assets, tamper=tamper))
        self.assertFalse(self.out.exists())

    def test_unsafe_checksum_paths_are_rejected(self):
        name = 'dotnet-crap-x86_64-apple-darwin.tar.xz'
        sidecar = self.assets / f'{name}.sha256'
        original = sidecar.read_text()
        for listed in ('../escape', f'../{name}', f'/tmp/{name}', f'..\\{name}', 'C:escape'):
            with self.subTest(listed=listed):
                sidecar.write_text('0' * 64 + f' *{listed}\n')
                with self.assertRaisesRegex(ValueError, 'unsafe checksum filename'):
                    script.verify_assets(self.assets, VERSION)
        sidecar.write_text(original)

    def test_checksum_for_another_file_is_rejected(self):
        name = 'dotnet-crap-x86_64-apple-darwin.tar.xz'
        sidecar = self.assets / f'{name}.sha256'
        digest = sidecar.read_text().split()[0]
        sidecar.write_text(f'{digest} *other.tar.xz\n')
        with self.assertRaisesRegex(ValueError, 'names'):
            script.verify_assets(self.assets, VERSION)

    def test_exact_asset_set_rejects_missing_and_extra_files(self):
        missing = self.assets / 'dotnet-crap.rb'
        saved = missing.read_bytes()
        missing.unlink()
        with self.assertRaisesRegex(ValueError, 'missing'):
            script.verify_assets(self.assets, VERSION)
        missing.write_bytes(saved)
        (self.assets / 'unexpected.txt').write_text('extra')
        with self.assertRaisesRegex(ValueError, 'unexpected'):
            script.verify_assets(self.assets, VERSION)

    def test_combined_checksum_list_is_validated(self):
        name = 'dotnet-crap-x86_64-apple-darwin.tar.xz'
        lines = (self.assets / 'sha256.sum').read_text().splitlines()
        lines = ['0' * 64 + f' *{name}' if line.endswith(f'*{name}') else line for line in lines]
        (self.assets / 'sha256.sum').write_text('\n'.join(lines) + '\n')
        with self.assertRaisesRegex(ValueError, 'sha256.sum disagrees'):
            script.verify_assets(self.assets, VERSION)

    def test_malformed_inputs_do_not_touch_gh_or_create_output(self):
        cases = [('1.2', SHA), ('01.2.3', SHA), ('1.2.3-01', SHA), ('1.2.3-rc.01', SHA),
                 (VERSION, SHA.upper()),
                 (VERSION, 'a' * 39), (VERSION, SHA + '\n'), (VERSION, '')]
        for version, sha in cases:
            with self.subTest(version=version, sha=sha):
                gh = FetchGh(self.assets)
                with self.assertRaises(ValueError):
                    script.fetch(version, sha, self.out, runner=gh)
                self.assertEqual(gh.calls, [])
                self.assertFalse(self.out.exists())

    def test_existing_output_is_refused_before_gh(self):
        self.out.mkdir()
        gh = FetchGh(self.assets)
        with self.assertRaises(FileExistsError):
            script.fetch(VERSION, SHA, self.out, runner=gh)
        self.assertEqual(gh.calls, [])


    def test_command_line_reports_errors_as_exit_status_two(self):
        stderr = io.StringIO()
        with contextlib.redirect_stderr(stderr):
            status = script.main(['fetch', '--version', '1.2', '--source-sha', SHA,
                                  '--output', str(self.out)])
        self.assertEqual(status, 2)
        self.assertIn('invalid version', stderr.getvalue())
        self.assertFalse(self.out.exists())

    def test_removed_npm_command_is_rejected(self):
        with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit) as exit:
            script.build_parser().parse_args(['npm', '--version', VERSION])
        self.assertEqual(exit.exception.code, 2)


if __name__ == '__main__':
    unittest.main()
