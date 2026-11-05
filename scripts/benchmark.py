#!/usr/bin/env python3
"""Measure the release CLI end to end against a deterministic C# corpus."""
import argparse
import hashlib
import json
import math
import platform
import statistics
import subprocess
import tempfile
import time
import os
import shutil
from pathlib import Path


def validate_report(report, files, has_coverage):
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


def write_output(path, binary, encoded):
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


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=Path("target/release/dotnet-crap.exe" if os.name == "nt" else "target/release/dotnet-crap"))
    parser.add_argument("--files", type=int, default=500)
    parser.add_argument("--runs", type=int, default=7)
    parser.add_argument("--coverage", action="store_true")
    parser.add_argument("--output", type=Path)
    parser.add_argument("--max-median-ms", type=float)
    parser.add_argument("--timeout", type=float, default=120.0,
                        help="per-invocation timeout in seconds (default: 120)")
    args = parser.parse_args()
    if args.files < 1 or args.runs < 3:
        parser.error("use at least one file and three measured runs")
    if args.max_median_ms is not None and (
        not math.isfinite(args.max_median_ms) or args.max_median_ms <= 0
    ):
        parser.error("--max-median-ms must be finite and positive")
    if not math.isfinite(args.timeout) or args.timeout <= 0:
        parser.error("--timeout must be finite and positive")
    binary = args.binary.resolve(strict=True)
    samples = []
    output = args.output.absolute() if args.output else None
    if output and output.is_symlink():
        parser.error("--output cannot be a symbolic link")
    if output and (output.resolve() == binary or (output.exists() and output.samefile(binary))):
        parser.error("--output cannot alias the benchmark binary")
    with tempfile.TemporaryDirectory(prefix="dotnet-crap-bench-") as directory:
        root = Path(directory)
        coverage_files = {}
        source_bytes = 0
        for index in range(args.files):
            source = (
                f"class C{index} {{\n"
                " int F(int x) {\n"
                "  if (x > 0 && x < 10) return x;\n"
                "  return 0;\n"
                " }\n"
                " int G(int x) => x > 0 ? x : -x;\n"
                "}\n"
            ).encode()
            name = f"C{index:06}.cs"
            (root / name).write_bytes(source)
            source_bytes += len(source)
            coverage_files[name] = {
                "sha256": hashlib.sha256(source).hexdigest(),
                "lines": {"3": 1, "4": 0, "6": 1},
            }
        private_binary = root / (binary.name if binary.suffix.lower() == ".exe" else "dotnet-crap")
        shutil.copy2(binary, private_binary)
        binary_digest = hashlib.sha256(private_binary.read_bytes()).hexdigest()
        command = [str(private_binary), "--root", str(root), "--threshold", "100", "--format", "json"]
        if args.coverage:
            coverage = root / "coverage.json"
            coverage.write_text(json.dumps({"version": 1, "files": coverage_files}))
            command += ["--coverage", str(coverage), "--require-coverage"]
        for run in range(args.runs + 1):
            started = time.perf_counter()
            result = subprocess.run(command, capture_output=True, check=True, timeout=args.timeout)
            elapsed = (time.perf_counter() - started) * 1000
            report = json.loads(result.stdout)
            validate_report(report, args.files, args.coverage)
            if run:  # Discard one warmup; keep every measured sample.
                samples.append(elapsed)
    measurement = {
        "version": 1,
        "platform": platform.platform(),
        "binary_sha256": binary_digest,
        "files": args.files,
        "callables": args.files * 2,
        "source_bytes": source_bytes,
        "coverage": args.coverage,
        "runs": args.runs,
        "samples_ms": samples,
        "median_ms": statistics.median(samples),
        "min_ms": min(samples),
        "max_ms": max(samples),
    }
    encoded = json.dumps(measurement, indent=2) + "\n"
    if args.output:
        write_output(args.output, binary, encoded)
    print(encoded, end="")
    return int(args.max_median_ms is not None and measurement["median_ms"] > args.max_median_ms)


if __name__ == "__main__":
    raise SystemExit(main())
