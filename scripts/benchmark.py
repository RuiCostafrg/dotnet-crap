#!/usr/bin/env python3
"""Measure the release CLI end to end against deterministic C# corpora.

Every measured invocation is validated against independently derived expected
scores. Each sample records wall time and the child's own peak resident set size
(via wait4), and the output records the provenance needed to reproduce it.
"""
import argparse
import bisect
import datetime
import hashlib
import json
import math
import os
import platform
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import bench_workloads  # noqa: E402  (sibling module, stdlib only)

MODES = ("scan", "scan-coverage", "collect")
COPY_SCRIPT = """#!/bin/sh
# Tooling-only trusted command: replays pre-generated Coverlet-shaped Cobertura
# reports into the collector's private results directory. It never runs tests.
set -eu
cp -R "$1"/. "$2"/
"""


def validate_report(report, files, has_coverage):
    """Reject incomplete corpus identities or scores that differ from the known C# fixture."""
    expected = {(f"C{i:06}.cs", name) for i in range(files) for name in ("F", "G")}
    if not isinstance(report, dict) or not isinstance(report.get("functions"), list):
        raise ValueError("benchmark report must contain a functions list")
    functions = report["functions"]
    if not all(isinstance(item, dict) for item in functions):
        raise ValueError("benchmark report contains invalid callable records")
    actual = {(item.get("file"), item.get("name")) for item in functions}
    if (report.get("files_scanned") != files or len(functions) != files * 2
            or actual != expected or len(actual) != len(functions)):
        raise ValueError("benchmark scan did not process the complete corpus")
    for item in functions:
        complexity = 3 if item["name"] == "F" else 2
        ratio = (0.5 if item["name"] == "F" else 1.0) if has_coverage else None
        score = (4.125 if item["name"] == "F" else 2.0) if has_coverage else complexity ** 2 + complexity
        if (item.get("complexity") != complexity or item.get("coverage") != ratio
                or not isinstance(item.get("crap"), (float, int))
                or not math.isclose(item["crap"], score, rel_tol=0, abs_tol=1e-12)):
            raise ValueError("benchmark callable score differs from the generated corpus")
    if report.get("failed") is not False:
        raise ValueError("benchmark must pass its threshold gate")


def validate_collected(evidence, workload):
    """Require collected native evidence to equal the generator's independent line map."""
    if evidence != workload.native_coverage:
        raise ValueError("collected coverage differs from the generated Cobertura evidence")


def write_output(path, binary, encoded):
    """Atomically publish measurement JSON while preserving symlinks and executable inputs."""
    # Destinations outside the project are intentional. Never follow a final
    # symlink or overwrite the measured executable (including a hardlink alias).
    path = path.absolute()
    if path.is_symlink() or path.resolve() == binary or (path.exists() and path.samefile(binary)):
        raise ValueError("benchmark output aliases a symlink or executable")
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", dir=path.parent, delete=False) as stream:
            temporary = Path(stream.name)
            stream.write(encoded)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


# Measuring spawner, run as a fresh `python -I -S` interpreter. On Linux, exec
# folds the spawning address space's RSS high-water mark into the new process's
# ru_maxrss, so spawning the CLI directly from this harness (which holds the
# generated corpus in memory) would floor every sample at the harness's RSS. The
# small spawner lowers that floor; `rss_floor_kib` records it. The spawner puts
# the CLI in its own process group, kills the whole group on timeout, and reports
# wall nanoseconds, the CLI's own ru_maxrss, its exit code and a timeout flag.
SPAWNER = """
import os, signal, sys, time
timeout, fd, argv = float(sys.argv[1]), int(sys.argv[2]), sys.argv[3:]
os.set_inheritable(fd, False)
started = time.perf_counter_ns()
pid = os.fork()
if pid == 0:
    try:
        os.setpgid(0, 0)
        os.execv(argv[0], argv)
    finally:
        os._exit(127)
expired = []
def expire(signum, frame):
    expired.append(True)
    try:
        os.killpg(pid, signal.SIGKILL)
    except ProcessLookupError:  # the child has not created its group yet
        os.kill(pid, signal.SIGKILL)
signal.signal(signal.SIGALRM, expire)
signal.setitimer(signal.ITIMER_REAL, timeout)
_, status, usage = os.wait4(pid, 0)
elapsed = time.perf_counter_ns() - started
signal.setitimer(signal.ITIMER_REAL, 0)
code = os.waitstatus_to_exitcode(status)
os.write(fd, f"{elapsed} {usage.ru_maxrss} {code} {int(bool(expired))}".encode())
"""


