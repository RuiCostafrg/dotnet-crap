import contextlib
import importlib.util
import io
import json
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def module(name, path):
    """Load a tooling script without invoking its command-line entry point."""
    spec = importlib.util.spec_from_file_location(name, ROOT / path)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


benchmark = module("benchmark_for_summary_tests", "scripts/benchmark.py")
summary = module("bench_summary", "scripts/bench_summary.py")


def record(wall, cpu, rss=(100, 100, 100)):
    return {"wall_ms": list(wall), "cpu_ms": list(cpu), "rss_kib": list(rss)}


FASTER = record((5, 6, 7), (9, 10, 11), (150, 160, 170))
SAME = record((9, 11, 13), (19, 21, 23), (200, 210, 220))
SLOWER = record((20, 21, 22), (30, 31, 32), (300, 310, 320))


def measurement(workload="small-files", mode="scan", base=None, heads=(((), SAME),), repo=None, usable=4, counted=8):
    """A measurement shaped exactly like benchmark.py's output, built from its own helpers."""
    base = base or record((10, 11, 12), (20, 21, 22), (200, 210, 220))
    candidates = []
    for argv, head in heads:
        entry = {"args": list(argv), "label": benchmark.set_label(list(argv)), **benchmark.summarize(head),
                 "outputs_byte_identical": True, "comparison": benchmark.compare(base, head)}
        candidates.append(entry)
    result = {"version": 3,
              "provenance": {"platform": "Linux-test", "cpu": "Test CPU", "cpu_count": counted, "usable_cpus": usable},
              "workload": "repo" if repo else workload, "mode": mode, "files": 12, "source_bytes": 1234567,
              "candidates": candidates, "baseline": {"binary_sha256": "0", **benchmark.summarize(base)}}
    if repo:
        result["repo"] = repo
    return result


class RenderTests(unittest.TestCase):
    def test_separated_ratios_are_marked_and_overlapping_ones_are_not(self):
        text = summary.render([measurement(heads=(((), FASTER),)), measurement("dense-file", heads=(((), SAME),)),
                               measurement("solution", heads=(((), SLOWER),))], [], "dist")
        rows = {line.split(" | ")[0].strip("| "): line for line in text.splitlines() if line.startswith("| ")}
        self.assertIn("0.545 separated, lower", rows["small-files"])      # wall 6/11
        self.assertIn("0.476 separated, lower", rows["small-files"])      # CPU 10/21
        self.assertNotIn("separated", rows["dense-file"])
        self.assertIn("1.000", rows["dense-file"])
        self.assertIn("1.909 separated, higher", rows["solution"])
        self.assertIn("Build profile: `dist`", text)

    def test_columns_hold_wall_cpu_and_rss_for_base_and_head(self):
        text = summary.render([measurement(heads=(((), FASTER),))], [], "release")
        header = next(line for line in text.splitlines() if line.startswith("| workload"))
        self.assertEqual([c.strip() for c in header.strip("|").split("|")], [
            "workload", "mode", "base wall ms", "head wall ms", "wall ratio", "base CPU ms", "head CPU ms",
            "CPU ratio", "base RSS KiB", "head RSS KiB"])
        row = next(line for line in text.splitlines() if line.startswith("| small-files"))
        cells = [c.strip() for c in row.strip("|").split("|")]
        self.assertEqual(cells[2:5], ["11.0", "6.0", "0.545 separated, lower"])
        self.assertEqual(cells[5:7], ["21.0", "10.0"])
        self.assertEqual(cells[8:], ["210", "160"])

    def test_large_rss_is_not_printed_in_scientific_notation(self):
        big = record((10, 11, 12), (20, 21, 22), (2_500_000, 2_500_001, 2_500_002))
        text = summary.render([measurement(base=big, heads=(((), big),))], [], "dist")
        self.assertIn("| 2500001 | 2500001 |", text)

    def test_sweep_adds_one_row_per_argument_set_and_an_args_column(self):
        sweep = measurement(heads=((("--jobs", "1"), SAME), (("--jobs", "4"), FASTER)))
        text = summary.render([sweep], [], "dist")
        self.assertIn("head args", text)
        rows = [line for line in text.splitlines() if line.startswith("| small-files")]
        self.assertEqual(len(rows), 2)
        self.assertIn("--jobs 1", rows[0])
        self.assertIn("--jobs 4", rows[1])
        self.assertIn("separated", rows[1])
        self.assertNotIn("separated", rows[0])
        self.assertNotIn("head args", summary.render([measurement(heads=(((), SAME),))], [], "dist"))

    def test_rows_follow_the_workload_and_mode_order(self):
        items = [measurement("skew", "scan"), measurement("small-files", "collect"), measurement("small-files", "scan"),
                 measurement("many-medium", "scan-coverage")]
        text = summary.render(items, [], "dist")
        order = [line.split("|")[1].strip() + "/" + line.split("|")[2].strip()
                 for line in text.splitlines() if line.startswith("| ") and "workload" not in line]
        self.assertEqual(order, ["small-files/scan", "small-files/collect", "many-medium/scan-coverage", "skew/scan"])

    def test_runner_line_reports_usable_and_counted_cpus(self):
        text = summary.render([measurement(usable=2, counted=8)], [], "dist")
        self.assertIn("Runner: Linux-test, Test CPU, 2 usable CPUs (os.cpu_count 8).", text)
        text = summary.render([measurement(usable=4, counted=4)], [], "dist")
        self.assertIn("4 usable CPUs.", text)
        self.assertNotIn("os.cpu_count", text)


