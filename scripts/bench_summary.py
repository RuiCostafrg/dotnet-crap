#!/usr/bin/env python3
"""Render benchmark.py measurements as the Markdown summary of the Benchmark workflow.

    bench_summary.py --profile dist --benchmarks DIR [--repos DIR]

Every row compares the baseline and the head measured by one invocation of
benchmark.py on one runner. A ratio is a claim only when it is marked
``separated``: every head sample beat (or lost to) every baseline sample, the
rule PERFORMANCE_PLAN decision 7 and docs/BENCHMARKS.md set. Rows without the
mark are observations. Nothing here compares rows from different runners.
"""
import argparse
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import bench_workloads  # noqa: E402  (sibling module, stdlib only)

MODE_ORDER = ("scan", "scan-coverage", "collect")


def load(directory):
    """Parse every measurement JSON in ``directory`` (none when it is absent or empty)."""
    if directory is None or not Path(directory).is_dir():
        return []
    return [json.loads(path.read_text()) for path in sorted(Path(directory).glob("*.json"))]


def cell(text):
    """Keep free text (an error message) from breaking a Markdown table."""
    return str(text).replace("|", "\\|").replace("\n", " ")


def ratio(entry, metric):
    """``0.743 separated, lower`` for a separated difference, plain ``1.021`` otherwise."""
    comparison = entry["comparison"][metric]
    text = f"{comparison['median_ratio']:.3f}"
    return f"{text} separated, {comparison['separated']}" if comparison["separated"] else text


def table(header, rows):
    lines = ["| " + " | ".join(header) + " |", "|" + "---|" * len(header)]
    lines += ["| " + " | ".join(row) + " |" for row in rows]
    return "\n".join(lines) + "\n"


def runner_line(measurements):
    """Describe the runner once; every measurement in one job has the same one."""
    if not measurements:
        return ""
    provenance = measurements[0]["provenance"]
    usable = provenance.get("usable_cpus")
    counted = provenance.get("cpu_count")
    cpus = f"{usable} usable CPUs" + (f" (os.cpu_count {counted})" if counted != usable else "")
    return f"Runner: {provenance.get('platform')}, {provenance.get('cpu') or 'unknown CPU'}, {cpus}."


def synthetic_rows(measurements):
    with_args = any(entry["args"] for r in measurements for entry in r["candidates"])
    order = {name: index for index, name in enumerate(bench_workloads.WORKLOADS)}
    modes = {name: index for index, name in enumerate(MODE_ORDER)}
    rows = []
    for r in sorted(measurements, key=lambda r: (order.get(r["workload"], len(order)),
                                                 modes.get(r["mode"], len(modes)))):
        base = r["baseline"]
        for entry in r["candidates"]:
            row = [r["workload"], r["mode"]]
            if with_args:
                row.append(cell(entry["label"]))
            row += [f"{base['median_ms']:.1f}", f"{entry['median_ms']:.1f}", ratio(entry, "wall_ms"),
                    f"{base['median_cpu_ms']:.1f}", f"{entry['median_cpu_ms']:.1f}", ratio(entry, "cpu_ms"),
                    f"{base['median_peak_rss_kib']:.0f}", f"{entry['median_peak_rss_kib']:.0f}"]
            rows.append(row)
    header = ["workload", "mode"] + (["head args"] if with_args else []) + [
        "base wall ms", "head wall ms", "wall ratio", "base CPU ms", "head CPU ms", "CPU ratio",
        "base RSS KiB", "head RSS KiB"]
    return header, rows


def scan_status(r):
    """One word for how the real repository scanned, with the reason when it did not."""
    failure = r["repo"]["scan_failure"]
    if failure is None:
        scanned, selected = r["repo"]["files_scanned"], r["files"]
        if scanned != selected:
            return f"ok, but the scanner read {scanned} files and the harness counted {selected}"
        return "ok"
    kind = "syntax failure" if failure["syntax_failure"] else f"failed (exit {failure['exit_code']})"
    return f"{kind}: {failure['message']}"


def repo_rows(measurements):
    rows = []
    for r in sorted(measurements, key=lambda r: r["repo"]["name"]):
        base = r["baseline"]
        for entry in r["candidates"]:
            rows.append([
                cell(r["repo"]["name"]), str(r["files"]), f"{r['source_bytes']:,}",
                f"{base['median_ms']:.1f}", f"{entry['median_ms']:.1f}", ratio(entry, "wall_ms"),
                f"{base['median_cpu_ms']:.1f}", f"{entry['median_cpu_ms']:.1f}", ratio(entry, "cpu_ms"),
                f"{base['median_peak_rss_kib']:.0f}", f"{entry['median_peak_rss_kib']:.0f}", cell(scan_status(r))])
    header = ["repository", "files", "source bytes", "base wall ms", "head wall ms", "wall ratio",
              "base CPU ms", "head CPU ms", "CPU ratio", "base RSS KiB", "head RSS KiB", "scan"]
    return header, rows


def render(synthetic, repos, profile):
    """The whole summary as Markdown."""
    parts = [f"Build profile: `{profile}` for both base and head. {runner_line(synthetic or repos)}".rstrip(),
             "Ratios are head over base (below 1 is faster or smaller). Only a ratio marked `separated` is a claim: "
             "every head sample beat every base sample. Rows from different jobs or runners are not comparable."]
    if synthetic:
        parts.append("### Generated workloads\n\n" + table(*synthetic_rows(synthetic)))
    if repos:
        parts.append("### Real repositories (scan only)\n\n" + table(*repo_rows(repos)))
        if any(r["repo"]["scan_failure"] for r in repos):
            parts.append("A scan failure is a finding about the scanner on that code, not a job failure: base and "
                         "head failed identically. Its timings cover the work up to the failure.")
    return "\n\n".join(parts) + "\n"


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--benchmarks", type=Path, help="directory of generated-workload measurements")
    parser.add_argument("--repos", type=Path, help="directory of real-repository measurements")
    parser.add_argument("--profile", required=True, help="cargo profile both binaries were built with")
    args = parser.parse_args(argv)
    synthetic, repos = load(args.benchmarks), load(args.repos)
    if not synthetic and not repos:
        parser.error("no measurements found")
    print(render(synthetic, repos, args.profile), end="")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
