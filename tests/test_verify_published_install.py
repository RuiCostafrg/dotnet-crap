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
                return ''
            return 'dotnet-crap 0.1.0' if command[-1] == '--version' else ''

        with patch.object(install, 'run', side_effect=fake) as run:
            install.tool(self.args, work)
        self.assertEqual(len(run.call_args_list), 7)


if __name__ == '__main__':
    unittest.main()
