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
        measured = benchmark.run_measured(["/bin/sh", "-c", "printf ok"], 10)
        self.assertGreater(measured.wall_ms, 0)
        self.assertGreater(measured.rss_kib, 0)
        self.assertEqual((measured.stdout, measured.code), (b"ok", 0))
        with self.assertRaises(subprocess.CalledProcessError):
            benchmark.run_measured(["/bin/sh", "-c", "exit 3"], 10)
        failed = benchmark.run_measured(["/bin/sh", "-c", "echo bad >&2; exit 3"], 10, check=False)
        self.assertEqual((failed.code, failed.stderr), (3, b"bad\n"))
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
                candidate = result["candidates"][0]
                self.assertEqual(len(candidate["samples_ms"]), 4)
                self.assertEqual(len(candidate["cpu_samples_ms"]), 4)
                self.assertGreater(candidate["median_cpu_ms"], 0)
                self.assertEqual(len(result["baseline"]["peak_rss_kib"]), 4)
                self.assertGreater(result["rss_floor_kib"], 0)
                self.assertTrue(candidate["outputs_byte_identical"])
                self.assertEqual(result["provenance"]["binary_sha256"], result["baseline"]["binary_sha256"])

    @unittest.skipUnless(__import__('os').environ.get("DOTNET_CRAP_BENCH_BINARY"), "set DOTNET_CRAP_BENCH_BINARY")
    def test_sweep_with_a_real_candidate_option(self):
        """--fail-on-empty exists today and leaves the output unchanged, so a sweep over it must pass."""
        binary = __import__('os').environ["DOTNET_CRAP_BENCH_BINARY"]
        with tempfile.TemporaryDirectory() as tmp:
            output = Path(tmp) / "m.json"
            with __import__('contextlib').redirect_stdout(__import__('io').StringIO()):
                code = benchmark.main(["--binary", binary, "--baseline", binary, "--workload", "many-medium",
                                       "--scale", "2", "--runs", "6", "--candidate-args=",
                                       "--candidate-args=--fail-on-empty", "--output", str(output)])
            self.assertEqual(code, 0)
            result = json.loads(output.read_text())
            self.assertEqual([c["args"] for c in result["candidates"]], [[], ["--fail-on-empty"]])

    @unittest.skipIf(__import__('os').name == 'nt', 'Unix process fixture')
    def test_rss_is_not_floored_by_the_harness_footprint(self):
        """A large harness heap must not leak into the measured program's ru_maxrss."""
        ballast = bytearray(256 * 1024 * 1024)
        for offset in range(0, len(ballast), 4096):
            ballast[offset] = 1
        rss = benchmark.run_measured([str(Path(__import__('shutil').which("true")).resolve())], 10).rss_kib
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


def fake_binary(directory, report=None, body=""):
    """A tooling-only stand-in executable: logs its path and arguments, then prints ``report``."""
    log = Path(directory) / "calls.log"
    script = Path(directory) / "fake"
    if report is not None:
        (Path(directory) / "report.json").write_text(json.dumps(report))
    script.write_text(f"""#!/bin/sh
echo "$0 $*" >> '{log}'
{body}
""")
    script.chmod(0o700)
    return script, log


def small_files_report():
    """The exact scanner report for the generated small-files corpus of one file (no coverage)."""
    with tempfile.TemporaryDirectory() as tmp:
        workload = bench_workloads.generate("small-files", Path(tmp) / "r", 1)
    functions = []
    for want in workload.expected:
        got = dict(want)
        got["coverage"] = None
        got["crap"] = float(want["complexity"] ** 2 + want["complexity"])
        functions.append(got)
    return {"threshold": 1e12, "files_scanned": 1, "coverage_complete": False,
            "functions": functions, "failed": False}


bench_workloads = module("bench_workloads_for_benchmark_tests", "scripts/bench_workloads.py")


