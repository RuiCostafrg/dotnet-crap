import contextlib
import importlib.util
import io
import json
import os
import re
import shlex
import tempfile
import time
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def module(name, path):
    """Load a tooling script without invoking its command-line entry point."""
    spec = importlib.util.spec_from_file_location(name, ROOT / path)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


evaluate = module("evaluate_projects", "scripts/evaluate-projects.py")

PROJECTS = json.loads((ROOT / "evaluation" / "projects.json").read_text(encoding="utf-8"))


class ProjectSchemaTests(unittest.TestCase):
    def test_project_count_and_required_fields(self):
        self.assertGreaterEqual(len(PROJECTS), 3)
        self.assertLessEqual(len(PROJECTS), 5)
        required = {"name", "kind", "repository", "commit", "test", "paths", "add_coverlet", "expected_reports"}
        for project in PROJECTS:
            with self.subTest(project=project.get("name")):
                self.assertTrue(required <= set(project))
                self.assertTrue(project["name"])
                self.assertIn(project["kind"], {"library", "web", "solution"})
                self.assertTrue(project["test"].endswith((".csproj", ".sln", ".slnx")))

    def test_names_are_unique(self):
        names = [project["name"] for project in PROJECTS]
        self.assertEqual(len(names), len(set(names)))

    def test_commits_are_full_lowercase_shas(self):
        for project in PROJECTS:
            with self.subTest(project=project["name"]):
                self.assertRegex(project["commit"], re.compile(r"^[0-9a-f]{40}$"))

    def test_repositories_are_https_urls(self):
        for project in PROJECTS:
            with self.subTest(project=project["name"]):
                self.assertTrue(project["repository"].startswith("https://"))
                self.assertNotIn(" ", project["repository"])

    def test_kinds_cover_the_required_shapes(self):
        kinds = {project["kind"] for project in PROJECTS}
        self.assertTrue({"library", "web", "solution"} <= kinds)

    def test_paths_and_expected_reports(self):
        for project in PROJECTS:
            with self.subTest(project=project["name"]):
                self.assertIsInstance(project["paths"], list)
                self.assertTrue(project["paths"])
                for path in project["paths"]:
                    self.assertIsInstance(path, str)
                    self.assertFalse(Path(path).is_absolute())
                self.assertIsInstance(project["expected_reports"], int)
                self.assertGreaterEqual(project["expected_reports"], 1)
                self.assertIsInstance(project["add_coverlet"], list)
                for path in project["add_coverlet"]:
                    self.assertTrue(path.endswith(".csproj"))
                self.assertIsInstance(project.get("test_args", []), list)

    def test_workflow_matrix_matches_project_names(self):
        try:
            import yaml
        except ImportError:
            self.skipTest("PyYAML is not installed")
        workflow = yaml.safe_load((ROOT / ".github" / "workflows" / "evaluate.yml").read_text(encoding="utf-8"))
        names = workflow["jobs"]["evaluate"]["strategy"]["matrix"]["project"]
        self.assertEqual(sorted(names), sorted(project["name"] for project in PROJECTS))


class TimeParsingTests(unittest.TestCase):
    def test_parses_minutes_and_rss(self):
        text = (
            "\tCommand being timed: \"dotnet-crap\"\n"
            "\tElapsed (wall clock) time (h:mm:ss or m:ss): 1:02.50\n"
            "\tMaximum resident set size (kbytes): 123456\n"
        )
        self.assertEqual(evaluate.parse_time_v(text), (62.5, 123456))

    def test_parses_hours_and_sub_second(self):
        self.assertEqual(evaluate.parse_time_v("Elapsed (wall clock) time (h:mm:ss or m:ss): 1:00:03.25"), (3603.25, None))
        self.assertEqual(evaluate.parse_time_v("Elapsed (wall clock) time (h:mm:ss or m:ss): 0:00.01"), (0.01, None))

    def test_missing_fields_are_none(self):
        self.assertEqual(evaluate.parse_time_v("nothing here"), (None, None))


