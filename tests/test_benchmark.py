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
    """Load a tooling script without invoking its command-line entry point."""
    spec = importlib.util.spec_from_file_location(name, ROOT / path)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


benchmark = module("benchmark", "scripts/benchmark.py")

class BenchmarkTests(unittest.TestCase):
    @staticmethod
    def report(covered):
        """Return independent expected scores for the two generated C# callables."""
        return {"files_scanned": 1, "failed": False, "functions": [
            {"file": "C000000.cs", "name": "F", "complexity": 3,
             "coverage": 0.5 if covered else None, "crap": 4.125 if covered else 12.0},
            {"file": "C000000.cs", "name": "G", "complexity": 2,
             "coverage": 1.0 if covered else None, "crap": 2.0 if covered else 6.0},
        ]}

    def test_generated_corpus_has_independently_calculated_scores(self):
        """Accept both uncovered and partially covered hand-calculated fixture reports."""
        for covered in (False, True):
            benchmark.validate_report(self.report(covered), 1, covered)

    def test_duplicate_identity_cannot_hide_a_missing_function(self):
        """Reject duplicated F records even when the reported callable count remains correct."""
        report = self.report(False)
        report["functions"][1] = copy.deepcopy(report["functions"][0])
        with self.assertRaises(ValueError):
            benchmark.validate_report(report, 1, False)

    def test_bad_complexity_coverage_crap_or_gate_is_rejected(self):
        """Reject independent corruptions of each score input and the final gate result."""
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
        """Replace an ordinary report atomically while preserving the measured executable."""
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
        """Reject filesystem aliases that could overwrite the measured executable."""
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
        """Verify a deliberately stalled tooling-only executable exits under the timeout."""
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

    @unittest.skipIf(__import__('os').name == 'nt', 'Unix executable fixture')
    def test_measured_run_reports_child_rss_and_rejects_failures(self):
        """Measure only the reaped child, keep its stdout, and surface nonzero exits."""
        elapsed, rss, stdout = benchmark.run_measured(["/bin/sh", "-c", "printf ok"], 10)
        self.assertGreater(elapsed, 0)
        self.assertGreater(rss, 0)
        self.assertEqual(stdout, b"ok")
        with self.assertRaises(subprocess.CalledProcessError):
            benchmark.run_measured(["/bin/sh", "-c", "exit 3"], 10)
        with self.assertRaises(subprocess.TimeoutExpired):
            benchmark.run_measured(["/bin/sh", "-c", "exec sleep 30"], 0.05)

    def test_rss_units_are_normalized_to_kibibytes(self):
        """Linux reports ru_maxrss in KiB and macOS in bytes."""
        expected = 4 if __import__('sys').platform == "darwin" else 4096
        self.assertEqual(benchmark.max_rss_kib(4096), expected)

    def test_cli_keeps_historical_flags_and_rejects_conflicts(self):
        """--files/--coverage keep their meaning; inconsistent selections are usage errors."""
        _, args = benchmark.parse_args(["--files", "3", "--coverage"])
        self.assertEqual((args.workload, args.scale, args.mode), ("small-files", 3, "scan-coverage"))
        _, args = benchmark.parse_args([])
        self.assertEqual((args.scale, args.mode, args.runs, args.warmup), (500, "scan", 7, 1))
        for argv in (["--workload", "dense-file", "--files", "2"], ["--files", "2", "--scale", "3"],
                     ["--coverage", "--mode", "collect"], ["--runs", "2"], ["--scale", "0"],
                     ["--warmup", "-1"], ["--max-median-ms", "nan"], ["--timeout", "0"]):
            with self.subTest(argv=argv), self.assertRaises(SystemExit), \
                    __import__('contextlib').redirect_stderr(__import__('io').StringIO()):
                benchmark.parse_args(argv)

    def test_collected_evidence_must_equal_generated_lines(self):
        """A dropped or altered collected line is rejected."""
        workload = SimpleNamespace(native_coverage={"version": 1, "files": {"A.cs": {"sha256": "0" * 64, "lines": {"1": 1}}}})
        benchmark.validate_collected(copy.deepcopy(workload.native_coverage), workload)
        altered = copy.deepcopy(workload.native_coverage)
        altered["files"]["A.cs"]["lines"]["1"] = 0
        with self.assertRaises(ValueError):
            benchmark.validate_collected(altered, workload)

    def test_span_point_visits_counts_points_inside_each_full_span(self):
        """Nested spans are counted again for every enclosing callable (hand-counted: 3 + 2 + 0)."""
        workload = SimpleNamespace(
            expected=[{"file": "A.cs", "line": 1, "end_line": 4}, {"file": "A.cs", "line": 2, "end_line": 3},
                      {"file": "B.cs", "line": 1, "end_line": 9}],
            native_coverage={"files": {"A.cs": {"lines": {"2": 1, "3": 0, "4": 1, "10": 1}}}})
        self.assertEqual(benchmark.span_point_visits(workload), 5)

    @unittest.skipUnless(__import__('os').environ.get("DOTNET_CRAP_BENCH_BINARY"), "set DOTNET_CRAP_BENCH_BINARY")
    def test_baseline_comparison_end_to_end(self):
        """Measure a binary against itself for every mode and record both sample sets."""
        binary = __import__('os').environ["DOTNET_CRAP_BENCH_BINARY"]
        for mode in benchmark.MODES:
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as tmp:
                output = Path(tmp) / "m.json"
                with __import__('contextlib').redirect_stdout(__import__('io').StringIO()):
                    code = benchmark.main(["--binary", binary, "--baseline", binary, "--workload", "solution",
                                           "--scale", "2", "--mode", mode, "--runs", "4", "--output", str(output)])
                self.assertEqual(code, 0)
                result = json.loads(output.read_text())
                self.assertEqual(len(result["samples_ms"]), 4)
                self.assertEqual(len(result["baseline"]["peak_rss_kib"]), 4)
                self.assertGreater(result["rss_floor_kib"], 0)
                self.assertTrue(result["baseline"]["outputs_byte_identical"])
                self.assertEqual(result["provenance"]["binary_sha256"], result["baseline"]["binary_sha256"])

    @unittest.skipIf(__import__('os').name == 'nt', 'Unix process fixture')
    def test_rss_is_not_floored_by_the_harness_footprint(self):
        """A large harness heap must not leak into the measured program's ru_maxrss."""
        ballast = bytearray(256 * 1024 * 1024)
        for offset in range(0, len(ballast), 4096):
            ballast[offset] = 1
        _, rss, _ = benchmark.run_measured([str(Path(__import__('shutil').which("true")).resolve())], 10)
        self.assertLess(rss, 128 * 1024)
        self.assertLessEqual(benchmark.rss_floor_kib(), 128 * 1024)
        del ballast

    @unittest.skipIf(__import__('os').name == 'nt', 'Unix process fixture')
    def test_timeout_kills_the_whole_process_group(self):
        """Grandchildren (for example the collector's replay shell) do not outlive a timeout."""
        with tempfile.TemporaryDirectory() as tmp:
            pidfile = Path(tmp) / "pid"
            with self.assertRaises(subprocess.TimeoutExpired):
                benchmark.run_measured(["/bin/sh", "-c", f'sleep 30 & echo $! > "{pidfile}"; wait'], 0.5)
            pid = int(pidfile.read_text())
            for _ in range(100):
                try:
                    __import__('os').kill(pid, 0)
                except ProcessLookupError:
                    break
                __import__('time').sleep(0.02)
            else:
                self.fail("grandchild survived the timeout")

    def test_baseline_requires_even_runs(self):
        """Balanced ABBA order needs an even number of measured runs."""
        with self.assertRaises(SystemExit), __import__('contextlib').redirect_stderr(__import__('io').StringIO()):
            benchmark.parse_args(["--baseline", "x", "--runs", "7"])
        _, args = benchmark.parse_args(["--baseline", "x", "--runs", "8"])
        self.assertEqual(args.runs, 8)


if __name__ == "__main__":
    unittest.main()
