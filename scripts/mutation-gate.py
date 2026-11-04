#!/usr/bin/env python3
"""Run the complete production mutation inventory and enforce its score."""
import json
import pathlib
import subprocess
import sys

command = ["cargo", "mutants", "--jobs", "2", "--jobserver-tasks", "2",
           "--timeout", "120", "--build-timeout", "600", "-C", "--locked"]
result = subprocess.run(command, check=False)
out = pathlib.Path("mutants.out")
try:
    total = len(json.loads((out / "mutants.json").read_text()))
    counts = {name: len((out / (name + ".txt")).read_text().splitlines())
              for name in ("caught", "missed", "timeout", "unviable")}
except (OSError, ValueError) as error:
    sys.exit(f"Incomplete mutation output: {error}")
print(f"total={total} " + " ".join(f"{k}={v}" for k, v in counts.items()), flush=True)
if result.returncode not in (0, 2, 3) or sum(counts.values()) != total or not total:
    sys.exit("Mutation run did not complete successfully")
viable = counts["caught"] + counts["missed"] + counts["timeout"]
if not viable:
    sys.exit("No viable mutants")
print(f"score={100 * counts['caught'] / viable:.2f}% (caught / (caught + missed + timeout))")
if 10 * counts["caught"] <= 9 * viable:
    sys.exit("Mutation score must exceed 90%")