class SeparationTests(unittest.TestCase):
    def test_separated_only_without_any_overlap(self):
        """Every candidate sample must beat every baseline sample, strictly."""
        self.assertEqual(benchmark.separation([10, 11, 12], [7, 8, 9]), "lower")
        self.assertEqual(benchmark.separation([10, 11, 12], [13, 14, 15]), "higher")
        self.assertIsNone(benchmark.separation([10, 11, 12], [9, 13, 8]))
        # Touching ranges are not separated, on either side.
        self.assertIsNone(benchmark.separation([10, 11, 12], [8, 9, 10]))
        self.assertIsNone(benchmark.separation([10, 11, 12], [12, 13, 14]))

    def test_compare_reports_ratio_and_separation_for_wall_and_cpu(self):
        baseline = {"wall_ms": [10.0, 12.0, 14.0], "cpu_ms": [20.0, 22.0, 24.0]}
        candidate = {"wall_ms": [5.0, 6.0, 7.0], "cpu_ms": [21.0, 23.0, 25.0]}
        result = benchmark.compare(baseline, candidate)
        self.assertEqual(result["wall_ms"], {"median_ratio": 0.5, "separated": "lower"})
        self.assertEqual(result["cpu_ms"]["median_ratio"], 23.0 / 22.0)
        self.assertIsNone(result["cpu_ms"]["separated"])

    def test_summary_carries_cpu_next_to_wall_and_rss(self):
        summary = benchmark.summarize({"wall_ms": [3.0, 1.0, 2.0], "cpu_ms": [9.0, 7.0, 8.0], "rss_kib": [5, 6, 7]})
        self.assertEqual((summary["median_ms"], summary["min_ms"], summary["max_ms"]), (2.0, 1.0, 3.0))
        self.assertEqual((summary["median_cpu_ms"], summary["min_cpu_ms"], summary["max_cpu_ms"]), (8.0, 7.0, 9.0))
        self.assertEqual((summary["median_peak_rss_kib"], summary["max_peak_rss_kib"]), (6, 7))
        self.assertEqual(summary["cpu_samples_ms"], [9.0, 7.0, 8.0])


@unittest.skipIf(__import__('os').name == 'nt', 'Unix process fixture')
class CpuTimeTests(unittest.TestCase):
    def test_cpu_time_is_user_plus_system_of_the_child(self):
        """A busy child costs CPU about equal to its wall time; a sleeping one costs almost none."""
        busy = benchmark.run_measured([__import__('sys').executable, "-I", "-S", "-c",
                                       "import time\nend = time.perf_counter() + 0.3\nwhile time.perf_counter() < end: pass"], 30)
        self.assertGreater(busy.cpu_ms, 150)
        self.assertLess(busy.cpu_ms, busy.wall_ms * 1.5 + 50)
        idle = benchmark.run_measured(["/bin/sleep", "0.3"], 30)
        self.assertLess(idle.cpu_ms, 100)
        self.assertGreater(idle.wall_ms, 250)

    def test_cpu_time_includes_waited_for_descendants(self):
        """The collector's replay shell is a child of the CLI; its CPU is part of the sample."""
        script = "import time\nend = time.perf_counter() + 0.3\nwhile time.perf_counter() < end: pass"
        command = ["/bin/sh", "-c", f'{__import__("shlex").quote(__import__("sys").executable)} -I -S -c "{script}"; true']
        self.assertGreater(benchmark.run_measured(command, 30).cpu_ms, 150)


class CandidateArgsTests(unittest.TestCase):
    def parse(self, *argv):
        with __import__('contextlib').redirect_stderr(__import__('io').StringIO()):
            return benchmark.parse_args(list(argv))[1]

    def test_default_is_one_set_without_extra_arguments(self):
        self.assertEqual(self.parse().candidate_args, [[]])

    def test_sets_are_split_like_shell_words_and_kept_in_order(self):
        args = self.parse("--baseline", "x", "--runs", "8", "--candidate-args=--jobs 1",
                          "--candidate-args=--jobs=2 --fail-on-empty", "--candidate-args=")
        self.assertEqual(args.candidate_args, [["--jobs", "1"], ["--jobs=2", "--fail-on-empty"], []])
        self.assertEqual(self.parse("--candidate-args=--name 'a b'").candidate_args, [["--name", "a b"]])

    def test_invalid_sets_are_usage_errors(self):
        for argv in (
            ["--baseline", "x", "--runs", "6", "--candidate-args=--jobs 1", "--candidate-args=--jobs 1"],
            ["--baseline", "x", "--runs", "6", "--candidate-args=", "--candidate-args= "],
            ["--candidate-args=--jobs 1", "--candidate-args=--jobs 2"],  # a sweep needs a baseline
            ["--candidate-args=--root /x"], ["--candidate-args=--format=sarif"],
            ["--candidate-args=--threshold 1"], ["--candidate-args=--output o"],
            ["--candidate-args=--coverage c"], ["--candidate-args=--require-coverage"],
            ["--candidate-args=--expected-reports 2"], ["--candidate-args=-- sh"],
            ["--candidate-args=--name 'unterminated"],
        ):
            with self.subTest(argv=argv), self.assertRaises(SystemExit):
                self.parse(*argv)

    def test_a_flag_that_only_resembles_a_reserved_one_is_allowed(self):
        self.assertEqual(self.parse("--candidate-args=--rootless").candidate_args, [["--rootless"]])

    def test_runs_must_be_a_multiple_of_the_executables_measured(self):
        """Baseline plus N sets rotate through N + 1 positions."""
        base = ["--baseline", "x", "--candidate-args=--jobs 1", "--candidate-args=--jobs 2"]
        for runs in ("4", "7", "8"):
            with self.subTest(runs=runs), self.assertRaises(SystemExit):
                self.parse(*base, "--runs", runs)
        self.assertEqual(self.parse(*base, "--runs", "9").runs, 9)

    def test_set_labels(self):
        self.assertEqual(benchmark.set_label([]), "(no extra arguments)")
        self.assertEqual(benchmark.set_label(["--jobs", "4"]), "--jobs 4")