class SummaryTests(unittest.TestCase):
    def report(self):
        return {
            "threshold": 10.0,
            "files_scanned": 4,
            "coverage_complete": False,
            "failed": True,
            "skipped_files": [{"path": "src/Bad.cs", "message": "syntax error at line 1"}],
            "functions": [
                {"file": "src/A.cs", "name": "One", "kind": "method", "line": 1, "column": 1,
                 "end_line": 2, "complexity": 8, "coverage": 0.0, "crap": 72.0},
                {"file": "src/A.cs", "name": "Two", "kind": "method", "line": 3, "column": 1,
                 "end_line": 4, "complexity": 2, "coverage": 1.0, "crap": 2.0},
                {"file": "src/B.cs", "name": "Three", "kind": "method", "line": 5, "column": 1,
                 "end_line": 6, "complexity": 5, "coverage": None, "crap": 30.0},
                {"file": "src/B.cs", "name": "Four", "kind": "method", "line": 7, "column": 1,
                 "end_line": 8, "complexity": 6, "coverage": 0.5, "crap": 27.5},
                {"file": "src/C.cs", "name": "Five", "kind": "method", "line": 9, "column": 1,
                 "end_line": 10, "complexity": 7, "coverage": 0.25, "crap": 55.0},
                {"file": "src/C.cs", "name": "Six", "kind": "method", "line": 11, "column": 1,
                 "end_line": 12, "complexity": 1, "coverage": 1.0, "crap": 1.0},
            ],
        }

    def test_counts_and_ratios(self):
        summary = evaluate.summarize_report(self.report())
        self.assertEqual(summary["files_scanned"], 4)
        self.assertEqual(summary["callables"], 6)
        self.assertEqual(summary["callables_with_coverage"], 5)
        self.assertAlmostEqual(summary["percent_with_coverage"], 83.33)
        self.assertEqual(summary["max_crap"], 72.0)
        self.assertEqual(summary["crap_over_30"], 2)
        self.assertFalse(summary["coverage_complete"])
        self.assertTrue(summary["report_failed"])

    def test_top_five_is_ordered_and_shaped(self):
        summary = evaluate.summarize_report(self.report())
        self.assertEqual(len(summary["top_callables"]), 5)
        self.assertEqual([item["name"] for item in summary["top_callables"]],
                         ["One", "Five", "Three", "Four", "Two"])
        self.assertEqual(set(summary["top_callables"][0]),
                         {"name", "file", "line", "crap", "complexity", "coverage"})

    def test_skipped_files_are_reported(self):
        summary = evaluate.summarize_report(self.report())
        self.assertEqual(summary["skipped_files_count"], 1)
        self.assertEqual(summary["skipped_files"][0]["path"], "src/Bad.cs")
        self.assertIn("syntax error", summary["skipped_files"][0]["message"])

    def test_threshold_count_and_gate(self):
        summary = evaluate.summarize_report(self.report())
        self.assertEqual(summary["threshold"], 10.0)
        self.assertEqual(summary["crap_over_threshold"], 4)
        self.assertEqual(summary["gate"], "fail")
        report = self.report()
        report.update(threshold=40.0, failed=False)
        summary = evaluate.summarize_report(report)
        self.assertEqual(summary["crap_over_threshold"], 2)
        self.assertEqual(summary["gate"], "pass")

    def test_empty_report_does_not_divide_by_zero(self):
        summary = evaluate.summarize_report({"files_scanned": 0, "functions": []})
        self.assertEqual(summary["callables"], 0)
        self.assertEqual(summary["percent_with_coverage"], 0.0)
        self.assertEqual(summary["max_crap"], 0.0)
        self.assertEqual(summary["top_callables"], [])
        self.assertEqual(summary["skipped_files_count"], 0)


