"""Exercise the actual upload-only publisher's immutable-release confirmation."""
import os
from pathlib import Path
import subprocess
import tempfile
import textwrap
import unittest

ROOT = Path(__file__).resolve().parents[1]


class ImmutablePublication(unittest.TestCase):
    def publish(self, immutable):
        workflow = (ROOT / '.github/workflows/release.yml').read_text()
        step = workflow.split('      - name: Publish verified assets\n', 1)[1].split('  publish-npm:', 1)[0]
        script = textwrap.dedent(step.split('        run: |\n', 1)[1])
        mock = '''gh() {
          case "$1 $2" in
            'api repos/RuiCostafrg/dotnet-crap/commits/v0.1.0') printf '%s' "$GITHUB_SHA" ;;
            'api --paginate') return 0 ;;
            'api repos/RuiCostafrg/dotnet-crap/releases/tags/v0.1.0') printf '%s' "$IMMUTABLE" ;;
            'release create'|'release edit') return 0 ;;
            *) echo "unexpected API or command: $*" >&2; return 97 ;;
          esac
        }
        '''
        with tempfile.TemporaryDirectory() as directory:
            assets = Path(directory) / 'release-assets'
            assets.mkdir()
            (assets / 'verified-payload').write_bytes(b'already tested bytes')
            return subprocess.run(['bash', '-euo', 'pipefail', '-c', mock + script], cwd=directory,
                                  env=dict(os.environ, GITHUB_REPOSITORY='RuiCostafrg/dotnet-crap',
                                           GITHUB_SHA='a' * 40, TAG='v0.1.0', IMMUTABLE=immutable),
                                  text=True, capture_output=True, timeout=15)

    def test_immutable_release_allows_publication_job_to_finish(self):
        result = self.publish('true')
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_mutable_or_missing_metadata_blocks_npm_staging(self):
        for value in ('false', 'null', ''):
            with self.subTest(value=value):
                result = self.publish(value)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn('not immutable', result.stdout)

    def test_read_only_preflight_never_queries_admin_settings(self):
        workflow = (ROOT / '.github/workflows/release.yml').read_text()
        self.assertNotIn('/immutable-releases', workflow)


if __name__ == '__main__':
    unittest.main()