class RepoOptionTests(unittest.TestCase):
    def parse(self, *argv):
        with __import__('contextlib').redirect_stderr(__import__('io').StringIO()):
            return benchmark.parse_args(list(argv))[1]

    def test_repo_requires_a_baseline_a_directory_and_scan_mode(self):
        with tempfile.TemporaryDirectory() as tmp:
            args = self.parse("--repo", tmp, "--baseline", "x", "--runs", "4")
            self.assertEqual((args.mode, args.repo_name), ("scan", None))
            for argv in (["--repo", tmp], ["--repo", str(Path(tmp) / "missing"), "--baseline", "x"],
                         ["--repo", tmp, "--baseline", "x", "--mode", "collect"],
                         ["--repo", tmp, "--baseline", "x", "--coverage"],
                         ["--repo", tmp, "--baseline", "x", "--workload", "skew"],
                         ["--repo", tmp, "--baseline", "x", "--scale", "3"],
                         ["--repo-name", "name"]):
                with self.subTest(argv=argv), self.assertRaises(SystemExit):
                    self.parse(*argv)

    def test_selection_mirrors_the_scanner_rules(self):
        """Count what src/selection.rs would select: skip bin, obj, .git, links and generated files."""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            keep = {"A.cs": b"12345", "sub/B.cs": b"123", "Upper.CS": b"12", "dir.cs/inner.cs": b"1"}
            skip = ["bin/C.cs", "obj/D.cs", ".git/E.cs", "sub/bin/F.cs", "G.g.cs", "H.generated.cs",
                    "I.Designer.cs", "j.DESIGNER.cs", "notes.txt", "K.csx", "cs"]
            for rel, data in {**keep, **{name: b"x" * 99 for name in skip}}.items():
                path = root / rel
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(data)
            (root / "linked.cs").symlink_to(root / "A.cs")
            (root / "linkdir").symlink_to(root / "sub", target_is_directory=True)
            self.assertEqual(benchmark.select_sources(root), (4, sum(len(v) for v in keep.values())))

    def test_failure_description(self):
        failed = benchmark.Measured(1.0, 1.0, 1, 2, b"", b"error: invalid C# syntax in A.cs\nmore\n")
        self.assertEqual(benchmark.describe_failure(failed), {
            "exit_code": 2, "message": "error: invalid C# syntax in A.cs", "syntax_failure": True})
        other = benchmark.Measured(1.0, 1.0, 1, 2, b"", b"error: source is not UTF-8")
        self.assertFalse(benchmark.describe_failure(other)["syntax_failure"])
        silent = benchmark.Measured(1.0, 1.0, 1, 2, b"", b"")
        self.assertEqual(benchmark.describe_failure(silent)["message"], "")


