#!/usr/bin/env python3
"""Run cargo-mutants in an invocation-private evidence directory."""
import argparse
import json
import pathlib
import shutil
import subprocess
import sys
import tempfile

STATES = ("caught", "missed", "timeout", "unviable")


def read_inventory(out):
    """Validate the real cargo-mutants inventory against complete, disjoint outcome identities."""
    # cargo-mutants 27.1.0 uses `name`, matching the outcome text files.
    # --output names a parent directory; the files live in its mutants.out child.
    inventory = json.loads((out / "mutants.json").read_text())
    if not isinstance(inventory, list) or not inventory:
        raise ValueError("inventory must be a non-empty list")
    ids = []
    for mutant in inventory:
        if not isinstance(mutant, dict) or not isinstance(mutant.get("name"), str) or not mutant["name"].strip():
            raise ValueError("inventory contains a malformed mutant")
        ids.append(mutant["name"])
    if len(set(ids)) != len(ids):
        raise ValueError("inventory contains duplicate mutant identities")
    buckets = {}
    for state in STATES:
        lines = (out / f"{state}.txt").read_text().splitlines()
        if any(not line.strip() for line in lines) or len(set(lines)) != len(lines):
            raise ValueError(f"{state} contains empty or duplicate identities")
        buckets[state] = set(lines)
    seen = set()
    for state, members in buckets.items():
        if members - set(ids):
            raise ValueError(f"{state} contains identities absent from inventory")
        if seen & members:
            raise ValueError("mutant appears in multiple outcome buckets")
        seen |= members
    if seen != set(ids):
        raise ValueError("outcomes do not account for the complete inventory")
    return {key: len(value) for key, value in buckets.items()}, len(ids)


def check_score(counts):
    """Require strictly greater than ninety percent caught among viable mutants, including timeouts."""
    viable = counts["caught"] + counts["missed"] + counts["timeout"]
    if not viable:
        raise ValueError("No viable mutants")
    score = 100 * counts["caught"] / viable
    if 10 * counts["caught"] <= 9 * viable:
        raise ValueError(f"Mutation score {score:.2f}% must exceed 90%")
    return score


def prune_evidence(root, current, keep, marker_name=".complete"):
    """Prune older evidence in one completed/failed pool, tolerating concurrent removals."""
    try:
        current_key = ((current / marker_name).stat().st_mtime_ns, current.name)
    except FileNotFoundError:
        return  # A newer completed invocation already retired this evidence.
    completed = []
    for directory in root.glob("run-*"):
        if directory == current or directory.is_symlink():
            continue
        marker = directory / marker_name
        try:
            if marker.is_file() and marker.read_text() == "dotnet-crap-mutation-v1":
                key = (marker.stat().st_mtime_ns, directory.name)
                if key < current_key:
                    completed.append((key, directory))
        except FileNotFoundError:
            continue
    completed.sort(reverse=True)
    for _, directory in completed[keep - 1:]:
        try:
            shutil.rmtree(directory)
        except FileNotFoundError:
            pass  # Another invocation pruned it; other deletion errors must surface.


def run_gate(runner=subprocess.run, evidence_root=pathlib.Path("target/mutation-runs"), keep_runs=5):
    """Run cargo-mutants in private storage, validate its evidence, and enforce the quality threshold."""
    if keep_runs < 1:
        raise ValueError("keep_runs must be at least one")
    evidence_root = evidence_root.resolve()
    evidence_root.mkdir(parents=True, exist_ok=True)
    parent = pathlib.Path(tempfile.mkdtemp(prefix="run-", dir=evidence_root))
    out = parent / "mutants.out"
    print(f"evidence={out}", flush=True)
    command = ["cargo", "mutants", "--output", str(parent), "--jobs", "2",
               "--jobserver-tasks", "2", "--timeout", "120", "--build-timeout", "600", "-C", "--locked"]
    finished = False
    validated = False
    try:
        try:
            result = runner(command, check=False)
        except OSError:
            finished = True
            raise
        finished = True
        if result.returncode not in (0, 2, 3):
            raise ValueError(f"Mutation runner failed with status {result.returncode}; evidence retained at {out}")
        counts, total = read_inventory(out)
        validated = True
        print(f"total={total} " + " ".join(f"{k}={v}" for k, v in counts.items()), flush=True)
        score = check_score(counts)
        print(f"score={score:.2f}% (caught / (caught + missed + timeout))")
        return out
    except (OSError, ValueError, TypeError) as error:
        if finished:
            (parent / "failure.txt").write_text(str(error) + "\n")
        raise
    finally:
        # Complete means validated evidence, even if its measured score is low.
        # Baseline/launch/schema failures use an independent diagnostics pool;
        # they must never evict the last validated run. Interrupted runs stay active.
        if finished:
            marker = ".complete" if validated else ".failed"
            (parent / marker).write_text("dotnet-crap-mutation-v1")
            prune_evidence(evidence_root, parent, keep_runs, marker)


def main():
    """Parse retention policy and report process, evidence, or score failures without accepting stale data."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--keep-runs", type=int, default=5,
                        help="completed evidence runs to retain, including this run (default: 5)")
    args = parser.parse_args()
    try:
        run_gate(keep_runs=args.keep_runs)
    except (OSError, ValueError, TypeError) as error:
        sys.exit(f"Mutation gate failed: {error}")


if __name__ == "__main__":
    main()
