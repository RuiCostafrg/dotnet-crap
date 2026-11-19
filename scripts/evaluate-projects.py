#!/usr/bin/env python3
"""Reproducible evaluation of dotnet-crap on pinned real C# projects.

Usage:
    evaluate-projects.py run --project NAME --binary PATH --work DIR --out DIR
    evaluate-projects.py table --out DIR

``run`` checks out an exact commit, optionally adds ``coverlet.collector``, runs
``dotnet-crap collect`` (which runs ``dotnet test`` with the XPlat collector) and
then scans the collected native coverage with ``--format json``.  Both steps run
under GNU ``time -v`` so wall time and maximum RSS are recorded.  A failing test
run or scan is reported in ``<out>/<name>.summary.json``; it is not a harness
crash.  ``table`` renders every summary in a directory as Markdown.

Only the Python standard library is used.  GH_TOKEN and GITHUB_TOKEN are removed
from every child environment.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PROJECTS_FILE = ROOT / "evaluation" / "projects.json"
COVERLET_VERSION = "10.1.1"
GNU_TIME = "/usr/bin/time"
COLLECT_TIMEOUT = 2100
COLLECT_CHILD_TIMEOUT = 1800
SCAN_TIMEOUT = 300
GIT_TIMEOUT = 600
ADD_PACKAGE_TIMEOUT = 600
TAIL_LIMIT = 4000

_ELAPSED = re.compile(
    r"Elapsed \(wall clock\) time \(h:mm:ss or m:ss\):\s*(?:(\d+):)?(\d+):(\d+(?:\.\d+)?)"
)
_RSS = re.compile(r"Maximum resident set size \(kbytes\):\s*(\d+)")


class EvaluationError(RuntimeError):
    """A step that prevents producing a usable evaluation result."""


def load_projects(path: Path = PROJECTS_FILE) -> list[dict]:
    """Read and validate the pinned project list."""
    data = json.loads(Path(path).read_text(encoding="utf-8"))
    if not isinstance(data, list):
        raise EvaluationError(f"{path}: expected a JSON array")
    return data


def find_project(name: str, path: Path = PROJECTS_FILE) -> dict:
    """Return one project by its exact name."""
    for project in load_projects(path):
        if project.get("name") == name:
            return project
    names = ", ".join(sorted(str(p.get("name")) for p in load_projects(path)))
    raise EvaluationError(f"unknown project {name!r}; available: {names}")


def child_env(base: dict | None = None) -> dict:
    """Copy the environment for child processes and remove credentials."""
    env = dict(os.environ if base is None else base)
    for key in ("GH_TOKEN", "GITHUB_TOKEN"):
        env.pop(key, None)
    env["DOTNET_CLI_TELEMETRY_OPTOUT"] = "1"
    env["DOTNET_NOLOGO"] = "1"
    env["DOTNET_SKIP_FIRST_TIME_EXPERIENCE"] = "1"
    env["GIT_TERMINAL_PROMPT"] = "0"
    return env


def parse_time_v(text: str) -> tuple[float | None, int | None]:
    """Extract (elapsed seconds, maximum RSS KiB) from GNU time -v output."""
    seconds = None
    match = _ELAPSED.search(text)
    if match:
        hours = int(match.group(1) or 0)
        minutes = int(match.group(2))
        secs = float(match.group(3))
        seconds = round(hours * 3600 + minutes * 60 + secs, 3)
    rss = None
    match = _RSS.search(text)
    if match:
        rss = int(match.group(1))
    return seconds, rss


def tail(path: Path, limit: int = TAIL_LIMIT) -> str:
    """Return the last ``limit`` characters of a log, treating it as data."""
    try:
        text = path.read_text(encoding="utf-8", errors="replace")
    except OSError:
        return ""
    return text[-limit:]


def run_process(
    argv: list[str],
    *,
    cwd: Path,
    log_path: Path,
    timeout: int,
    env: dict,
) -> dict:
    """Run a command with combined output captured to a log file."""
    log_path.parent.mkdir(parents=True, exist_ok=True)
    started = time.monotonic()
    timed_out = False
    try:
        with log_path.open("wb") as log:
            completed = subprocess.run(
                argv,
                cwd=str(cwd),
                stdin=subprocess.DEVNULL,
                stdout=log,
                stderr=subprocess.STDOUT,
                env=env,
                timeout=timeout,
            )
        code: int | None = completed.returncode
    except subprocess.TimeoutExpired:
        code = None
        timed_out = True
    except OSError as error:
        code = None
        log_path.write_text(f"harness: could not start {argv[0]}: {error}\n", encoding="utf-8")
    return {
        "argv": argv,
        "exit_code": code,
        "seconds": round(time.monotonic() - started, 3),
        "timed_out": timed_out,
        "log": str(log_path),
    }


def run_timed(
    argv: list[str],
    *,
    cwd: Path,
    log_path: Path,
    time_log: Path,
    timeout: int,
    env: dict,
    friction: list[str],
) -> tuple[dict, int | None]:
    """Run ``argv`` under GNU time -v and return the result plus max RSS KiB."""
    prefix: list[str] = []
    if Path(GNU_TIME).exists():
        prefix = [GNU_TIME, "-v", "-o", str(time_log)]
        time_log.parent.mkdir(parents=True, exist_ok=True)
        if time_log.exists():
            time_log.unlink()
    else:
        friction.append("GNU time (/usr/bin/time) is unavailable; maximum RSS was not recorded")
    result = run_process(prefix + argv, cwd=cwd, log_path=log_path, timeout=timeout, env=env)
    rss = None
    if prefix and time_log.exists():
        _, rss = parse_time_v(time_log.read_text(encoding="utf-8", errors="replace"))
    return result, rss


def checkout(project: dict, work_dir: Path, out_dir: Path, env: dict, friction: list[str]) -> Path:
    """Clone the pinned commit with a shallow fetch and verify HEAD exactly."""
    name = project["name"]
    commit = project["commit"]
    repo = work_dir / name / "repo"
    log = out_dir / f"{name}.git.log"
    if (repo / ".git").is_dir():
        head = run_process(
            ["git", "-C", str(repo), "rev-parse", "HEAD"],
            cwd=repo,
            log_path=log,
            timeout=60,
            env=env,
        )
        current = ""
        try:
            current = (out_dir / f"{name}.git.log").read_text(encoding="utf-8").strip()
        except OSError:
            pass
        if head["exit_code"] == 0 and current == commit:
            return repo
        shutil.rmtree(repo)
    repo.parent.mkdir(parents=True, exist_ok=True)
    repo.mkdir()
    steps = [
        ["git", "init", "-q", str(repo)],
        ["git", "-C", str(repo), "remote", "add", "origin", project["repository"]],
        ["git", "-C", str(repo), "fetch", "--depth", "1", "origin", commit],
        ["git", "-C", str(repo), "-c", "advice.detachedHead=false", "checkout", "FETCH_HEAD"],
    ]
    for argv in steps:
        result = run_process(argv, cwd=repo, log_path=log, timeout=GIT_TIMEOUT, env=env)
        if result["exit_code"] != 0:
            raise EvaluationError(
                f"git step failed ({' '.join(argv[:4])}...): {tail(log, 1200)}"
            )
    head = run_process(
        ["git", "-C", str(repo), "rev-parse", "HEAD"],
        cwd=repo,
        log_path=out_dir / f"{name}.git-head.log",
        timeout=60,
        env=env,
    )
    actual = ""
    try:
        actual = (out_dir / f"{name}.git-head.log").read_text(encoding="utf-8").strip()
    except OSError:
        pass
    if head["exit_code"] != 0 or actual != commit:
        raise EvaluationError(f"checked out {actual or 'unknown'} but expected {commit}")
    friction.append(f"checked out {project['repository']} at {commit}")
    return repo


def add_coverlet(project: dict, repo: Path, out_dir: Path, env: dict, friction: list[str]) -> None:
    """Add coverlet.collector to the test projects that do not reference it."""
    name = project["name"]
    for relative in project.get("add_coverlet", []):
        result = run_process(
            ["dotnet", "add", relative, "package", "coverlet.collector", "--version", COVERLET_VERSION],
            cwd=repo,
            log_path=out_dir / f"{name}.dotnet-add.log",
            timeout=ADD_PACKAGE_TIMEOUT,
            env=env,
        )
        if result["exit_code"] != 0:
            raise EvaluationError(
                f"dotnet add coverlet.collector {COVERLET_VERSION} {relative} failed: "
                f"{tail(out_dir / f'{name}.dotnet-add.log', 1200)}"
            )
        friction.append(f"added coverlet.collector {COVERLET_VERSION} to {relative}")


def collect_argv(project: dict, binary: str, repo: Path, coverage: Path) -> list[str]:
    """Build the exact Coverlet Cobertura collection command."""
    argv = [
        binary,
        "collect",
        "--root",
        str(repo),
    ]
    for path in project["paths"]:
        argv += ["--path", path]
    argv += [
        "--output",
        str(coverage),
        "--expected-reports",
        str(project["expected_reports"]),
        "--timeout-seconds",
        str(COLLECT_CHILD_TIMEOUT),
        "--skip-unparsed",
        "--",
        "dotnet",
        "test",
        project["test"],
    ]
    argv += list(project.get("test_args", []))
    argv += [
        "--collect",
        "XPlat Code Coverage",
        "--results-directory",
        "{results}",
        "--",
        "DataCollectionRunSettings.DataCollectors.DataCollector.Configuration.Format=cobertura",
    ]
    return argv


def scan_argv(project: dict, binary: str, repo: Path, coverage: Path, report: Path) -> list[str]:
    """Build the JSON scan command over the collected coverage."""
    argv = [
        binary,
        "--root",
        str(repo),
        "--coverage",
        str(coverage),
        "--format",
        "json",
        "--output",
        str(report),
        "--skip-unparsed",
        "--threshold",
        "10",
    ]
    argv += list(project["paths"])
    return argv


def summarize_report(report: dict) -> dict:
    """Compute the summary fields from a dotnet-crap JSON report."""
    functions = report.get("functions") or []
    callables = len(functions)
    covered = sum(1 for item in functions if item.get("coverage") is not None)
    craps = [float(item.get("crap", 0.0)) for item in functions]
    top = sorted(
        functions,
        key=lambda item: (-float(item.get("crap", 0.0)), item.get("file", ""), int(item.get("line", 0))),
    )[:5]
    skipped = report.get("skipped_files") or []
    return {
        "files_scanned": report.get("files_scanned"),
        "callables": callables,
        "callables_with_coverage": covered,
        "percent_with_coverage": round(100.0 * covered / callables, 2) if callables else 0.0,
        "max_crap": round(max(craps), 3) if craps else 0.0,
        "crap_over_30": sum(1 for value in craps if value > 30.0),
        "top_callables": [
            {
                "name": item.get("name"),
                "file": item.get("file"),
                "line": item.get("line"),
                "crap": round(float(item.get("crap", 0.0)), 3),
                "complexity": item.get("complexity"),
                "coverage": None if item.get("coverage") is None else round(float(item["coverage"]), 4),
            }
            for item in top
        ],
        "skipped_files_count": len(skipped),
        "skipped_files": [
            {"path": item.get("path"), "message": item.get("message")} for item in skipped
        ],
        "coverage_complete": bool(report.get("coverage_complete")),
        "report_failed": bool(report.get("failed")),
    }


def base_summary(project: dict) -> dict:
    """A summary with every field present, even when a step never ran."""
    return {
        "name": project["name"],
        "kind": project["kind"],
        "repository": project["repository"],
        "commit": project["commit"],
        "test": project["test"],
        "test_args": list(project.get("test_args", [])),
        "paths": list(project["paths"]),
        "expected_reports": project["expected_reports"],
        "status": "error",
        "error": None,
        "files_scanned": None,
        "callables": None,
        "callables_with_coverage": None,
        "percent_with_coverage": None,
        "max_crap": None,
        "crap_over_30": None,
        "top_callables": [],
        "skipped_files_count": None,
        "skipped_files": [],
        "coverage_complete": None,
        "report_failed": None,
        "collect_seconds": None,
        "collect_max_rss_kb": None,
        "collect_exit_code": None,
        "scan_seconds": None,
        "scan_max_rss_kb": None,
        "scan_exit_code": None,
        "friction": [],
    }


def write_json(path: Path, data: dict) -> None:
    """Write stable, human-diffable JSON."""
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, indent=2, sort_keys=False) + "\n", encoding="utf-8")


def run_project(project: dict, binary: Path, work_dir: Path, out_dir: Path) -> dict:
    """Run the whole evaluation for one project and always return a summary."""
    out_dir.mkdir(parents=True, exist_ok=True)
    name = project["name"]
    summary = base_summary(project)
    friction: list[str] = summary["friction"]
    env = child_env()
    coverage = work_dir / name / "coverage.json"
    report_path = out_dir / f"{name}.report.json"
    try:
        if not binary.is_file():
            raise EvaluationError(f"binary does not exist: {binary}")
        repo = checkout(project, work_dir, out_dir, env, friction)
        add_coverlet(project, repo, out_dir, env, friction)
        for excluded in project.get("excluded", []):
            friction.append(f"excluded {excluded['project']}: {excluded['reason']}")
        if project.get("notes"):
            friction.append(f"note: {project['notes']}")
        friction.append("collect and scan used --skip-unparsed; skipped files are listed, not scored")

        coverage.parent.mkdir(parents=True, exist_ok=True)
        if coverage.exists():
            coverage.unlink()
        collect, rss = run_timed(
            collect_argv(project, str(binary), repo, coverage),
            cwd=repo,
            log_path=out_dir / f"{name}.collect.log",
            time_log=out_dir / f"{name}.collect.time",
            timeout=COLLECT_TIMEOUT,
            env=env,
            friction=friction,
        )
        summary["collect_seconds"] = collect["seconds"]
        summary["collect_max_rss_kb"] = rss
        summary["collect_exit_code"] = collect["exit_code"]
        if collect["timed_out"]:
            raise EvaluationError(f"collect timed out after {COLLECT_TIMEOUT}s")
        if collect["exit_code"] != 0:
            raise EvaluationError(
                f"collect failed with exit {collect['exit_code']}: "
                f"{tail(out_dir / f'{name}.collect.log')}"
            )
        if not coverage.is_file():
            raise EvaluationError("collect exited 0 but produced no coverage file")

        report_path.unlink(missing_ok=True)
        scan, rss = run_timed(
            scan_argv(project, str(binary), repo, coverage, report_path),
            cwd=repo,
            log_path=out_dir / f"{name}.scan.log",
            time_log=out_dir / f"{name}.scan.time",
            timeout=SCAN_TIMEOUT,
            env=env,
            friction=friction,
        )
        summary["scan_seconds"] = scan["seconds"]
        summary["scan_max_rss_kb"] = rss
        summary["scan_exit_code"] = scan["exit_code"]
        if scan["timed_out"]:
            raise EvaluationError(f"scan timed out after {SCAN_TIMEOUT}s")
        if not report_path.is_file():
            raise EvaluationError(
                f"scan produced no report (exit {scan['exit_code']}): "
                f"{tail(out_dir / f'{name}.scan.log')}"
            )
        report = json.loads(report_path.read_text(encoding="utf-8"))
        summary.update(summarize_report(report))
        if scan["exit_code"] not in (0, 1):
            raise EvaluationError(
                f"scan failed with exit {scan['exit_code']}: "
                f"{tail(out_dir / f'{name}.scan.log')}"
            )
        if scan["exit_code"] == 1:
            friction.append("scan gate failed (exit 1); the report is still captured")
        summary["status"] = "ok"
        summary["error"] = None
    except Exception as error:  # noqa: BLE001 - a failed run must still summarize
        summary["status"] = "error"
        summary["error"] = str(error)
        if not report_path.exists():
            write_json(report_path, {"error": str(error)})
    finally:
        summary["friction"] = friction
        write_json(out_dir / f"{name}.summary.json", summary)
    return summary


def _cell(value: object) -> str:
    """Render one Markdown table cell."""
    if value is None:
        return "n/a"
    return str(value).replace("\\", "\\\\").replace("|", "\\|").replace("\n", " ")


def render_table(summaries: list[dict]) -> str:
    """Render summaries as a Markdown table."""
    columns = [
        "Project",
        "Kind",
        "Commit",
        "Files",
        "Callables",
        "Covered %",
        "Max CRAP",
        "CRAP > 30",
        "Collect s",
        "Collect RSS MB",
        "Scan s",
        "Scan RSS MB",
        "Skipped",
        "Status",
    ]
    lines = ["| " + " | ".join(columns) + " |", "|" + "|".join(["---"] * len(columns)) + "|"]
    for item in sorted(summaries, key=lambda entry: str(entry.get("name", ""))):
        status = "ok" if item.get("status") == "ok" else f"error: {item.get('error') or 'unknown'}"
        rss_collect = item.get("collect_max_rss_kb")
        rss_scan = item.get("scan_max_rss_kb")
        row = [
            item.get("name"),
            item.get("kind"),
            str(item.get("commit", ""))[:10],
            item.get("files_scanned"),
            item.get("callables"),
            f"{item['percent_with_coverage']:.1f}" if item.get("percent_with_coverage") is not None else None,
            item.get("max_crap"),
            item.get("crap_over_30"),
            f"{item['collect_seconds']:.1f}" if item.get("collect_seconds") is not None else None,
            f"{rss_collect / 1024:.1f}" if isinstance(rss_collect, int) else None,
            f"{item['scan_seconds']:.1f}" if item.get("scan_seconds") is not None else None,
            f"{rss_scan / 1024:.1f}" if isinstance(rss_scan, int) else None,
            item.get("skipped_files_count"),
            status,
        ]
        lines.append("| " + " | ".join(_cell(cell) for cell in row) + " |")
    return "\n".join(lines)


def load_summaries(out_dir: Path) -> list[dict]:
    """Read every summary in a directory, ignoring malformed files."""
    summaries = []
    for path in sorted(Path(out_dir).glob("*.summary.json")):
        try:
            data = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            continue
        if isinstance(data, dict):
            summaries.append(data)
    return summaries


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    subparsers = parser.add_subparsers(dest="command", required=True)

    run_parser = subparsers.add_parser("run", help="evaluate one pinned project")
    run_parser.add_argument("--project", required=True, help="name from evaluation/projects.json")
    run_parser.add_argument("--binary", required=True, type=Path, help="path to the dotnet-crap executable")
    run_parser.add_argument("--work", required=True, type=Path, help="checkout and coverage working directory")
    run_parser.add_argument("--out", required=True, type=Path, help="summary and log output directory")

    table_parser = subparsers.add_parser("table", help="render a Markdown table of summaries")
    table_parser.add_argument("--out", required=True, type=Path, help="directory of *.summary.json files")

    args = parser.parse_args(argv)
    if args.command == "run":
        try:
            project = find_project(args.project)
        except (EvaluationError, OSError, ValueError) as error:
            print(f"error: {error}", file=sys.stderr)
            return 2
        summary = run_project(project, args.binary, args.work, args.out)
        print(
            f"{summary['name']}: {summary['status']} "
            f"(collect {summary['collect_exit_code']}, scan {summary['scan_exit_code']})"
        )
        if summary["status"] != "ok":
            print(f"error: {summary['error']}", file=sys.stderr)
            return 1
        return 0
    summaries = load_summaries(args.out)
    print(render_table(summaries))
    if not summaries:
        print("\n_No summaries found._")
    return 0


if __name__ == "__main__":
    sys.exit(main())
