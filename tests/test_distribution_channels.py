"""Contracts for the retained release channels and their publication boundary."""
import hashlib
import os
from pathlib import Path
import re
import subprocess
import tempfile
import textwrap
import unittest

ROOT = Path(__file__).resolve().parents[1]
WORKFLOWS = ROOT / '.github/workflows'


def workflow(name):
    return (WORKFLOWS / name).read_text()


def run_step(source, name):
    step = source.split(f'      - name: {name}\n', 1)[1]
    step = re.split(r'^      - ', step, maxsplit=1, flags=re.M)[0]
    return textwrap.dedent(step.split('        run: |\n', 1)[1])


class DistributionChannels(unittest.TestCase):
    def test_build_digest_matches_publisher_and_rejects_payload_changes(self):
        build = run_step(workflow('build-release.yml'), 'Digest of the exact payload handed to publication')
        publish = run_step(workflow('release.yml'), 'Check the exact payload that verification tested')
        for change in ('bytes', 'rename', 'extra', 'missing'):
            with self.subTest(change=change), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                assets = root / 'release-assets'
                assets.mkdir()
                payloads = {'dotnet-crap.rb': b'formula', 'dotnet-crap.1.2.3.nupkg': b'tool',
                            'dotnet-crap-installer.sh': b'installer', 'native.tar.xz': b'native'}
                # Deliberately unsorted creation order; filenames and bytes both bind the handoff.
                for name, data in payloads.items():
                    (assets / name).write_bytes(data)
                output = root / 'output'
                result = subprocess.run(['bash', '-euo', 'pipefail', '-c', build], cwd=root,
                                        env=dict(os.environ, GITHUB_OUTPUT=str(output)), capture_output=True)
                self.assertEqual(result.returncode, 0, result.stderr)
                digest = output.read_text().strip().removeprefix('digest=')
                listing = ''.join(f'{hashlib.sha256(payloads[name]).hexdigest()}  {name}\n'
                                  for name in sorted(payloads))
                self.assertEqual(digest, hashlib.sha256(listing.encode()).hexdigest())
                def verify():
                    return subprocess.run(['bash', '-euo', 'pipefail', '-c', publish], cwd=root,
                                          env=dict(os.environ, EXPECTED=digest), capture_output=True)
                self.assertEqual(verify().returncode, 0)
                target = assets / 'dotnet-crap.rb'
                if change == 'bytes':
                    target.write_bytes(b'tampered')
                elif change == 'rename':
                    target.rename(assets / 'other.rb')
                elif change == 'extra':
                    (assets / 'unexpected').write_bytes(b'extra')
                else:
                    target.unlink()
                rejected = verify()
                self.assertNotEqual(rejected.returncode, 0)
                self.assertIn(b'artifacts differ', rejected.stdout)

    def test_remaining_channels_gate_publication(self):
        release = workflow('release.yml')
        self.assertIn('needs: [preflight, build, verify-installation, verify-action]', release)
        for name in ('installation.yml', 'github-action.yml'):
            self.assertIn(f'uses: ./.github/workflows/{name}', release)
            self.assertIn(f'uses: ./.github/workflows/{name}', workflow('distribution.yml'))
        build = workflow('build-release.yml')
        for command in ('scripts/prepare-release.py', 'scripts/stage-dotnet-tool.py',
                        'scripts/pack-dotnet-tool.sh', 'dotnet tool install', 'dist build'):
            self.assertIn(command, build)
        published = workflow('verify-published.yml')
        for job in ('shell', 'homebrew', 'action'):
            self.assertRegex(published, rf'(?m)^  {job}:$')
        self.assertIn('uses: ./.github/workflows/verify-release.yml', published)
        for rid in ('linux-x64', 'linux-arm64', 'osx-x64', 'osx-arm64', 'win-x64'):
            self.assertIn(f'rid: {rid},', workflow('verify-release.yml'))

    def test_publisher_keeps_approval_and_token_isolation(self):
        release = workflow('release.yml')
        publisher = release.split('\n  publish:\n', 1)[1]
        for required in ('environment: release', "github.event_name == 'push'",
                         "vars.DOTNET_CRAP_PUBLISH == 'true'", 'contents: write',
                         'id-token: write', 'attestations: write', 'release-assets/*'):
            self.assertIn(required, publisher)
        self.assertNotIn('uses: actions/checkout@', publisher)
        self.assertNotRegex(publisher, r'run:.*(?:python|cargo|dotnet)')
        self.assertEqual(release.count('environment: release'), 1)
        for name in ('build-release.yml', 'distribution.yml', 'release.yml', 'verify-published.yml'):
            self.assertNotRegex(workflow(name), r'(?i)npm|pnpm|yarn|\bbun\b|setup-node')

    def test_external_actions_remain_sha_pinned(self):
        for file in WORKFLOWS.glob('*.yml'):
            for ref in re.findall(r'uses:\s*([^\s#]+)', file.read_text()):
                if not ref.startswith('./'):
                    self.assertRegex(ref, r'@[0-9a-f]{40}$', f'{file.name}: {ref}')


if __name__ == '__main__':
    unittest.main()