class RepoRowTests(unittest.TestCase):
    def repo(self, name, failure=None, scanned=12):
        return measurement(repo={"name": name, "files_scanned": None if failure else scanned, "scan_failure": failure},
                           heads=(((), FASTER),))

    def test_successful_repo_row(self):
        text = summary.render([], [self.repo("owner/lib@abc123")], "dist")
        row = next(line for line in text.splitlines() if line.startswith("| owner/lib@abc123"))
        cells = [c.strip() for c in row.strip("|").split("|")]
        self.assertEqual(cells[1:3], ["12", "1,234,567"])
        self.assertEqual(cells[-1], "ok")
        self.assertIn("separated", row)
        self.assertNotIn("not a job failure", text)

    def test_syntax_failure_is_a_finding_in_the_row_and_noted_below(self):
        failure = {"exit_code": 2, "message": "error: invalid C# syntax in src/A|B.cs", "syntax_failure": True}
        text = summary.render([], [self.repo("owner/app@def456", failure)], "dist")
        row = next(line for line in text.splitlines() if line.startswith("| owner/app@def456"))
        self.assertIn("syntax failure: error: invalid C# syntax in src/A\\|B.cs", row)
        self.assertIn("not a job failure", text)

    def test_other_failure_names_its_exit_code(self):
        failure = {"exit_code": 2, "message": "error: source is not UTF-8", "syntax_failure": False}
        self.assertIn("failed (exit 2): error: source is not UTF-8",
                      summary.render([], [self.repo("r@1", failure)], "dist"))

    def test_a_file_count_disagreement_is_visible(self):
        text = summary.render([], [self.repo("r@1", scanned=11)], "dist")
        self.assertIn("scanner read 11 files and the harness counted 12", text)

    def test_repos_are_listed_by_name(self):
        text = summary.render([], [self.repo("b/two@1"), self.repo("a/one@1")], "dist")
        self.assertLess(text.index("a/one@1"), text.index("b/two@1"))


class MainTests(unittest.TestCase):
    def test_reads_directories_and_prints_both_tables(self):
        with tempfile.TemporaryDirectory() as tmp:
            bench, repos = Path(tmp) / "b", Path(tmp) / "r"
            bench.mkdir()
            repos.mkdir()
            (bench / "small-files-scan.json").write_text(json.dumps(measurement(heads=(((), FASTER),))))
            repo = measurement(repo={"name": "o/r@1", "files_scanned": 12, "scan_failure": None},
                               heads=(((), SAME),))
            (repos / "o-r.json").write_text(json.dumps(repo))
            out = io.StringIO()
            with contextlib.redirect_stdout(out):
                code = summary.main(["--profile", "dist", "--benchmarks", str(bench), "--repos", str(repos)])
            self.assertEqual(code, 0)
            self.assertIn("### Generated workloads", out.getvalue())
            self.assertIn("### Real repositories (scan only)", out.getvalue())

    def test_no_measurements_is_a_usage_error_and_a_missing_directory_is_empty(self):
        with tempfile.TemporaryDirectory() as tmp, self.assertRaises(SystemExit), \
                contextlib.redirect_stderr(io.StringIO()):
            summary.main(["--profile", "dist", "--benchmarks", tmp, "--repos", str(Path(tmp) / "missing")])
        self.assertEqual(summary.load(None), [])


if __name__ == "__main__":
    unittest.main()