@unittest.skipIf(__import__('os').name == 'nt', 'Unix executable fixture')
class FakeBinaryTests(unittest.TestCase):
    """Drive main() with tooling-only fake executables, so no real scanner is needed."""

    def run_main(self, *argv):
        with tempfile.TemporaryDirectory() as out, __import__('contextlib').redirect_stdout(__import__('io').StringIO()):
            target = Path(out) / "m.json"
            code = benchmark.main([*argv, "--output", str(target)])
            return code, json.loads(target.read_text())

    def test_extra_arguments_reach_only_the_candidate_and_identity_is_enforced(self):
        with tempfile.TemporaryDirectory() as tmp:
            fake, log = fake_binary(tmp, small_files_report(), f"cat '{tmp}/report.json'")
            code, result = self.run_main("--binary", str(fake), "--baseline", str(fake), "--files", "1",
                                         "--warmup", "0", "--runs", "6", "--candidate-args=--jobs 1",
                                         "--candidate-args=--jobs 2")
            self.assertEqual(code, 0)
            calls = log.read_text().splitlines()
            self.assertEqual(len(calls), 18)
            baseline = [c for c in calls if "/baseline/" in c]
            candidate = [c for c in calls if "/candidate/" in c]
            self.assertEqual(len(baseline), 6)
            self.assertFalse([c for c in baseline if "--jobs" in c])
            self.assertEqual(sorted(c.split()[-2:] [0] + c.split()[-1] for c in candidate), ["--jobs1"] * 6 + ["--jobs2"] * 6)
            self.assertEqual([c["args"] for c in result["candidates"]], [["--jobs", "1"], ["--jobs", "2"]])
            self.assertEqual(result["baseline"]["binary_sha256"], result["provenance"]["binary_sha256"])
            for entry in result["candidates"]:
                self.assertTrue(entry["outputs_byte_identical"])
                self.assertEqual(len(entry["cpu_samples_ms"]), 6)
                self.assertIn("separated", entry["comparison"]["cpu_ms"])
            self.assertGreaterEqual(result["provenance"]["usable_cpus"], 1)
            self.assertGreaterEqual(result["provenance"]["cpu_count"], result["provenance"]["usable_cpus"])

    def test_every_position_is_used_equally_often(self):
        """The rotation gives each executable each place in the run order the same number of times."""
        with tempfile.TemporaryDirectory() as tmp:
            fake, log = fake_binary(tmp, small_files_report(), f"cat '{tmp}/report.json'")
            self.run_main("--binary", str(fake), "--baseline", str(fake), "--files", "1", "--warmup", "0",
                          "--runs", "6", "--candidate-args=--jobs 1", "--candidate-args=--jobs 2")
            names = [("base" if "/baseline/" in c else c.split()[-1]) for c in log.read_text().splitlines()]
            rounds = [names[i:i + 3] for i in range(0, 18, 3)]
            for place in range(3):
                counts = {name: sum(1 for r in rounds if r[place] == name) for name in ("base", "1", "2")}
                self.assertEqual(counts, {"base": 2, "1": 2, "2": 2}, place)

    def test_a_set_that_changes_the_output_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            body = f"cat '{tmp}/report.json'\ncase \"$*\" in *--changes-output*) echo ' ' ;; esac"
            fake, _ = fake_binary(tmp, small_files_report(), body)
            with self.assertRaisesRegex(ValueError, "not byte-identical"), tempfile.TemporaryDirectory() as out:
                benchmark.main(["--binary", str(fake), "--baseline", str(fake), "--files", "1", "--warmup", "0",
                                "--runs", "6", "--candidate-args=--jobs 1", "--candidate-args=--changes-output",
                                "--output", str(Path(out) / "m.json")])

    def test_single_set_keeps_alternating_order_and_needs_no_baseline(self):
        with tempfile.TemporaryDirectory() as tmp:
            fake, log = fake_binary(tmp, small_files_report(), f"cat '{tmp}/report.json'")
            code, result = self.run_main("--binary", str(fake), "--files", "1", "--warmup", "0", "--runs", "3",
                                         "--candidate-args=--jobs 4")
            self.assertEqual(code, 0)
            self.assertNotIn("baseline", result)
            self.assertNotIn("comparison", result["candidates"][0])
            self.assertEqual(len(log.read_text().splitlines()), 3)
            code, _ = self.run_main("--binary", str(fake), "--baseline", str(fake), "--files", "1",
                                    "--warmup", "1", "--runs", "4")
            lines = log.read_text().splitlines()[3:]
            # One warmup round (baseline first), then four measured rounds starting with the candidate.
            self.assertEqual(["/candidate/" in c for c in lines],
                             [False, True, True, False, False, True, True, False, False, True])

    def test_collect_mode_puts_candidate_arguments_before_the_command_separator(self):
        with tempfile.TemporaryDirectory() as tmp:
            # Not a valid collect run: the fake only records argv and fails, which is enough to read the order.
            fake, log = fake_binary(tmp, None, "exit 9")
            with self.assertRaises(subprocess.CalledProcessError), tempfile.TemporaryDirectory() as out:
                benchmark.main(["--binary", str(fake), "--files", "1", "--mode", "collect", "--warmup", "0",
                                "--runs", "3", "--candidate-args=--jobs 4", "--output", str(Path(out) / "m.json")])
            argv = log.read_text().splitlines()[0].split()
            self.assertEqual(argv[argv.index("--jobs") + 1], "4")
            self.assertLess(argv.index("--jobs"), argv.index("--"))

    def test_max_median_applies_to_every_set(self):
        with tempfile.TemporaryDirectory() as tmp:
            fake, _ = fake_binary(tmp, small_files_report(), f"cat '{tmp}/report.json'")
            common = ["--binary", str(fake), "--files", "1", "--warmup", "0", "--runs", "3"]
            self.assertEqual(self.run_main(*common, "--max-median-ms", "100000")[0], 0)
            self.assertEqual(self.run_main(*common, "--max-median-ms", "0.001")[0], 1)


