"""Offline tests for scripts/verify-published.py.

Every gh call and every registry request is faked. The release fixture is built
with the real prepare-release/npm-packages conventions and valid ELF/Mach-O
headers, so verify-npm-packages.py runs unchanged.
"""
import base64
import contextlib
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
import struct
import tarfile
import tempfile
import unittest
from unittest import mock
import zipfile

ROOT = Path(__file__).resolve().parents[1]


def module(name, relative):
    spec = importlib.util.spec_from_file_location(name, ROOT / relative)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


script = module('verify_published', 'scripts/verify-published.py')
prepare = script.prepare
npm = script.npm

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


def package_map(tarballs):
    return {name: (npm.tarball_name(name, VERSION), tarballs[npm.tarball_name(name, VERSION)])
            for name in script.package_names()}


class FetchGh:
    """A recording fake for gh api, gh release download and gh attestation verify."""

    def __init__(self, source, *, commit=SHA, tag=TAG, draft=False, reject=None, tamper=None):
        self.source = source
        self.commit = commit
        self.tag = tag
        self.draft = draft
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


class NpmGh:
    """A recording fake for the run/artifact API, gh run download and attestation verification."""

    def __init__(self, tarballs, *, runs, artifacts, reject=None, tamper=None):
        self.tarballs = tarballs
        self.runs = runs
        self.artifacts = artifacts
        self.reject = reject
        self.tamper = tamper
        self.calls = []
        self.downloads = 0

    def __call__(self, arguments):
        self.calls.append(list(arguments))
        if arguments[0] == 'api':
            endpoint = next(a for a in arguments if a.startswith('repos/'))
            if '/actions/workflows/release.yml/runs' in endpoint:
                return json.dumps(self.runs)
            if '/actions/runs/' in endpoint and '/artifacts' in endpoint:
                return json.dumps(self.artifacts)
            raise AssertionError(f'unexpected api endpoint: {endpoint}')
        if arguments[0:2] == ['run', 'download']:
            directory = Path(arguments[arguments.index('--dir') + 1])
            directory.mkdir(parents=True, exist_ok=True)
            for name, data in self.tarballs.items():
                (directory / name).write_bytes(data)
            self.downloads += 1
            if self.tamper:
                self.tamper(directory)
            return ''
        if arguments[0:2] == ['attestation', 'verify']:
            if self.reject and self.reject(arguments):
                raise RuntimeError('attestation rejected by the fake verifier')
            return ''
        raise AssertionError(f'unexpected gh call: {arguments}')


class FakeRegistry:
    """Injected network: serves npm metadata and tarballs without touching the network."""

    def __init__(self, packages, *, integrity=None, tarball_url=None, tarball_bytes=None,
                 metadata=None):
        self.packages = packages
        self.calls = []
        self.integrity = {name: 'sha512-' + base64.b64encode(
            hashlib.sha512(data).digest()).decode() for name, (_, data) in packages.items()}
        self.urls = {name: f'https://registry.npmjs.org/{name}/-/{filename}'
                     for name, (filename, _) in packages.items()}
        self.bytes = {name: data for name, (_, data) in packages.items()}
        self.metadata = {}
        for name, (filename, _) in packages.items():
            self.metadata[name] = {'name': name, 'version': VERSION}
        self.integrity.update(integrity or {})
        self.urls.update(tarball_url or {})
        self.bytes.update(tarball_bytes or {})
        for key, value in (metadata or {}).items():
            self.metadata[key].update(value)
        self.metadata_urls = {f'https://registry.npmjs.org/{name}/{VERSION}': name
                              for name in packages}
        self.tarball_urls = {url: name for name, url in self.urls.items()}

    def __call__(self, url, *, max_bytes):
        self.calls.append(url)
        if url in self.metadata_urls:
            name = self.metadata_urls[url]
            fields = dict(self.metadata[name])
            fields['dist'] = {'integrity': self.integrity[name], 'tarball': self.urls[name]}
            return url, json.dumps(fields).encode()
        if url in self.tarball_urls:
            name = self.tarball_urls[url]
            return url, self.bytes[name]
        raise AssertionError(f'unexpected registry request: {url}')


def run_payload(**overrides):
    value = {
        'id': 123,
        'head_sha': SHA,
        'head_branch': TAG,
        'event': 'push',
        'status': 'completed',
        'conclusion': 'success',
        'path': '.github/workflows/release.yml',
        'run_attempt': 1,
        'run_number': 10,
    }
    value.update(overrides)
    return value


def artifact_payload(**overrides):
    value = {
        'id': 456,
        'name': 'npm-packages',
        'expired': False,
        'size_in_bytes': 1234,
        'workflow_run': {'id': 123, 'head_sha': SHA, 'head_branch': TAG},
    }
    value.update(overrides)
    return value


class PublishedTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.fixture_temporary = tempfile.TemporaryDirectory()
        cls.fixture_assets = Path(cls.fixture_temporary.name) / 'assets'
        cls.fixture_assets.mkdir()
        release_dir(cls.fixture_assets)
        cls.tarballs = npm.packages(cls.fixture_assets, VERSION)
        cls.packages = package_map(cls.tarballs)

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

    def npm_gh(self, *, runs=None, artifacts=None, **kwargs):
        return NpmGh(self.tarballs, runs=runs if runs is not None else [run_payload()],
                     artifacts=artifacts if artifacts is not None else [artifact_payload()],
                     **kwargs)

    def npm(self, gh, registry, *, verify_packages=None):
        return script.fetch_npm(VERSION, SHA, self.assets, self.out, runner=gh,
                                network=registry, verify_packages=verify_packages)


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
        cases = [('1.2', SHA), ('01.2.3', SHA), (VERSION, SHA.upper()),
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


class NpmTests(PublishedTest):
    def test_full_flow_matches_registry_without_rebuilding(self):
        gh = self.npm_gh()
        registry = FakeRegistry(self.packages)
        original_verify = script.npm_verify.verify
        with mock.patch.object(npm, 'build', side_effect=AssertionError('must not rebuild')), \
                mock.patch.object(npm, 'packages', side_effect=AssertionError('must not rebuild')), \
                mock.patch.object(script.npm_verify, 'verify', wraps=original_verify) as verifier:
            with contextlib.redirect_stdout(io.StringIO()) as stdout:
                evidence = self.npm(gh, registry)
        self.assertEqual(stdout.getvalue(), '')
        self.assertEqual(verifier.call_count, 1)
        self.assertEqual(evidence['run_id'], 123)
        self.assertEqual(evidence['run_attempt'], 1)
        self.assertEqual(evidence['artifact_id'], 456)
        self.assertEqual(evidence['artifact_name'], 'npm-packages')
        self.assertEqual(sorted(evidence['packages']), sorted(script.package_names()))
        for name, (filename, data) in self.packages.items():
            self.assertEqual((self.out / 'npm-packages' / filename).read_bytes(), data)
            self.assertEqual((self.out / 'registry' / filename).read_bytes(), data)
            package = evidence['packages'][name]
            self.assertEqual(package['sha256'], hashlib.sha256(data).hexdigest())
            self.assertEqual(package['registry_integrity'],
                             'sha512-' + base64.b64encode(hashlib.sha512(data).digest()).decode())
            self.assertTrue(package['bytes_match'])
        downloads = [call for call in gh.calls if call[0:2] == ['run', 'download']]
        self.assertEqual(len(downloads), 1)
        self.assertEqual(downloads[0][2], '123')
        self.assertEqual(downloads[0][downloads[0].index('--name') + 1], 'npm-packages')
        run_requests = [next(a for a in call if a.startswith('repos/'))
                        for call in gh.calls
                        if call[0] == 'api' and '/actions/workflows/release.yml/runs' in ' '.join(call)]
        self.assertEqual(len(run_requests), 1)
        self.assertIn(f'head_sha={SHA}', run_requests[0])
        self.assertIn('event=push', run_requests[0])
        attestations = [call for call in gh.calls if call[0:2] == ['attestation', 'verify']]
        self.assertEqual(len(attestations), 5)
        for call in attestations:
            self.assertEqual(call[call.index('--cert-identity') + 1],
                             f'https://github.com/{REPOSITORY}/.github/workflows/release.yml@refs/tags/{TAG}')
            self.assertEqual(call[call.index('--source-ref') + 1], f'refs/tags/{TAG}')
            self.assertEqual(call[call.index('--source-digest') + 1], SHA)
            self.assertIn('--deny-self-hosted-runners', call)
        self.assertEqual(len(registry.calls), 10)

    def test_local_tarball_attestation_rejection_leaves_no_output(self):
        def reject(call):
            return Path(call[2]).name == npm.tarball_name(npm.MAIN, VERSION)

        gh = self.npm_gh(reject=reject)
        registry = FakeRegistry(self.packages)
        with self.assertRaisesRegex(RuntimeError, 'attestation rejected'):
            self.npm(gh, registry)
        self.assertFalse(self.out.exists())
        self.assertEqual(registry.calls, [])

    def test_requires_exactly_one_release_run(self):
        two_runs = self.npm_gh(runs=[run_payload(id=123), run_payload(id=124, run_number=11)])
        registry = FakeRegistry(self.packages)
        with self.assertRaisesRegex(ValueError, 'exactly one successful'):
            self.npm(two_runs, registry)
        self.assertFalse(self.out.exists())
        self.assertEqual(registry.calls, [])

    def test_requires_a_successful_release_run_at_the_exact_sha_and_tag(self):
        cases = [
            [run_payload(conclusion='failure')],
            [run_payload(head_branch='main')],
            [run_payload(head_sha=OTHER_SHA)],
            [run_payload(event='workflow_dispatch')],
            [],
        ]
        for runs in cases:
            with self.subTest(runs=runs):
                gh = self.npm_gh(runs=runs)
                with self.assertRaisesRegex(ValueError, 'no successful'):
                    self.npm(gh, FakeRegistry(self.packages))
                self.assertFalse(self.out.exists())

    def test_requires_exactly_one_unexpired_artifact(self):
        cases = [
            ([], 'no npm-packages artifact'),
            ([artifact_payload(expired=True)], 'has expired'),
            ([artifact_payload(id=1), artifact_payload(id=2)], 'exactly one unexpired'),
        ]
        for artifacts, message in cases:
            with self.subTest(artifacts=artifacts):
                gh = self.npm_gh(artifacts=artifacts)
                registry = FakeRegistry(self.packages)
                with self.assertRaisesRegex(ValueError, message):
                    self.npm(gh, registry)
                self.assertFalse(self.out.exists())
                self.assertEqual(registry.calls, [])

    def test_fails_closed_instead_of_rebuilding(self):
        gh = self.npm_gh(artifacts=[artifact_payload(expired=True)])
        with mock.patch.object(npm, 'build', side_effect=AssertionError('must not rebuild')), \
                mock.patch.object(npm, 'packages', side_effect=AssertionError('must not rebuild')):
            with self.assertRaisesRegex(ValueError, 'has expired'):
                self.npm(gh, FakeRegistry(self.packages))
        self.assertFalse(self.out.exists())

    def test_registry_integrity_mismatch_is_rejected(self):
        name = 'dotnet-crap-linux-x64'
        bad = 'sha512-' + base64.b64encode(b'not the attested bytes').decode()
        registry = FakeRegistry(self.packages, integrity={name: bad})
        with self.assertRaisesRegex(ValueError, 'dist.integrity'):
            self.npm(self.npm_gh(), registry)
        self.assertFalse(self.out.exists())

    def test_registry_tarball_bytes_must_match_the_attested_hash(self):
        name = 'dotnet-crap-linux-arm64'
        original = self.packages[name][1]
        registry = FakeRegistry(self.packages, tarball_bytes={name: original + b'x'})
        with self.assertRaisesRegex(ValueError, 'does not match dist.integrity'):
            self.npm(self.npm_gh(), registry)
        self.assertFalse(self.out.exists())

    def test_registry_tarball_url_hostname_must_be_npm_registry(self):
        for url in ('https://evil.example/dotnet-crap.tgz',
                    'http://registry.npmjs.org/dotnet-crap.tgz',
                    'https://registry.npmjs.org.evil/dotnet-crap.tgz'):
            with self.subTest(url=url):
                name = 'dotnet-crap-darwin-x64'
                registry = FakeRegistry(self.packages, tarball_url={name: url})
                with self.assertRaisesRegex(ValueError, 'registry URL|registry tarball URL'):
                    self.npm(self.npm_gh(), registry)
                self.assertFalse(self.out.exists())

    def test_registry_metadata_name_and_version_are_exact(self):
        cases = [
            ({'name': 'other-package'}, 'metadata name'),
            ({'version': '9.9.9'}, 'metadata version'),
        ]
        for metadata, message in cases:
            with self.subTest(metadata=metadata):
                package = 'dotnet-crap-darwin-arm64'
                registry = FakeRegistry(self.packages, metadata={package: metadata})
                with self.assertRaisesRegex(ValueError, message):
                    self.npm(self.npm_gh(), registry)
                self.assertFalse(self.out.exists())

    def test_tampered_assets_are_rejected_before_any_network(self):
        name = 'dotnet-crap-x86_64-unknown-linux-gnu.tar.xz'
        digest = (self.assets / f'{name}.sha256').read_text().split()[0]
        (self.assets / f'{name}.sha256').write_text('0' * 64 + f' *{name}\n')
        self.assertNotEqual(digest, '0' * 64)
        gh = self.npm_gh()
        registry = FakeRegistry(self.packages)
        with self.assertRaisesRegex(ValueError, 'checksum mismatch'):
            self.npm(gh, registry)
        self.assertEqual(gh.calls, [])
        self.assertEqual(registry.calls, [])
        self.assertFalse(self.out.exists())

    def test_output_inside_assets_is_refused(self):
        self.out = self.assets / 'out'
        gh = self.npm_gh()
        with self.assertRaisesRegex(ValueError, 'output must not be inside'):
            self.npm(gh, FakeRegistry(self.packages))
        self.assertEqual(gh.calls, [])

    def test_malformed_inputs_do_not_touch_gh_or_registry(self):
        cases = [('1.2', SHA), ('01.2.3', SHA), (VERSION, SHA.upper()),
                 (VERSION, 'a' * 39), (VERSION, SHA + '\n')]
        for version, sha in cases:
            with self.subTest(version=version, sha=sha):
                gh = self.npm_gh()
                registry = FakeRegistry(self.packages)
                with self.assertRaises(ValueError):
                    script.fetch_npm(version, sha, self.assets, self.out,
                                     runner=gh, network=registry)
                self.assertEqual(gh.calls, [])
                self.assertEqual(registry.calls, [])
                self.assertFalse(self.out.exists())

    def test_command_line_reports_errors_as_exit_status_two(self):
        stderr = io.StringIO()
        with contextlib.redirect_stderr(stderr):
            status = script.main(['fetch', '--version', '1.2', '--source-sha', SHA,
                                  '--output', str(self.out)])
        self.assertEqual(status, 2)
        self.assertIn('invalid version', stderr.getvalue())
        self.assertFalse(self.out.exists())


if __name__ == '__main__':
    unittest.main()
