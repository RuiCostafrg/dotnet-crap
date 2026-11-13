"""Offline fail-closed contracts for the standalone NuGet publisher."""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch
import zipfile

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location('nuget', ROOT / 'scripts/verify-nuget-release.py')
N = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(N)
VERSION = '0.2.0'
SHA = 'a' * 40
PACKAGE = f'dotnet-crap.{VERSION}.nupkg'
ENV = dict(VERSION=VERSION, SOURCE_SHA=SHA, GITHUB_SHA=SHA,
           GITHUB_REF=f'refs/tags/v{VERSION}', GITHUB_REPOSITORY=N.REPOSITORY)


def release():
    return dict(immutable=True, draft=False, prerelease=False, tag_name=f'v{VERSION}',
                assets=[dict(name=PACKAGE), dict(name=PACKAGE + '.sha256')])


def package(directory, version=VERSION, identifier='dotnet-crap', kind='DotnetTool'):
    path = directory / PACKAGE
    with zipfile.ZipFile(path, 'w') as archive:
        archive.writestr('dotnet-crap.nuspec', f'''<package xmlns="urn:test"><metadata>
          <id>{identifier}</id><version>{version}</version>
          <packageTypes><packageType name="{kind}" /></packageTypes>
          </metadata></package>''')
    path.with_name(path.name + '.sha256').write_text(
        f'{hashlib.sha256(path.read_bytes()).hexdigest()}  {path.name}\n')
    return path


class NugetPublisher(unittest.TestCase):
    def test_identity_rejects_legacy_release_and_untrusted_dispatch(self):
        N.identity(VERSION, SHA, ENV)
        for changes in (dict(GITHUB_REF='refs/heads/main'), dict(GITHUB_SHA='b' * 40),
                        dict(GITHUB_REPOSITORY='fork/dotnet-crap')):
            with self.subTest(changes=changes), self.assertRaises(ValueError):
                N.identity(VERSION, SHA, dict(ENV, **changes))
        for version in ('0.1.0', '0.2.0-rc.1', '01.2.0', '../0.2.0', '0.2.0\n'):
            with self.subTest(version=version), self.assertRaises(ValueError):
                N.identity(version, SHA, ENV)
        with self.assertRaises(ValueError):
            N.identity(VERSION, 'short', ENV)

    def test_mutable_draft_prerelease_and_missing_or_duplicate_asset_rejected(self):
        for key, value in (('immutable', False), ('immutable', None), ('draft', True),
                           ('prerelease', True), ('tag_name', 'v0.3.0'), ('assets', []),
                           ('assets', release()['assets'] * 2)):
            with self.subTest(key=key), self.assertRaises(ValueError):
                N.release_metadata(dict(release(), **{key: value}), VERSION)

    def test_checksums_and_internal_identity(self):
        with tempfile.TemporaryDirectory() as directory:
            directory = Path(directory)
            path = package(directory)
            N.package_metadata(path, VERSION)
            path.write_bytes(path.read_bytes() + b'changed bytes')
            with self.assertRaisesRegex(ValueError, 'SHA256'):
                N.package_metadata(path, VERSION)
            for args in (dict(version='0.3.0'), dict(identifier='other'), dict(kind='Library')):
                with self.subTest(args=args), self.assertRaisesRegex(ValueError, 'ID, version'):
                    N.package_metadata(package(directory, **args), VERSION)

    def test_all_remote_and_provenance_failures_stop_verification(self):
        # Run the actual orchestration, with a deterministic data-only remote.
        for failure in (None, 'checkout', 'ancestry', 'tag', 'cargo', 'immutable',
                        'release-signature', 'asset-signature', 'provenance'):
            with self.subTest(failure=failure), tempfile.TemporaryDirectory() as directory:
                destination = Path(directory) / 'verified'
                calls = []

                def remote(*args):
                    calls.append(args)
                    if args[:2] == ('git', 'rev-parse'):
                        return 'b' * 40 if failure == 'checkout' else SHA
                    if args[:2] == ('git', 'merge-base'):
                        if failure == 'ancestry':
                            raise subprocess.CalledProcessError(1, args)
                        return ''
                    if args[:2] == ('git', 'show'):
                        return '[package]\nversion="' + ('0.3.0' if failure == 'cargo' else VERSION) + '"'
                    if args[:2] == ('gh', 'api'):
                        if '/commits/' in args[2]:
                            return 'b' * 40 if failure == 'tag' else SHA
                        return json.dumps(dict(release(), immutable=failure != 'immutable'))
                    if args[:3] == ('gh', 'release', 'download'):
                        package(destination)
                    rejects = {'release-signature': ('gh', 'release', 'verify'),
                               'asset-signature': ('gh', 'release', 'verify-asset'),
                               'provenance': ('gh', 'attestation', 'verify')}
                    if failure in rejects and args[:3] == rejects[failure]:
                        raise subprocess.CalledProcessError(1, args)
                    return ''

                with patch.object(N, 'command', side_effect=remote):
                    if failure:
                        with self.assertRaises((ValueError, subprocess.CalledProcessError)):
                            N.verify(ENV, destination)
                    else:
                        N.verify(ENV, destination)
                        attestations = [c for c in calls if c[:3] == ('gh', 'attestation', 'verify')]
                        self.assertEqual(len(attestations), 2)
                        for call in attestations:
                            self.assertIn('--source-digest', call)
                            self.assertEqual(call[call.index('--source-digest') + 1], SHA)
                            self.assertIn('--source-ref', call)
                            self.assertIn('--cert-identity', call)
                            self.assertIn('--deny-self-hosted-runners', call)
                        with self.assertRaises(FileExistsError):
                            N.verify(ENV, destination)

    def test_workflow_gates_and_duplicate_push_failure(self):
        workflow = (ROOT / '.github/workflows/publish-nuget.yml').read_text()
        for contract in ("github.actor == 'RuiCostafrg'", "github.triggering_actor == 'RuiCostafrg'",
                         "github.ref_type == 'tag'", "vars.DOTNET_CRAP_NUGET_PUBLISH == 'true'",
                         'environment: release', 'persist-credentials: false'):
            self.assertIn(contract, workflow)
        self.assertLess(workflow.index('run: python3 scripts/verify-nuget-release.py'),
                        workflow.index('uses: NuGet/login@'))
        script = workflow.split('        run: |\n', 1)[1]
        # The real push step must propagate a registry conflict, without skip flags.
        with tempfile.TemporaryDirectory() as directory:
            destination = Path(directory) / 'verified-nuget'
            destination.mkdir()
            package(destination)
            mock = 'dotnet() { [[ "$*" != *--skip-duplicate* ]] || exit 98; return 23; }; export -f dotnet\n'
            result = subprocess.run(['bash', '-euo', 'pipefail', '-c', mock + script],
                                    cwd=directory, env=dict(os.environ, VERSION=VERSION, NUGET_API_KEY='fake'),
                                    capture_output=True, text=True)
            self.assertEqual(result.returncode, 23, result.stderr)


if __name__ == '__main__':
    unittest.main()
