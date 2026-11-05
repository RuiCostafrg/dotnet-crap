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


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=Path("target/release/dotnet-crap"))
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
    binary_digest = hashlib.sha256(binary.read_bytes()).hexdigest()
    output = args.output.absolute() if args.output else None
    if output and output.is_symlink():
        parser.error("--output cannot be a symbolic link")
    if output and (output == binary or binary in output.parents):
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
            expected = {(f"C{i:06}.cs", name) for i in range(args.files) for name in ("F", "G")}
            functions = report.get("functions")
            actual = {(f.get("file"), f.get("name")) for f in functions or [] if isinstance(f, dict)}
            if (report.get("files_scanned") != args.files or len(functions or []) != args.files * 2
                    or actual != expected or len(actual) != len(functions or [])):
                raise RuntimeError("benchmark scan did not process the complete corpus")
            for item in functions:
                if item.get("complexity") != (2 if item["name"] == "F" else 1):
                    raise RuntimeError("benchmark callable complexity differs from the generated corpus")
                if args.coverage and (item.get("coverage") != 1.0 or not math.isclose(item.get("crap", -1), 1.0)):
                    raise RuntimeError("benchmark coverage score differs from the generated corpus")
                if not args.coverage and item.get("coverage") is not None:
                    raise RuntimeError("unexpected coverage in scan-only benchmark")
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
        args.output.parent.mkdir(parents=True, exist_ok=True)
        temporary = args.output.with_name(args.output.name + f".tmp-{os.getpid()}")
        temporary.write_text(encoded)
        os.replace(temporary, args.output)
    print(encoded, end="")
    return int(args.max_median_ms is not None and measurement["median_ms"] > args.max_median_ms)


if __name__ == "__main__":
    raise SystemExit(main())