@unittest.skipIf(__import__('os').name == 'nt', 'Unix executable fixture')
class RepoMeasurementTests(unittest.TestCase):
    def make_repo(self, directory):
        repo = Path(directory) / "repo"
        (repo / "bin").mkdir(parents=True, exist_ok=True)
        (repo / "A.cs").write_text("class A {}\n")
        (repo / "Z.g.cs").write_text("class Z {}\n")
        (repo / "bin" / "B.cs").write_text("class B {}\n")
        return repo

    def run_repo(self, tmp, fake, *extra):
        repo = self.make_repo(tmp)
        target = Path(tmp) / "m.json"
        with __import__('contextlib').redirect_stdout(__import__('io').StringIO()):
            code = benchmark.main(["--binary", str(fake), "--baseline", str(fake), "--repo", str(repo),
                                   "--repo-name", "demo", "--warmup", "0", "--runs", "4", "--output", str(target), *extra])
        return code, json.loads(target.read_text())

    def test_successful_scan_records_counts_bytes_and_samples(self):
        with tempfile.TemporaryDirectory() as tmp:
            report = {"files_scanned": 1, "functions": [], "failed": False}
            fake, _ = fake_binary(tmp, report, f"cat '{tmp}/report.json'")
            code, result = self.run_repo(tmp, fake)
            self.assertEqual(code, 0)
            self.assertEqual((result["workload"], result["mode"], result["files"], result["source_bytes"]),
                             ("repo", "scan", 1, len("class A {}\n")))
            self.assertEqual(result["repo"], {"name": "demo", "files_scanned": 1, "scan_failure": None})
            self.assertEqual(result["callables"], 0)
            self.assertEqual(len(result["candidates"][0]["cpu_samples_ms"]), 4)
            self.assertTrue(result["candidates"][0]["outputs_byte_identical"])

    def test_an_agreed_syntax_failure_is_a_finding_not_an_error(self):
        with tempfile.TemporaryDirectory() as tmp:
            fake, _ = fake_binary(tmp, None, "echo 'error: invalid C# syntax in A.cs' >&2\nexit 2")
            code, result = self.run_repo(tmp, fake)
            self.assertEqual(code, 0)
            self.assertEqual(result["repo"]["scan_failure"], {
                "exit_code": 2, "message": "error: invalid C# syntax in A.cs", "syntax_failure": True})
            self.assertIsNone(result["repo"]["files_scanned"])
            self.assertIsNone(result["callables"])
            self.assertEqual(len(result["baseline"]["samples_ms"]), 4)

    def test_disagreement_between_base_and_head_is_an_error(self):
        with tempfile.TemporaryDirectory() as tmp:
            body = "case \"$0\" in */candidate/*) echo 'error: other' >&2 ;; *) echo 'error: invalid C# syntax in A.cs' >&2 ;; esac\nexit 2"
            fake, _ = fake_binary(tmp, None, body)
            with self.assertRaisesRegex(ValueError, "not byte-identical"):
                self.run_repo(tmp, fake)
            body = "case \"$0\" in */candidate/*) exit 0 ;; *) exit 2 ;; esac"
            fake, _ = fake_binary(tmp, None, body)
            with self.assertRaises(ValueError):
                self.run_repo(tmp, fake)

    def test_a_crash_or_malformed_report_is_an_error_even_when_executables_agree(self):
        with tempfile.TemporaryDirectory() as tmp:
            fake, _ = fake_binary(tmp, None, "echo 'thread panicked' >&2\nexit 101")
            with self.assertRaises(subprocess.CalledProcessError):
                self.run_repo(tmp, fake)
            fake, _ = fake_binary(tmp, {"files_scanned": "1", "functions": []}, f"cat '{tmp}/report.json'")
            with self.assertRaisesRegex(ValueError, "unexpected report"):
                self.run_repo(tmp, fake)

    def test_candidate_arguments_apply_to_repo_scans(self):
        with tempfile.TemporaryDirectory() as tmp:
            fake, log = fake_binary(tmp, {"files_scanned": 1, "functions": []}, f"cat '{tmp}/report.json'")
            self.run_repo(tmp, fake, "--candidate-args=--jobs 2")
            self.assertTrue(all("--jobs 2" in c for c in log.read_text().splitlines() if "/candidate/" in c))


if __name__ == "__main__":
    unittest.main()
