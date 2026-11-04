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
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=Path("target/release/dotnet-crap"))
    parser.add_argument("--files", type=int, default=500)
    parser.add_argument("--runs", type=int, default=7)
    parser.add_argument("--coverage", action="store_true")
    parser.add_argument("--output", type=Path)
    parser.add_argument("--max-median-ms", type=float)
    args = parser.parse_args()
    if args.files < 1 or args.runs < 3:
        parser.error("use at least one file and three measured runs")
    if args.max_median_ms is not None and (
        not math.isfinite(args.max_median_ms) or args.max_median_ms <= 0
    ):
        parser.error("--max-median-ms must be finite and positive")
    binary = args.binary.resolve(strict=True)
    samples = []
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
        command = [str(binary), "--root", str(root), "--threshold", "100", "--format", "json"]
        if args.coverage:
            coverage = root / "coverage.json"
            coverage.write_text(json.dumps({"version": 1, "files": coverage_files}))
            command += ["--coverage", str(coverage), "--require-coverage"]
        for run in range(args.runs + 1):
            started = time.perf_counter()
            result = subprocess.run(command, capture_output=True, check=True)
            elapsed = (time.perf_counter() - started) * 1000
            report = json.loads(result.stdout)
            if report["files_scanned"] != args.files or len(report["functions"]) != args.files * 2:
                raise RuntimeError("benchmark scan did not process the complete corpus")
            if run:  # Discard one warmup; keep every measured sample.
                samples.append(elapsed)
    measurement = {
        "version": 1,
        "platform": platform.platform(),
        "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
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
        args.output.write_text(encoded)
    print(encoded, end="")
    return int(args.max_median_ms is not None and measurement["median_ms"] > args.max_median_ms)


if __name__ == "__main__":
    raise SystemExit(main())
