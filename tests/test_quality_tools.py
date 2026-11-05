import concurrent.futures
import copy
import importlib.util
import json
import subprocess
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace

ROOT = Path(__file__).resolve().parents[1]


def module(name, path):
    spec = importlib.util.spec_from_file_location(name, ROOT / path)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


gate = module("mutation_gate", "scripts/mutation-gate.py")
benchmark = module("benchmark", "scripts/benchmark.py")


def write_inventory(out, names, buckets):
    out.mkdir(parents=True, exist_ok=True)
    # Match cargo-mutants 27.1.0's real format, not a made-up `id` field.
    (out / "mutants.json").write_text(json.dumps([{"name": n, "file": "src/main.rs"} for n in names]))
    for state in gate.STATES:
        (out / f"{state}.txt").write_text("".join(n + "\n" for n in buckets.get(state, [])))


class MutationTests(unittest.TestCase):
    def test_complete_real_format(self):
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp)
            names = [f"src/main.rs:{i}:1: replace > with >= in scan" for i in range(4)]
            write_inventory(out, names, {"caught": names[:2], "timeout": names[2:3], "unviable": names[3:]})
            counts, total = gate.read_inventory(out)
            self.assertEqual(total, 4)
            self.assertEqual(counts, dict(caught=2, missed=0, timeout=1, unviable=1))

    def test_incomplete_duplicate_unknown_and_overlapping_outcomes_fail(self):
        cases = [
            (["a", "b"], {"caught": ["a"]}),
            (["a", "b"], {"caught": ["a", "a"], "missed": ["b"]}),
            (["a", "b"], {"caught": ["a", "b", "other"]}),
            (["a", "b"], {"caught": ["a", "b"], "timeout": ["b"]}),
            (["a", "a"], {"caught": ["a"]}),
            ([], {}),
        ]
        for names, buckets in cases:
            with self.subTest(names=names, buckets=buckets), tempfile.TemporaryDirectory() as tmp:
                out = Path(tmp)
                write_inventory(out, names, buckets)
                with self.assertRaises(ValueError):
                    gate.read_inventory(out)

    def test_threshold_is_strict_and_timeouts_are_not_kills(self):
        for counts in [dict(caught=9, missed=1, timeout=0, unviable=0),
                       dict(caught=9, missed=0, timeout=1, unviable=100),
                       dict(caught=0, missed=0, timeout=0, unviable=4)]:
            with self.subTest(counts=counts), self.assertRaises(ValueError):
                gate.check_score(counts)
        self.assertAlmostEqual(gate.check_score(dict(caught=10, missed=1, timeout=0, unviable=100)), 1000 / 11)

    def test_concurrent_invocations_read_only_their_own_evidence(self):
        with tempfile.TemporaryDirectory() as tmp:
            def runner(command, **kwargs):
                parent = Path(command[command.index("--output") + 1])
                name = parent.name
                write_inventory(parent / "mutants.out", [name], {"caught": [name]})
                return SimpleNamespace(returncode=0)
            def run(_):
                return gate.run_gate(runner=runner, evidence_root=Path(tmp))
            with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
                outputs = list(pool.map(run, range(2)))
            self.assertNotEqual(outputs[0], outputs[1])
            for out in outputs:
                self.assertEqual(gate.read_inventory(out), (dict(caught=1, missed=0, timeout=0, unviable=0), 1))

    def test_failed_runner_never_consumes_stale_evidence(self):
        with tempfile.TemporaryDirectory() as tmp:
            with self.assertRaisesRegex(ValueError, "runner failed"):
                gate.run_gate(runner=lambda *a, **k: SimpleNamespace(returncode=1), evidence_root=Path(tmp))


class BenchmarkTests(unittest.TestCase):
    @staticmethod
    def report(covered):
        return {"files_scanned": 1, "failed": False, "functions": [
            {"file": "C000000.cs", "name": "F", "complexity": 3,
             "coverage": 0.5 if covered else None, "crap": 4.125 if covered else 12.0},
            {"file": "C000000.cs", "name": "G", "complexity": 2,
             "coverage": 1.0 if covered else None, "crap": 2.0 if covered else 6.0},
        ]}

    def test_generated_corpus_has_independently_calculated_scores(self):
        for covered in (False, True):
            benchmark.validate_report(self.report(covered), 1, covered)

    def test_duplicate_identity_cannot_hide_a_missing_function(self):
        report = self.report(False)
        report["functions"][1] = copy.deepcopy(report["functions"][0])
        with self.assertRaises(ValueError):
            benchmark.validate_report(report, 1, False)

    def test_bad_complexity_coverage_crap_or_gate_is_rejected(self):
        for field, value in [("complexity", 2), ("coverage", 1.0), ("crap", 3.0)]:
            report = self.report(True)
            report["functions"][0][field] = value
            with self.subTest(field=field), self.assertRaises(ValueError):
                benchmark.validate_report(report, 1, True)
        report = self.report(False)
        report["failed"] = True
        with self.assertRaises(ValueError):
            benchmark.validate_report(report, 1, False)

    def test_output_replacement_and_binary_alias_protection(self):
        with tempfile.TemporaryDirectory() as tmp:
            binary = Path(tmp) / "binary"
            binary.write_bytes(b"binary contents")
            output = Path(tmp) / "report.json"
            output.write_text("old")
            benchmark.write_output(output, binary.resolve(), "new")
            self.assertEqual(output.read_text(), "new")
            with self.assertRaises(ValueError):
                benchmark.write_output(binary, binary.resolve(), "bad")
            self.assertEqual(binary.read_bytes(), b"binary contents")
            self.assertEqual(sorted(p.name for p in Path(tmp).iterdir()), ["binary", "report.json"])

    @unittest.skipIf(__import__('os').name == 'nt', 'symlink/hardlink creation may require Windows privileges')
    def test_output_symlink_and_hardlink_to_binary_are_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            binary = Path(tmp) / "binary"
            binary.write_text("native")
            link = Path(tmp) / "link.json"
            link.symlink_to(binary)
            with self.assertRaises(ValueError):
                benchmark.write_output(link, binary.resolve(), "bad")
            link.unlink()
            if hasattr(__import__('os'), 'link'):
                __import__('os').link(binary, link)
            else:
                subprocess.run(['ln', str(binary), str(link)], check=True)
            with self.assertRaises(ValueError):
                benchmark.write_output(link, binary.resolve(), "bad")
            self.assertEqual(binary.read_text(), "native")

    @unittest.skipIf(__import__('os').name == 'nt', 'Unix executable fixture')
    def test_hung_process_obeys_timeout(self):
        with tempfile.TemporaryDirectory() as tmp:
            # Tooling-only fake executable, never described as real C# evidence.
            binary = Path(tmp) / "hang"
            binary.write_text('#!/bin/sh\nexec sleep 30\n')
            binary.chmod(0o700)
            output = subprocess.run([__import__('sys').executable, str(ROOT / 'scripts/benchmark.py'),
                                     '--binary', str(binary), '--files', '1', '--runs', '3', '--timeout', '0.05'],
                                    capture_output=True, timeout=5)
            self.assertNotEqual(output.returncode, 0)
            self.assertIn(b'timed out', output.stderr)


if __name__ == "__main__":
    unittest.main()
