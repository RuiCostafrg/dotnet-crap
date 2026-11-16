#!/usr/bin/env python3
"""Run cargo-mutants in an invocation-private evidence directory, or aggregate sharded evidence."""
import argparse
import json
import pathlib
import shutil
import subprocess
import sys
import tempfile

STATES = ("caught", "missed", "timeout", "unviable")
FULL_LIST = "full-mutants.json"


def read_identities(path, label):
    """Validate a cargo-mutants JSON mutant list and return its distinct names."""
    mutants = json.loads(path.read_text())
    if not isinstance(mutants, list) or not mutants:
        raise ValueError(f"{label} must be a non-empty list")
    names = []
    for mutant in mutants:
        if not isinstance(mutant, dict) or not isinstance(mutant.get("name"), str) or not mutant["name"].strip():
            raise ValueError(f"{label} contains a malformed mutant")
        names.append(mutant["name"])
    if len(set(names)) != len(names):
        raise ValueError(f"{label} contains duplicate mutant identities")
    return names


def read_buckets(out):
    """Validate one evidence directory's inventory against complete, disjoint outcome identities."""
    # cargo-mutants 27.1.0 uses `name`, matching the outcome text files.
    # --output names a parent directory; the files live in its mutants.out child.
    ids = read_identities(out / "mutants.json", "inventory")
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
    return buckets, len(ids)


def read_inventory(out):
    """Validate the real cargo-mutants inventory and return its outcome counts and size."""
    buckets, total = read_buckets(out)
    return {key: len(value) for key, value in buckets.items()}, total


def check_score(counts):
    """Require strictly greater than ninety percent caught among viable mutants, including timeouts."""
    viable = counts["caught"] + counts["missed"] + counts["timeout"]
    if not viable:
        raise ValueError("No viable mutants")
    score = 100 * counts["caught"] / viable
    if 10 * counts["caught"] <= 9 * viable:
        raise ValueError(f"Mutation score {score:.2f}% must exceed 90%")
    return score


def aggregate_shards(out_dirs):
    """Validate every shard, prove the shards partition the full inventory, and enforce the score."""
    if not out_dirs:
        raise ValueError("aggregate requires one or more shard evidence directories")
    full = None
    shards = []
    for directory in out_dirs:
        # Each directory is a run's parent: run mode records the full list
        # there, and cargo-mutants writes its outcomes to the mutants.out child.
        parent = pathlib.Path(directory)
        out = parent / "mutants.out"
        listed = read_identities(parent / FULL_LIST, "full inventory")
        if full is None:
            full = listed
        elif set(listed) != set(full):
            raise ValueError("shards disagree on the full inventory")
        buckets, total = read_buckets(out)
        shards.append((out, buckets, total))
    counts = dict.fromkeys(STATES, 0)
    union = set()
    for out, buckets, total in shards:
        print(f"shard={out} total={total} "
              + " ".join(f"{state}={len(members)}" for state, members in buckets.items()), flush=True)
        for state, members in buckets.items():
            counts[state] += len(members)
            overlap = union & members
            if overlap:
                raise ValueError(f"{out}: {len(overlap)} mutant(s) tested by more than one shard, e.g. {sorted(overlap)[0]}")
            union |= members
    missing = sorted(set(full) - union)
    unexpected = sorted(union - set(full))
    if missing or unexpected:
        raise ValueError(
            f"shard outcomes do not cover the full inventory exactly: {len(missing)} missing, "
            f"{len(unexpected)} unexpected (e.g. missing {missing[0] if missing else '-'}, "
            f"unexpected {unexpected[0] if unexpected else '-'})")
    print(f"total={len(full)} " + " ".join(f"{k}={v}" for k, v in counts.items()), flush=True)
    score = check_score(counts)
    print(f"score={score:.2f}% (caught / (caught + missed + timeout))")
    return counts, len(full)


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


def record_full_inventory(runner, parent):
    """Record the complete unsharded inventory so aggregation can prove the shards cover it exactly once."""
    listed = runner(["cargo", "mutants", "--list", "--json"], check=False,
                    capture_output=True, text=True)
    if listed.returncode != 0:
        stderr = (getattr(listed, "stderr", "") or "").strip()
        raise ValueError(f"cargo mutants --list failed with status {listed.returncode}: {stderr}")
    (parent / FULL_LIST).write_text(listed.stdout)


def run_gate(runner=subprocess.run, evidence_root=pathlib.Path("target/mutation-runs"), keep_runs=5, shard=None):
    """Run cargo-mutants in private storage, validate its evidence, and enforce the quality threshold."""
    if keep_runs < 1:
        raise ValueError("keep_runs must be at least one")
    evidence_root = evidence_root.resolve()
    evidence_root.mkdir(parents=True, exist_ok=True)
    parent = pathlib.Path(tempfile.mkdtemp(prefix="run-", dir=evidence_root))
    out = parent / "mutants.out"
    print(f"evidence={out}", flush=True)
    if shard is not None:
        print(f"shard={shard[0]}/{shard[1]}", flush=True)
    command = ["cargo", "mutants", "--output", str(parent), "--jobs", "2",
               "--jobserver-tasks", "2", "--timeout", "120", "--build-timeout", "600", "-C", "--locked"]
    if shard is not None:
        command += ["--shard", f"{shard[0]}/{shard[1]}"]
    finished = False
    validated = False
    try:
        try:
            if shard is not None:
                record_full_inventory(runner, parent)
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
        if shard is None:
            score = check_score(counts)
            print(f"score={score:.2f}% (caught / (caught + missed + timeout))")
        else:
            # A shard's own caught fraction is not the gate's verdict; the union
            # score is enforced by the aggregate mode in the final job.
            print("shard evidence complete; the union score is enforced by aggregate")
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


def shard_value(text):
    """Parse a K/N shard selection in cargo-mutants' form, requiring 0 <= K < N."""
    k_text, sep, n_text = text.partition("/")
    try:
        k, n = int(k_text), int(n_text)
    except ValueError:
        raise argparse.ArgumentTypeError("shard must be integers in K/N form") from None
    if not sep or k < 0 or n < 1 or k >= n:
        raise argparse.ArgumentTypeError("shard must satisfy 0 <= K < N")
    return (k, n)


def main():
    """Parse retention policy and report process, evidence, or score failures without accepting stale data."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--keep-runs", type=int, default=None,
                        help="completed evidence runs to retain, including this run (default: 5; run mode only)")
    parser.add_argument("--shard", type=shard_value, metavar="K/N", default=None,
                        help="run only shard K of N of the inventory (0 <= K < N), record the full inventory, "
                             "and defer the score check to aggregate (run mode only)")
    parser.add_argument("command", nargs="?", default="run", choices=("run", "aggregate"))
    parser.add_argument("evidence", nargs="*", metavar="DIR",
                        help="aggregate mode: one or more shard evidence directories, each a run-*/ parent")
    args = parser.parse_args()
    try:
        if args.command == "aggregate":
            if args.keep_runs is not None or args.shard is not None:
                raise ValueError("--keep-runs and --shard apply only to the run mode")
            aggregate_shards(args.evidence)
        else:
            if args.evidence:
                raise ValueError("run mode takes no evidence directories")
            run_gate(keep_runs=5 if args.keep_runs is None else args.keep_runs, shard=args.shard)
    except (OSError, ValueError, TypeError) as error:
        sys.exit(f"Mutation gate failed: {error}")


if __name__ == "__main__":
    main()