class TailTests(unittest.TestCase):
    def test_reads_only_the_last_bytes(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "log"
            path.write_text("a" * 50 + "tail", encoding="utf-8")
            self.assertEqual(evaluate.tail(path, 4), "tail")

    def test_short_files_are_returned_whole(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "log"
            path.write_text("short", encoding="utf-8")
            self.assertEqual(evaluate.tail(path, 4000), "short")

    def test_invalid_utf8_is_replaced(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "log"
            path.write_bytes(b"\xff\xfeok")
            self.assertEqual(evaluate.tail(path, 2), "ok")

    def test_missing_file_is_empty(self):
        self.assertEqual(evaluate.tail(Path("/nonexistent/evaluate-projects.log")), "")


class RunProcessTests(unittest.TestCase):
    @unittest.skipUnless(os.name == "posix", "process groups are POSIX-only")
    def test_timeout_kills_the_whole_process_group(self):
        with tempfile.TemporaryDirectory() as tmp:
            pgid_file = Path(tmp) / "pgid"
            command = f"echo $$ > {shlex.quote(str(pgid_file))}; sleep 30 & sleep 30"
            result = evaluate.run_process(
                ["sh", "-c", command],
                cwd=Path(tmp),
                log_path=Path(tmp) / "child.log",
                timeout=1,
                env=evaluate.child_env(),
            )
            self.assertTrue(result["timed_out"])
            self.assertIsNotNone(result["exit_code"])
            pgid = int(pgid_file.read_text(encoding="utf-8").strip())
            deadline = time.monotonic() + 10
            while time.monotonic() < deadline:
                try:
                    os.killpg(pgid, 0)
                except ProcessLookupError:
                    break
                time.sleep(0.05)
            else:
                self.fail("the child process group is still alive after the timeout")


class CheckoutTests(unittest.TestCase):
    def project(self, repository, commit):
        return {
            "name": "Demo",
            "kind": "library",
            "repository": repository,
            "commit": commit,
            "test": "tests/Demo.Tests/Demo.Tests.csproj",
            "paths": ["src"],
            "add_coverlet": [],
            "expected_reports": 1,
        }

    def test_partial_checkout_is_removed_before_cloning(self):
        with tempfile.TemporaryDirectory() as tmp:
            work = Path(tmp) / "work"
            repo = work / "Demo" / "repo"
            repo.mkdir(parents=True)
            (repo / "leftover.txt").write_text("stale", encoding="utf-8")
            project = self.project("file:///nonexistent/evaluate-projects.git", "c" * 40)
            with self.assertRaises(evaluate.EvaluationError):
                evaluate.checkout(project, work, Path(tmp) / "out", evaluate.child_env(), [])
            self.assertFalse((repo / "leftover.txt").exists())

    def test_modified_checkout_is_recloned_at_the_pinned_commit(self):
        with tempfile.TemporaryDirectory() as tmp:
            source = Path(tmp) / "source"
            source.mkdir()
            (source / "file.txt").write_text("one", encoding="utf-8")

            def git(*argv):
                return evaluate.subprocess.run(
                    ["git", "-C", str(source), *argv],
                    check=True, capture_output=True, text=True,
                ).stdout.strip()

            git("init", "-q")
            git("config", "user.email", "eval@example.invalid")
            git("config", "user.name", "Evaluation")
            git("add", "file.txt")
            git("commit", "-q", "-m", "pinned")
            commit = git("rev-parse", "HEAD")

            work = Path(tmp) / "work"
            project = self.project(source.as_uri(), commit)
            out = Path(tmp) / "out"
            repo = evaluate.checkout(project, work, out, evaluate.child_env(), [])
            (repo / "file.txt").write_text("modified", encoding="utf-8")
            (repo / "untracked.txt").write_text("scratch", encoding="utf-8")
            repo = evaluate.checkout(project, work, out, evaluate.child_env(), [])
            self.assertEqual((repo / "file.txt").read_text(encoding="utf-8"), "one")
            self.assertFalse((repo / "untracked.txt").exists())


class CommandTests(unittest.TestCase):
    project = {
        "name": "Demo",
        "paths": ["src", "lib"],
        "test": "tests/Demo.Tests/Demo.Tests.csproj",
        "test_args": ["--framework", "net10.0"],
        "expected_reports": 1,
    }

    def test_collect_uses_the_pinned_coverlet_pattern(self):
        argv = evaluate.collect_argv(self.project, "dotnet-crap", Path("/repo"), Path("/tmp/coverage.json"))
        self.assertEqual(argv[:2], ["dotnet-crap", "collect"])
        self.assertEqual(argv.count("{results}"), 1)
        self.assertIn("XPlat Code Coverage", argv)
        self.assertEqual(argv[-1], "DataCollectionRunSettings.DataCollectors.DataCollector.Configuration.Format=cobertura")
        self.assertEqual(argv[argv.index("--expected-reports") + 1], "1")
        self.assertEqual([argv[i + 1] for i, item in enumerate(argv) if item == "--path"], ["src", "lib"])
        self.assertEqual(argv[argv.index("--") + 1:argv.index("--") + 5],
                         ["dotnet", "test", "tests/Demo.Tests/Demo.Tests.csproj", "--framework"])

    def test_scan_uses_json_and_the_collected_coverage(self):
        argv = evaluate.scan_argv(self.project, "dotnet-crap", Path("/repo"), Path("/tmp/c.json"), Path("/tmp/r.json"))
        self.assertIn("--format", argv)
        self.assertEqual(argv[argv.index("--format") + 1], "json")
        self.assertEqual(argv[argv.index("--coverage") + 1], "/tmp/c.json")
        self.assertEqual(argv[-2:], ["src", "lib"])

    def test_child_environment_is_an_allowlist(self):
        env = evaluate.child_env({
            "GH_TOKEN": "x", "GITHUB_TOKEN": "x", "AWS_SECRET_ACCESS_KEY": "x",
            "NUGET_API_KEY": "x", "DOTNET_TOKEN": "x", "NUGET_PASSWORD": "x",
            "RANDOM_VAR": "x", "PATH": "/bin", "HOME": "/h", "CI": "true",
            "DOTNET_ROOT": "/d", "NUGET_PACKAGES": "/n", "DOTNET_NOLOGO": "0",
        })
        self.assertEqual(env, {
            "PATH": "/bin", "HOME": "/h", "CI": "true", "DOTNET_ROOT": "/d",
            "NUGET_PACKAGES": "/n", "DOTNET_NOLOGO": "1",
            "DOTNET_CLI_TELEMETRY_OPTOUT": "1",
        })


class TableTests(unittest.TestCase):
    def summary(self, name="Demo", status="ok", error=None):
        return {
            "name": name, "kind": "library", "commit": "a" * 40,
            "files_scanned": 12, "callables": 34, "percent_with_coverage": 56.25,
            "max_crap": 72.5, "crap_over_30": 3, "skipped_files_count": 1,
            "collect_seconds": 12.5, "collect_peak_rss_kb_largest_process": 204800,
            "scan_seconds": 0.25, "scan_peak_rss_kb": 51200,
            "status": status, "error": error,
        }

    def test_table_has_columns_and_rows(self):
        text = evaluate.render_table([self.summary("Zeta"), self.summary("Alpha", "error", "boom | pipe")])
        header = text.splitlines()[0]
        self.assertTrue(header.startswith("| Project | Kind | Commit |"))
        rows = [line for line in text.splitlines() if line.startswith("| ") and "Project" not in line]
        self.assertEqual(len(rows), 2)
        self.assertIn("Alpha", rows[0])
        self.assertIn("boom \\| pipe", rows[0])
        self.assertIn("Zeta", rows[1])
        self.assertIn("200.0", rows[1])

    def test_missing_metrics_render_as_not_available(self):
        item = self.summary()
        cells_by_column = {
            "Files": "files_scanned", "Callables": "callables", "Covered %": "percent_with_coverage",
            "Max CRAP": "max_crap", "CRAP > 30": "crap_over_30", "Collect s": "collect_seconds",
            "Collect peak RSS (largest process, MiB)": "collect_peak_rss_kb_largest_process",
            "Scan s": "scan_seconds", "Scan peak RSS (MiB)": "scan_peak_rss_kb",
            "Skipped": "skipped_files_count",
        }
        for key in cells_by_column.values():
            item[key] = None
        lines = evaluate.render_table([item]).splitlines()
        header = [c.strip() for c in lines[0].strip("|").split(" | ")]
        row = [c.strip() for c in lines[2].strip("|").split(" | ")]
        self.assertEqual(len(header), len(row))
        cells = dict(zip(header, row))
        for column in cells_by_column:
            self.assertEqual(cells[column], "n/a", column)
        self.assertEqual(cells["Project"], "Demo")

    def test_missing_summary_renders_a_missing_row(self):
        lines = evaluate.render_table([self.summary("A")], expected=["A", "B"]).splitlines()
        self.assertEqual(len(lines), 4)
        self.assertIn("| B |", lines[3])
        self.assertTrue(lines[3].rstrip().endswith("| missing |"))

    def test_gate_column(self):
        passing = self.summary("P"); passing.update(gate="pass")
        failing = self.summary("F"); failing.update(gate="fail", crap_over_threshold=7)
        text = evaluate.render_table([passing, failing])
        self.assertIn("| pass |", text)
        self.assertIn("| fail (7 over threshold) |", text)

    def test_table_exit_code(self):
        def run_table(summaries, expected):
            with tempfile.TemporaryDirectory() as tmp:
                for item in summaries:
                    (Path(tmp) / f"{item['name']}.summary.json").write_text(json.dumps(item))
                real = evaluate.load_projects
                evaluate.load_projects = lambda *a, **k: [{"name": n} for n in expected]
                try:
                    with contextlib.redirect_stdout(io.StringIO()) as out:
                        code = evaluate.main(["table", "--out", tmp])
                finally:
                    evaluate.load_projects = real
            return code, out.getvalue()
        self.assertEqual(run_table([self.summary("A")], ["A"])[0], 0)
        code, text = run_table([self.summary("A")], ["A", "B"])
        self.assertEqual(code, 1)
        self.assertIn("| B |", text)
        self.assertEqual(run_table([self.summary("A", "error", "boom")], ["A"])[0], 1)

    def test_load_summaries_skips_malformed_files(self):
        with tempfile.TemporaryDirectory() as tmp:
            (Path(tmp) / "a.summary.json").write_text(json.dumps(self.summary("A")))
            (Path(tmp) / "b.summary.json").write_text("{not json")
            (Path(tmp) / "ignored.json").write_text(json.dumps(self.summary("C")))
            summaries = evaluate.load_summaries(Path(tmp))
        self.assertEqual([item["name"] for item in summaries], ["A"])


class FailureSummaryTests(unittest.TestCase):
    project = {
        "name": "Broken",
        "kind": "library",
        "repository": "https://example.invalid/repo",
        "commit": "b" * 40,
        "test": "tests/Broken.Tests/Broken.Tests.csproj",
        "paths": ["src"],
        "add_coverlet": [],
        "expected_reports": 1,
    }

    def test_missing_binary_writes_an_error_summary(self):
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp) / "out"
            summary = evaluate.run_project(self.project, Path(tmp) / "missing", Path(tmp) / "work", out)
            self.assertEqual(summary["status"], "error")
            self.assertIn("binary does not exist", summary["error"])
            self.assertIsNone(summary["collect_exit_code"])
            on_disk = json.loads((out / "Broken.summary.json").read_text())
            self.assertEqual(on_disk["status"], "error")
            self.assertTrue((out / "Broken.report.json").is_file())

    def test_main_reports_failure_without_traceback(self):
        stderr = io.StringIO()
        with tempfile.TemporaryDirectory() as tmp, contextlib.redirect_stderr(stderr):
            code = evaluate.main([
                "run", "--project", "Broken", "--binary", str(Path(tmp) / "missing"),
                "--work", str(Path(tmp) / "work"), "--out", str(Path(tmp) / "out"),
            ])
        self.assertEqual(code, 2)  # Broken is not in evaluation/projects.json
        self.assertIn("unknown project", stderr.getvalue())
        self.assertNotIn("Traceback", stderr.getvalue())



class ErrorLineTests(unittest.TestCase):
    def test_final_error_line_is_kept_from_a_log_tail(self):
        error = ("collect failed with exit 2: Passed!  - Failed: 0, Passed: 44\n"
                 "Attachments:\n  /tmp/x/coverage.cobertura.xml\n"
                 "error: invalid Cobertura report /tmp/x/coverage.cobertura.xml: outside the root\n")
        self.assertEqual(evaluate.error_line(error),
                         "invalid Cobertura report /tmp/x/coverage.cobertura.xml: outside the root")

    def test_last_line_without_error_prefix_and_truncation(self):
        self.assertEqual(evaluate.error_line("first\n\nlast line\n"), "last line")
        self.assertEqual(evaluate.error_line(None), "unknown")
        long = evaluate.error_line("x" * 1000)
        self.assertEqual(len(long), evaluate.ERROR_CELL_LIMIT + 1)
        self.assertTrue(long.endswith("…"))

if __name__ == "__main__":
    unittest.main()
