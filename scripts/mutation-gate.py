#!/usr/bin/env python3
"""Run cargo-mutants in an invocation-private evidence directory."""
import json
import pathlib
import subprocess
import sys
import time


def read_inventory(out):
    inventory = json.loads((out / "mutants.json").read_text())
    if not isinstance(inventory, list) or not inventory:
        raise ValueError("inventory must be a non-empty list")
    ids = []
    for mutant in inventory:
        if not isinstance(mutant, dict) or not isinstance(mutant.get("id"), str) or not mutant["id"].strip():
            raise ValueError("inventory contains a malformed mutant")
        ids.append(mutant["id"])
    if len(set(ids)) != len(ids):
        raise ValueError("inventory contains duplicate mutant identities")
    buckets = {}
    for state in ("caught", "missed", "timeout", "unviable"):
        path = out / f"{state}.txt"
        lines = path.read_text().splitlines()
        if any(not line.strip() for line in lines):
            raise ValueError(f"{state} contains an empty identity")
        if len(set(lines)) != len(lines):
            raise ValueError(f"{state} contains duplicate identities")
        buckets[state] = set(lines)
    all_ids = set(ids)
    seen = set()
    for state, members in buckets.items():
        if members - all_ids:
            raise ValueError(f"{state} contains identities absent from inventory")
        if seen & members:
            raise ValueError("mutant appears in multiple outcome buckets")
        seen |= members
    if seen != all_ids:
        raise ValueError("outcomes do not account for the complete inventory")
    return {key: len(value) for key, value in buckets.items()}, len(ids)


def main():
    out = pathlib.Path("target/mutation-runs") / f"run-{time.time_ns()}-{__import__('os').getpid()}"
    out.mkdir(parents=True, exist_ok=False)
    command = ["cargo", "mutants", "--output", str(out.resolve()), "--jobs", "2", "--jobserver-tasks", "2", "--timeout", "120", "--build-timeout", "600", "-C", "--locked"]
    result = subprocess.run(command, check=False)
    try:
        counts, total = read_inventory(out)
    except (OSError, ValueError, TypeError) as error:
        sys.exit(f"Incomplete mutation output in {out}: {error}")
    print(f"evidence={out} total={total} " + " ".join(f"{k}={v}" for k, v in counts.items()), flush=True)
    if result.returncode not in (0, 2, 3):
        sys.exit(f"Mutation runner failed with status {result.returncode}; evidence retained at {out}")
    viable = counts["caught"] + counts["missed"] + counts["timeout"]
    if not viable:
        sys.exit("No viable mutants")
    score = 100 * counts["caught"] / viable
    print(f"score={score:.2f}% (caught / (caught + missed + timeout))")
    if 10 * counts["caught"] <= 9 * viable:
        sys.exit("Mutation score must exceed 90%")


if __name__ == "__main__":
    main()
