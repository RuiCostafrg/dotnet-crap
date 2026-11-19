"""Published installs must match authenticated payloads and retain isolated feeds."""
import hashlib
import importlib.util
import io
from pathlib import Path
import tarfile
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch
import zipfile

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location('published_install', ROOT / 'scripts/verify-published-install.py')
install = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(install)
TARGET = 'x86_64-unknown-linux-musl'


class PublishedInstall(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.assets = self.root / 'assets'
        self.assets.mkdir()
        self.args = SimpleNamespace(assets=self.assets, version='0.1.0', source_sha='a' * 40,
                                    target=TARGET, rid='linux-x64', fixture=ROOT / 'examples/Fixture')

    def archive(self, data=b'authenticated executable', target=TARGET):
        name = f'dotnet-crap-{target}'
        path = self.assets / (name + ('.zip' if 'windows' in target else '.tar.xz'))
        if 'windows' in target:
            with zipfile.ZipFile(path, 'w') as source:
                source.writestr('dotnet-crap.exe', data)
        else:
            with tarfile.open(path, 'w:xz') as source:
                entry = tarfile.TarInfo(f'{name}/dotnet-crap')
                entry.size = len(data)
                source.addfile(entry, io.BytesIO(data))
        path.with_name(path.name + '.sha256').write_text(f'{hashlib.sha256(path.read_bytes()).hexdigest()}  {path.name}\n')
        return data

    def test_altered_installed_binary_is_not_executed(self):
        self.archive()
        binary = self.root / 'dotnet-crap'
        binary.write_bytes(b'same version but different payload')
        with patch.object(install, 'run') as run:
            with self.assertRaisesRegex(ValueError, 'bytes differ'):
                install.check_binary(binary, self.assets, [TARGET], '0.1.0', self.args.fixture)
            run.assert_not_called()

    def test_smoke_checks_pass_and_fail_gates_without_github_credentials(self):
        binary = self.root / 'dotnet-crap'
        binary.write_bytes(self.archive())
        with patch.dict('os.environ', GH_TOKEN='must-not-reach-program', GITHUB_TOKEN='nor-this'):
            with patch.object(install, 'run', side_effect=['dotnet-crap 0.1.0', '', '']) as run:
                install.check_binary(binary, self.assets, [TARGET], '0.1.0', self.args.fixture)
        self.assertEqual(len(run.call_args_list), 3)
        self.assertEqual(run.call_args_list[-1].kwargs['expected'], 1)
        for call in run.call_args_list:
            self.assertNotIn('GH_TOKEN', call.kwargs['env'])
            self.assertNotIn('GITHUB_TOKEN', call.kwargs['env'])

    def test_checksum_tampering_rejected_before_execution(self):
        binary = self.root / 'dotnet-crap'
        binary.write_bytes(self.archive())
        path = self.assets / f'dotnet-crap-{TARGET}.tar.xz'
        path.write_bytes(path.read_bytes() + b'changed')
        with patch.object(install, 'run') as run:
            with self.assertRaisesRegex(ValueError, 'Checksum mismatch'):
                install.check_binary(binary, self.assets, [TARGET], '0.1.0', self.args.fixture)
            run.assert_not_called()

    def test_windows_zip_layout_is_not_assumed_to_match_unix(self):
        target = 'x86_64-pc-windows-msvc'
        expected = self.archive(target=target)
        self.assertEqual(install.archive_binary(self.assets, target), expected)

    def test_live_formula_mismatch_prevents_brew_install(self):
        tap = self.root / 'tap'
        (tap / 'Formula').mkdir(parents=True)
        (tap / 'Formula/dotnet-crap.rb').write_text('unexpected live formula')
        (self.assets / 'dotnet-crap.rb').write_text('attested expected formula')
        with patch.object(install, 'run', side_effect=['', str(tap)]) as run:
            with self.assertRaisesRegex(ValueError, 'live tap formula differs'):
                install.homebrew(self.args)
        self.assertEqual(len(run.call_args_list), 2)

    def test_tool_uses_exact_release_package_and_isolated_feed(self):
        package = b'exact attested nupkg bytes'
        (self.assets / 'dotnet-crap.0.1.0.nupkg').write_bytes(package)
        binary = self.archive()
        work = self.root / 'work'
        work.mkdir()
        original_run = install.run

        def fake(command, **kwargs):
            if command[:3] == ['dotnet', 'tool', 'install']:
                self.assertEqual((work / 'feed/dotnet-crap.0.1.0.nupkg').read_bytes(), package)
                self.assertIn('<clear />', (work / 'NuGet.Config').read_text())
                self.assertEqual(command[command.index('--add-source') + 1], str(work / 'feed'))
                self.assertEqual(kwargs['env']['NUGET_PACKAGES'], str(work / 'cache'))
                self.assertNotIn('GH_TOKEN', kwargs['env'])
                native = work / 'tool/.store/dotnet-crap/0.1.0/tools/net10.0/any/native/linux-x64/dotnet-crap'
                native.parent.mkdir(parents=True)
                native.write_bytes(binary)
                native.chmod(0o600)
                shim = work / 'tool/dotnet-crap'
                shim.write_text('#!/bin/sh\n'
                                'if [ "$1" = --version ]; then echo "dotnet-crap 0.1.0"; exit 0; fi\n'
                                'while [ "$#" -gt 0 ]; do\n'
                                '  if [ "$1" = --threshold ] && [ "$2" = 0 ]; then exit 1; fi\n'
                                '  shift\n'
                                'done\n')
                shim.chmod(0o755)
                return ''
            return original_run(command, **kwargs)

        with patch.object(install, 'run', side_effect=fake) as run:
            install.tool(self.args, work)
        self.assertEqual(len(run.call_args_list), 4)


    def package(self, path, entries):
        with zipfile.ZipFile(path, 'w') as archive:
            for name, data in entries.items():
                archive.writestr(name, data)
        return path

    def test_nuget_accepts_repository_signature_only_on_the_published_package(self):
        release = self.package(self.assets / 'dotnet-crap.0.1.0.nupkg',
                               {'dotnet-crap.nuspec': b'<package />', 'tools/native/dotnet-crap': b'payload'})
        published = self.package(self.root / 'published.nupkg',
                                 {'dotnet-crap.nuspec': b'<package />', 'tools/native/dotnet-crap': b'payload',
                                  '.signature.p7s': b'repository signature'})
        install.compare_packages(release, published)

    def test_nuget_signed_release_asset_is_rejected(self):
        signed = self.package(self.assets / 'dotnet-crap.0.1.0.nupkg',
                              {'dotnet-crap.nuspec': b'<package />', '.signature.p7s': b'signature'})
        published = self.package(self.root / 'published.nupkg',
                                 {'dotnet-crap.nuspec': b'<package />', '.signature.p7s': b'signature'})
        with self.assertRaisesRegex(ValueError, r'unexpected \.signature\.p7s'):
            install.compare_packages(signed, published)

    def test_nuget_changed_entry_fails(self):
        release = self.package(self.assets / 'dotnet-crap.0.1.0.nupkg',
                               {'dotnet-crap.nuspec': b'<package />', 'tools/native/dotnet-crap': b'payload'})
        published = self.package(self.root / 'published.nupkg',
                                 {'dotnet-crap.nuspec': b'<package />', 'tools/native/dotnet-crap': b'changed'})
        with self.assertRaisesRegex(ValueError, 'bytes differ'):
            install.compare_packages(release, published)

    def test_nuget_extra_or_missing_entry_fails(self):
        entries = {'dotnet-crap.nuspec': b'<package />'}
        for change, contents in (('extra', dict(entries, **{'lib/extra.dll': b'extra'})),
                                 ('missing', {})):
            with self.subTest(change=change):
                release = self.package(self.assets / 'dotnet-crap.0.1.0.nupkg', entries)
                published = self.package(self.root / f'{change}.nupkg', contents)
                with self.assertRaisesRegex(ValueError, 'ZIP entries differ'):
                    install.compare_packages(release, published)

    def test_nuget_installs_only_from_nuget_org_and_runs_tool_checks(self):
        binary = self.archive()
        self.package(self.assets / 'dotnet-crap.0.1.0.nupkg',
                     {'dotnet-crap.nuspec': b'<package />', 'tools/native/dotnet-crap': binary})
        published = self.package(self.root / 'published.nupkg',
                                 {'dotnet-crap.nuspec': b'<package />', 'tools/native/dotnet-crap': binary,
                                  '.signature.p7s': b'repository signature'})
        work = self.root / 'work'
        work.mkdir()
        original_run = install.run

        def fake_download(url, destination, **kwargs):
            self.assertTrue(url.startswith('https://api.nuget.org/v3-flatcontainer/dotnet-crap/0.1.0/'))
            self.assertTrue(url.endswith('/dotnet-crap.0.1.0.nupkg'))
            destination.write_bytes(published.read_bytes())
            return destination

        def fake(command, **kwargs):
            if command[:3] == ['dotnet', 'tool', 'install']:
                config = (work / 'NuGet.Config').read_text()
                self.assertIn('<clear />', config)
                self.assertEqual(config.count('<add '), 1)
                self.assertIn('https://api.nuget.org/v3/index.json', config)
                self.assertNotIn('--add-source', command)
                self.assertNotIn(str(work / 'feed'), config)
                self.assertEqual(kwargs['env']['NUGET_PACKAGES'], str(work / 'cache'))
                self.restore(work / 'tool', published)
                native = work / 'tool/.store/dotnet-crap/0.1.0/tools/net10.0/any/native/linux-x64/dotnet-crap'
                native.parent.mkdir(parents=True)
                native.write_bytes(binary)
                shim = work / 'tool/dotnet-crap'
                shim.write_text('#!/bin/sh\n'
                                'if [ "$1" = --version ]; then echo "dotnet-crap 0.1.0"; exit 0; fi\n'
                                'while [ "$#" -gt 0 ]; do\n'
                                '  if [ "$1" = --threshold ] && [ "$2" = 0 ]; then exit 1; fi\n'
                                '  shift\n'
                                'done\n')
                shim.chmod(0o755)
                return ''
            return original_run(command, **kwargs)

        with patch.object(install, 'download', side_effect=fake_download), \
                patch.object(install, 'run', side_effect=fake) as run:
            install.nuget(self.args, work)
        self.assertEqual(len(run.call_args_list), 4)

    def restore(self, destination, package):
        """Lay out a package the way `dotnet tool install --tool-path` stores it."""
        store = destination / '.store/dotnet-crap/0.1.0/dotnet-crap/0.1.0'
        store.mkdir(parents=True, exist_ok=True)
        (store / 'dotnet-crap.0.1.0.nupkg').write_bytes(Path(package).read_bytes())
        with zipfile.ZipFile(package) as archive:
            for name in archive.namelist():
                if name.startswith('tools/'):
                    (store / name).parent.mkdir(parents=True, exist_ok=True)
                    (store / name).write_bytes(archive.read(name))
        return store

    def test_restored_package_and_extracted_files_must_match_the_release(self):
        entries = {'dotnet-crap.nuspec': b'<package />', 'tools/net10.0/any/launcher.dll': b'launcher'}
        release = self.package(self.assets / 'dotnet-crap.0.1.0.nupkg', entries)
        signed = self.package(self.root / 'signed.nupkg', dict(entries, **{'.signature.p7s': b'sig'}))
        store = self.restore(self.root / 'tool', signed)
        install.check_restored(self.args, self.root / 'tool')
        (store / 'tools/net10.0/any/launcher.dll').write_bytes(b'swapped after the download check')
        with self.assertRaisesRegex(ValueError, 'extracted tools/net10.0/any/launcher.dll differs'):
            install.check_restored(self.args, self.root / 'tool')
        other = self.package(self.root / 'other.nupkg', {'dotnet-crap.nuspec': b'<other />',
                                                         'tools/net10.0/any/launcher.dll': b'launcher'})
        self.restore(self.root / 'tool2', other)
        with self.assertRaisesRegex(ValueError, 'bytes differ'):
            install.check_restored(self.args, self.root / 'tool2')
        with self.assertRaisesRegex(ValueError, 'restored package not found'):
            install.check_restored(self.args, self.root / 'missing')
        self.assertTrue(release.is_file())

    def test_oversized_published_package_is_rejected_before_decompression(self):
        release = self.package(self.assets / 'dotnet-crap.0.1.0.nupkg', {'dotnet-crap.nuspec': b'<package />'})
        with zipfile.ZipFile(self.root / 'bomb.nupkg', 'w', zipfile.ZIP_DEFLATED) as archive:
            archive.writestr('dotnet-crap.nuspec', b'<package />')
            archive.writestr('.signature.p7s', b'\0' * (install.NUGET_SIGNATURE_LIMIT + 1))
        with self.assertRaisesRegex(ValueError, 'uncompressed size exceeds'):
            install.compare_packages(release, self.root / 'bomb.nupkg')

    def test_download_retries_truncated_responses(self):
        import http.client
        calls = []

        class Response(io.BytesIO):
            def __enter__(self):
                return self

            def __exit__(self, *exc):
                return False

        def opener(url, timeout):
            calls.append(url)
            if len(calls) == 1:
                raise http.client.IncompleteRead(b'partial', 10)
            return Response(b'complete')

        with patch.object(install.urllib.request, 'urlopen', side_effect=opener), \
                patch.object(install.time, 'sleep') as sleep:
            path = install.download('https://example.invalid/p.nupkg', self.root / 'p.nupkg', attempts=3, delay=0)
        self.assertEqual(path.read_bytes(), b'complete')
        self.assertEqual(len(calls), 2)
        sleep.assert_called_once()

    def test_registry_install_retries_while_the_version_propagates(self):
        import subprocess
        attempts = []

        def fake(command, **kwargs):
            if command[:3] == ['dotnet', 'tool', 'install']:
                attempts.append(command)
                if len(attempts) == 1:
                    raise subprocess.CalledProcessError(1, command)
                raise RuntimeError('stop after the retried install')
            return ''

        with patch.object(install, 'run', side_effect=fake), patch.object(install.time, 'sleep') as sleep:
            with self.assertRaisesRegex(RuntimeError, 'stop after the retried install'):
                install.tool(self.args, self.root, source=install.NUGET_ORG)
        self.assertEqual(len(attempts), 2)
        sleep.assert_called_once_with(install.NUGET_RETRY_SECONDS)


if __name__ == '__main__':
    unittest.main()