def max_rss_kib(maxrss):
    """Normalize ru_maxrss: Linux reports KiB, macOS reports bytes."""
    return maxrss // 1024 if sys.platform == "darwin" else maxrss


def run_measured(command, timeout):
    """Run argv without a shell; return (elapsed ms, CLI peak RSS KiB, stdout bytes).

    argv[0] must be an absolute executable path.
    """
    read_end, write_end = os.pipe()
    try:
        with tempfile.TemporaryFile() as stdout, tempfile.TemporaryFile() as stderr:
            # Audited: --binary intentionally selects a local executable. argv
            # is passed without a shell; quoting elements would corrupt paths.
            spawner = [sys.executable, "-I", "-S", "-c", SPAWNER, repr(float(timeout)), str(write_end), *command]
            subprocess.run(spawner, stdout=stdout, stderr=stderr, pass_fds=(write_end,), shell=False,  # nosemgrep: python.lang.security.audit.dangerous-subprocess-use-audit, python.lang.security.audit.dangerous-subprocess-use-tainted-env-args
                           check=True, timeout=timeout + 60)
            os.close(write_end)
            write_end = None
            with os.fdopen(read_end, "rb") as result:
                read_end = None
                fields = result.read().split()
            stdout.seek(0)
            stderr.seek(0)
            output, errors = stdout.read(), stderr.read()
    finally:
        for fd in (read_end, write_end):
            if fd is not None:
                os.close(fd)
    elapsed_ns, maxrss, code, expired = (int(field) for field in fields)
    if expired:
        # Audited: constructs the timeout exception for reporting; nothing runs here.
        raise subprocess.TimeoutExpired(command, timeout)  # nosemgrep: python.lang.security.audit.dangerous-subprocess-use-audit
    if code != 0:
        raise subprocess.CalledProcessError(code, command, output, errors)
    return elapsed_ns / 1e6, max_rss_kib(maxrss), output


def rss_floor_kib():
    """Peak RSS reported for a trivial program through the same spawner."""
    true = shutil.which("true") or "/usr/bin/true"
    return statistics.median(run_measured([str(Path(true).resolve())], 30)[1] for _ in range(3))


def command_text(argv):
    """Return trimmed stdout of a provenance probe, or None if unavailable."""
    try:
        # Audited: fixed provenance probes (git, sysctl, and the --rustc executable
        # the user names) passed as argv without a shell; quoting would corrupt them.
        return subprocess.run(argv, capture_output=True, text=True, check=True, timeout=30, shell=False).stdout.strip()  # nosemgrep: python.lang.security.audit.dangerous-subprocess-use-audit, python.lang.security.audit.dangerous-subprocess-use-tainted-env-args
    except (OSError, subprocess.SubprocessError):
        return None


def cpu_model():
    """Best-effort CPU description for Linux and macOS runners."""
    if sys.platform == "darwin":
        return command_text(["sysctl", "-n", "machdep.cpu.brand_string"])
    try:
        for line in Path("/proc/cpuinfo").read_text().splitlines():
            key, _, value = line.partition(":")
            if key.strip() in ("model name", "Hardware", "Model"):
                return value.strip()
    except OSError:
        pass
    return platform.processor() or None


def provenance(binary_digest, rustc):
    """Describe the runner, toolchain and source revision that produced a measurement."""
    root = Path(__file__).resolve().parents[1]
    commit = command_text(["git", "-C", str(root), "rev-parse", "HEAD"])
    status = command_text(["git", "-C", str(root), "status", "--porcelain", "--untracked-files=no"])
    return {
        "utc": datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="seconds"),
        "platform": platform.platform(),
        "machine": platform.machine(),
        "cpu": cpu_model(),
        "cpu_count": os.cpu_count(),
        "python": platform.python_version(),
        "rustc": command_text([rustc, "-vV"]),
        "source_commit": commit,
        "source_dirty": None if status is None else bool(status),
        "binary_sha256": binary_digest,
        "load_average": list(os.getloadavg()) if hasattr(os, "getloadavg") else None,
    }


