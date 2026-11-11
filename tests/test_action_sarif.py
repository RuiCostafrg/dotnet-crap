"""Unit tests for action/sarif.py, the action's SARIF post-processor."""
import copy
import importlib.util
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / 'action' / 'sarif.py'
spec = importlib.util.spec_from_file_location('action_sarif', SCRIPT)
sarif = importlib.util.module_from_spec(spec)
spec.loader.exec_module(sarif)


def report(uris=('Gauge.First.cs',), base='file:///tmp/scan/src/'):
    results = [{
        'ruleId': 'DNCRAP001',
        'message': {'text': 'Choose has CRAP 6.000 (threshold 5)'},
        'locations': [{'physicalLocation': {
            'artifactLocation': {'uri': uri, 'uriBaseId': '%SRCROOT%'},
            'region': {'startLine': 3, 'endLine': 9}}}],
    } for uri in uris]
    results.append({'ruleId': 'DNCRAP003', 'message': {'text': 'No implemented C# callables found'}})
    return {'version': '2.1.0', 'runs': [{
        'tool': {'driver': {'name': 'dotnet-crap'}},
        'originalUriBaseIds': {'%SRCROOT%': {'uri': base}},
        'results': results}]}


class RootPrefix(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.workspace = Path(self.temporary.name).resolve()
        (self.workspace / 'a b' / 'src').mkdir(parents=True)
        self.outside = tempfile.TemporaryDirectory()

    def tearDown(self):
        self.temporary.cleanup()
        self.outside.cleanup()

    def test_workspace_itself_has_no_prefix(self):
        self.assertEqual(sarif.root_prefix(self.workspace, self.workspace), '')
        self.assertEqual(sarif.root_prefix(os.curdir, os.curdir), '')

    def test_subdirectory_prefix_uses_forward_slashes(self):
        self.assertEqual(sarif.root_prefix(self.workspace / 'a b' / 'src', self.workspace), 'a b/src')

    def test_outside_scan_root_is_none(self):
        self.assertIsNone(sarif.root_prefix(self.outside.name, self.workspace))
        self.assertIsNone(sarif.root_prefix(self.workspace.parent, self.workspace))

    def test_sibling_with_shared_name_prefix_is_outside(self):
        sibling = Path(str(self.workspace) + '-sibling')
        sibling.mkdir()
        try:
            self.assertIsNone(sarif.root_prefix(sibling, self.workspace))
        finally:
            sibling.rmdir()

    def test_symlink_out_of_workspace_is_resolved(self):
        link = self.workspace / 'link'
        try:
            link.symlink_to(self.outside.name)
        except OSError:
            self.skipTest('symlinks unavailable')
        self.assertIsNone(sarif.root_prefix(link, self.workspace))


class Rewrite(unittest.TestCase):
    def test_prefixes_relative_uris_and_moves_the_source_root(self):
        data = report(('Gauge.First.cs', 'sub/Other%20File.cs'))
        with tempfile.TemporaryDirectory() as workspace:
            sarif.rewrite(data, 'a b/src', workspace)
            uris = [r['locations'][0]['physicalLocation']['artifactLocation']['uri']
                    for r in data['runs'][0]['results'] if 'locations' in r]
            self.assertEqual(uris, ['a%20b/src/Gauge.First.cs', 'a%20b/src/sub/Other%20File.cs'])
            base = data['runs'][0]['originalUriBaseIds']['%SRCROOT%']['uri']
            self.assertEqual(base, Path(workspace).resolve().as_uri().rstrip('/') + '/')

    def test_absolute_and_scheme_uris_are_left_alone(self):
        data = report(('/abs/File.cs', 'file:///x/File.cs'))
        with tempfile.TemporaryDirectory() as workspace:
            sarif.rewrite(data, 'src', workspace)
        uris = [r['locations'][0]['physicalLocation']['artifactLocation']['uri']
                for r in data['runs'][0]['results'] if 'locations' in r]
        self.assertEqual(uris, ['/abs/File.cs', 'file:///x/File.cs'])

    def test_results_without_locations_and_missing_base_ids_are_tolerated(self):
        data = {'runs': [{'results': [{'message': {'text': 'm'}}]}, {}]}
        expected = copy.deepcopy(data)
        sarif.rewrite(data, 'src', '.')
        self.assertEqual(data, expected)


class Summary(unittest.TestCase):
    def test_lines_for_located_and_unlocated_results(self):
        lines = sarif.summary(report(('a%20b/Gauge.cs',)))
        self.assertEqual(lines, [
            'a b/Gauge.cs:3: Choose has CRAP 6.000 (threshold 5)',
            'No implemented C# callables found',
            '2 SARIF result(s)'])

    def test_empty_report(self):
        self.assertEqual(sarif.summary({'runs': [{'results': []}]}), ['0 SARIF result(s)'])
        self.assertEqual(sarif.summary({}), ['0 SARIF result(s)'])

    def test_location_without_line_prints_the_path_only(self):
        data = {'runs': [{'results': [{'message': {'text': 'm'}, 'locations': [
            {'physicalLocation': {'artifactLocation': {'uri': 'A.cs'}}}]}]}]}
        self.assertEqual(sarif.summary(data)[0], 'A.cs: m')

    def test_control_characters_cannot_forge_workflow_commands(self):
        data = {'runs': [{'results': [{
            'message': {'text': 'bad\n::error::forged\x1b[31m'},
            'locations': [{'physicalLocation': {
                'artifactLocation': {'uri': 'x%0A::notice::y.cs'}, 'region': {'startLine': 1}}}]}]}]}
        lines = sarif.summary(data)
        self.assertEqual(len(lines), 2)
        self.assertNotIn('\n', lines[0])
        self.assertNotIn('\x1b', lines[0])
        self.assertIn('\\n::error::forged\\x1b[31m', lines[0])
        self.assertTrue(lines[0].startswith('x\\n::notice::y.cs:1: '))

    def test_printable_keeps_spaces_and_unicode(self):
        self.assertEqual(sarif.printable('Größe café.cs'), 'Größe café.cs')


class Command(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.workspace = Path(self.temporary.name).resolve()
        (self.workspace / 'fixtures' / 'lib').mkdir(parents=True)
        self.report = self.workspace / 'report.sarif'

    def tearDown(self):
        self.temporary.cleanup()

    def run_script(self, scan_root, workspace=None):
        return subprocess.run(
            [sys.executable, '-I', '-X', 'utf8', str(SCRIPT), str(self.report), str(scan_root),
             str(workspace or self.workspace)],
            capture_output=True, text=True)

    def write(self, data):
        self.report.write_text(json.dumps(data), encoding='utf-8')

    def test_subdirectory_scan_rewrites_in_place_and_exits_zero(self):
        self.write(report())
        result = self.run_script(self.workspace / 'fixtures' / 'lib')
        self.assertEqual(result.returncode, 0, result.stderr)
        data = json.loads(self.report.read_text(encoding='utf-8'))
        uri = data['runs'][0]['results'][0]['locations'][0]['physicalLocation']['artifactLocation']['uri']
        self.assertEqual(uri, 'fixtures/lib/Gauge.First.cs')
        self.assertEqual(data['runs'][0]['tool']['driver']['name'], 'dotnet-crap')
        self.assertEqual(data['version'], '2.1.0')
        self.assertIn('2 SARIF result(s)', result.stdout)

    def test_workspace_root_scan_leaves_the_file_untouched(self):
        self.write(report())
        before = self.report.read_bytes()
        result = self.run_script(self.workspace)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.report.read_bytes(), before)

    def test_outside_scan_root_exits_three_and_leaves_the_file_untouched(self):
        self.write(report())
        before = self.report.read_bytes()
        with tempfile.TemporaryDirectory() as outside:
            result = self.run_script(outside)
        self.assertEqual(result.returncode, 3)
        self.assertEqual(self.report.read_bytes(), before)
        self.assertIn('2 SARIF result(s)', result.stdout)

    def test_invalid_json_fails(self):
        self.report.write_text('{not json', encoding='utf-8')
        self.assertNotIn(self.run_script(self.workspace).returncode, (0, 3))


if __name__ == '__main__':
    unittest.main()
