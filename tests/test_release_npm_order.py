"""Exercise release.yml's actual npm ordering gate without registry credentials."""
import os
from pathlib import Path
import shlex
import subprocess
import textwrap
import unittest


ROOT = Path(__file__).resolve().parents[1]
PACKAGES = ('dotnet-crap-linux-x64', 'dotnet-crap-linux-arm64',
            'dotnet-crap-darwin-x64', 'dotnet-crap-darwin-arm64', 'dotnet-crap')


class NpmReleaseOrdering(unittest.TestCase):
    def gate(self, version, releases, published=(), latest='0.0.0', missing=None):
        workflow = (ROOT / '.github/workflows/release.yml').read_text()
        step = workflow.split('      - name: Stage platform packages, then the main package, with provenance\n')[1]
        script = textwrap.dedent(step.split('        run: |\n', 1)[1]).split('summary=', 1)[0]
        cases = [f"'dotnet-crap dist-tags.latest') printf '%s' {shlex.quote(latest)} ;;"]
        for old in published:
            for name in PACKAGES:
                if (name, old) != missing:
                    cases.append(f"{shlex.quote(name + '@' + old + ' version')}) printf '%s' {shlex.quote(old)} ;;")
        mock = ('npm() { case "$2 $3" in\n' + '\n'.join(cases)
                + '\n*) echo "npm error code E404" >&2; return 1 ;;\nesac; }\n'
                + 'gh() { [[ "$3" == "repos/RuiCostafrg/dotnet-crap/tags?per_page=100" ]] || return 70; '
                + '[[ -n "${GH_TOKEN:-}" ]] || return 4; printf "%s\\n" "$RELEASE_TAGS"; }\n')
        return subprocess.run(['bash', '-euo', 'pipefail', '-c', mock + script + '\necho "tag=$dist_tag"'],
                              env=dict(os.environ, TAG='v' + version, RELEASE_TAGS='\n'.join(releases),
                                       GITHUB_REPOSITORY='RuiCostafrg/dotnet-crap', GH_TOKEN='test-only'),
                              text=True, capture_output=True, timeout=15)

    def test_first_release_needs_no_previous_approval(self):
        result = self.gate('0.1.0', ['v0.1.0'])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('tag=latest', result.stdout)

    def test_newer_release_waits_for_older_stages(self):
        result = self.gate('0.2.0', ['v0.2.0', 'v0.1.0'])
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('approve all npm stages for 0.1.0', result.stdout)

    def test_all_five_older_packages_must_be_public(self):
        for package in PACKAGES:
            with self.subTest(package=package):
                result = self.gate('0.2.0', ['v0.1.0', 'v0.2.0'], ['0.1.0'],
                                   missing=(package, '0.1.0'))
                self.assertNotEqual(result.returncode, 0)

    def test_approved_older_release_unblocks_newer(self):
        result = self.gate('0.2.0', ['v0.2.0', 'v0.1.0'], ['0.1.0'], latest='0.1.0')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('tag=latest', result.stdout)

    def test_older_release_can_stage_before_newer_is_public(self):
        result = self.gate('0.1.0', ['v0.2.0', 'v0.1.0'])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('tag=backport', result.stdout)

    def test_tag_waiting_for_github_approval_blocks_newer_npm_stage(self):
        # The API mock accepts /tags only: a /releases query would omit this
        # earlier version while its protected GitHub publish job is waiting.
        result = self.gate('0.2.0', ['v0.2.0', 'v0.1.0'])
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('approve all npm stages for 0.1.0', result.stdout)

    def test_staging_step_explicitly_supplies_github_authentication(self):
        workflow = (ROOT / '.github/workflows/release.yml').read_text()
        step = workflow.split('      - name: Stage platform packages, then the main package, with provenance\n')[1]
        self.assertIn('GH_TOKEN: ${{ github.token }}', step.split('        run: |', 1)[0])

    def test_backport_does_not_move_latest_back(self):
        result = self.gate('0.1.0', ['v0.2.0', 'v0.1.0'], latest='0.2.0')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('tag=backport', result.stdout)

    def test_prerelease_uses_next_without_blocking_on_stable(self):
        result = self.gate('0.2.0-rc.1', ['v0.1.0'])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('tag=next', result.stdout)


if __name__ == '__main__':
    unittest.main()