def parse_args(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--binary", type=Path, default=Path("target/release/dotnet-crap.exe" if os.name == "nt" else "target/release/dotnet-crap"))
    parser.add_argument("--workload", choices=bench_workloads.WORKLOADS, default="small-files")
    parser.add_argument("--scale", type=int, help="workload size (default: the workload's DEFAULT_SCALE)")
    parser.add_argument("--files", type=int, help="small-files only: alias for --scale (default 500)")
    parser.add_argument("--mode", choices=MODES, help="default: scan, or scan-coverage with --coverage")
    parser.add_argument("--coverage", action="store_true", help="shorthand for --mode scan-coverage")
    parser.add_argument("--runs", type=int, default=7)
    parser.add_argument("--warmup", type=int, default=1)
    parser.add_argument("--baseline", type=Path,
                        help="second executable measured on the same corpus in alternating order; "
                             "outputs must be byte-identical")
    parser.add_argument("--output", type=Path)
    parser.add_argument("--rustc", default="rustc", help="compiler whose -vV is recorded (default: rustc)")
    parser.add_argument("--max-median-ms", type=float)
    parser.add_argument("--timeout", type=float, default=120.0,
                        help="per-invocation timeout in seconds (default: 120)")
    args = parser.parse_args(argv)
    if args.files is not None:
        if args.workload != "small-files":
            parser.error("--files applies only to the small-files workload")
        if args.scale is not None and args.scale != args.files:
            parser.error("--files and --scale disagree")
        args.scale = args.files
    if args.coverage:
        if args.mode not in (None, "scan-coverage"):
            parser.error("--coverage conflicts with --mode")
        args.mode = "scan-coverage"
    args.mode = args.mode or "scan"
    if args.scale is None:
        args.scale = bench_workloads.DEFAULT_SCALE[args.workload]
    if args.scale < 1 or args.runs < 3 or args.warmup < 0:
        parser.error("use a positive scale, at least three measured runs and a nonnegative warmup")
    if args.max_median_ms is not None and (
        not math.isfinite(args.max_median_ms) or args.max_median_ms <= 0
    ):
        parser.error("--max-median-ms must be finite and positive")
    if not math.isfinite(args.timeout) or args.timeout <= 0:
        parser.error("--timeout must be finite and positive")
    if args.baseline is not None and args.runs % 2:
        parser.error("--baseline needs an even --runs so ABBA order is balanced")
    if os.name == "nt" or not hasattr(os, "wait4"):
        parser.error("the benchmark harness supports Linux and macOS only")
    return parser, args


def span_point_visits(workload):
    """Count coverage points inside every callable's full line span.

    This is a deterministic corpus property: the number of per-point visits made
    by a scorer that walks each callable's whole span (before nested exclusion).
    """
    lines = {name: sorted(int(line) for line in data["lines"])
             for name, data in workload.native_coverage["files"].items()}
    visits = 0
    for item in workload.expected:
        points = lines.get(item["file"], [])
        visits += bisect.bisect_right(points, item["end_line"]) - bisect.bisect_left(points, item["line"])
    return visits


def summarize(samples, rss):
    """Return every sample plus order statistics."""
    return {
        "samples_ms": samples,
        "peak_rss_kib": rss,
        "median_ms": statistics.median(samples),
        "min_ms": min(samples),
        "max_ms": max(samples),
        "median_peak_rss_kib": statistics.median(rss),
        "max_peak_rss_kib": max(rss),
    }


def main(argv=None):
    """Generate a corpus, measure private executable copies, and enforce an optional median budget."""
    parser, args = parse_args(argv)
    binary = args.binary.resolve(strict=True)
    baseline = args.baseline.resolve(strict=True) if args.baseline else None
    output = args.output.absolute() if args.output else None
    if output and output.is_symlink():
        parser.error("--output cannot be a symbolic link")
    for executable in filter(None, (binary, baseline)):
        if output and (output.resolve() == executable or (output.exists() and output.samefile(executable))):
            parser.error("--output cannot alias a benchmark binary")
    labels = ("candidate", "baseline") if baseline else ("candidate",)
    samples = {label: [] for label in labels}
    rss = {label: [] for label in labels}
    with tempfile.TemporaryDirectory(prefix="dotnet-crap-bench-") as directory:
        # Canonical paths: Coverlet-shaped reports embed the absolute source root
        # (macOS temporary directories are reached through a /var symlink).
        base = Path(directory).resolve()
        root = base / "corpus"
        root.mkdir()
        workload = bench_workloads.generate(args.workload, root, args.scale)
        executables, digests = {}, {}
        for label, source in zip(labels, (binary, baseline)):
            private = base / label / (source.name if source.suffix.lower() == ".exe" else "dotnet-crap")
            private.parent.mkdir()
            shutil.copy2(source, private)
            executables[label] = private
            digests[label] = hashlib.sha256(private.read_bytes()).hexdigest()
        threshold = "1000000000000"
        if args.mode == "collect":
            reports = base / "reports"
            for relative, text in workload.cobertura_reports:
                target = reports / relative
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_text(text, encoding="utf-8")
            script = base / "replay.sh"
            script.write_text(COPY_SCRIPT)
        elif args.mode == "scan-coverage":
            coverage = base / "coverage.json"
            coverage.write_text(json.dumps(workload.native_coverage))

        def invoke(label, run):
            """Measure one validated invocation and return its output bytes."""
            executable = str(executables[label])
            if args.mode == "collect":
                evidence = base / f"native-{label}-{run}.json"
                command = [executable, "collect", "--root", str(root), "--output", str(evidence),
                           "--expected-reports", str(len(workload.cobertura_reports)),
                           "--", "/bin/sh", str(script), str(reports), "{results}"]
            else:
                command = [executable, "--root", str(root), "--threshold", threshold, "--format", "json"]
                if args.mode == "scan-coverage":
                    command += ["--coverage", str(coverage), "--require-coverage"]
            elapsed, peak, stdout = run_measured(command, args.timeout)
            if args.mode == "collect":
                stdout = evidence.read_bytes()
                evidence.unlink()
                validate_collected(json.loads(stdout), workload)
            else:
                bench_workloads.validate(json.loads(stdout), workload, args.mode == "scan-coverage")
            if run >= args.warmup:  # Discard warmups; keep every measured sample.
                samples[label].append(elapsed)
                rss[label].append(peak)
            return stdout

        floor = rss_floor_kib()
        for run in range(args.warmup + args.runs):
            # Alternate ABBA order so drift and cache effects do not favor one binary.
            order = labels if (run - args.warmup) % 2 == 0 else labels[::-1]
            outputs = {label: invoke(label, run) for label in order}
            if baseline and outputs["candidate"] != outputs["baseline"]:
                raise ValueError("candidate and baseline outputs are not byte-identical")
    measurement = {
        "version": 2,
        "provenance": provenance(digests["candidate"], args.rustc),
        "workload": workload.name,
        "scale": workload.scale,
        "mode": args.mode,
        "corpus_sha256": workload.corpus_sha256,
        "files": workload.files,
        "callables": len(workload.expected),
        "coverage_points": sum(len(f["lines"]) for f in workload.native_coverage["files"].values()),
        "span_point_visits": span_point_visits(workload),
        "cobertura_reports": len(workload.cobertura_reports),
        "source_bytes": workload.source_bytes,
        "warmup": args.warmup,
        "runs": args.runs,
        "rss_floor_kib": floor,
        **summarize(samples["candidate"], rss["candidate"]),
    }
    if baseline:
        measurement["baseline"] = {
            "binary_sha256": digests["baseline"],
            "outputs_byte_identical": True,
            **summarize(samples["baseline"], rss["baseline"]),
        }
    encoded = json.dumps(measurement, indent=2) + "\n"
    if args.output:
        write_output(args.output, binary, encoded)
    print(encoded, end="")
    return int(args.max_median_ms is not None and measurement["median_ms"] > args.max_median_ms)


if __name__ == "__main__":
    raise SystemExit(main())
