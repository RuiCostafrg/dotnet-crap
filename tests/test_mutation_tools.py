import argparse
import concurrent.futures
import importlib.util
import json
import tempfile
import unittest
from unittest import mock
from pathlib import Path
from types import SimpleNamespace

ROOT = Path(__file__).resolve().parents[1]


def module(name, path):
    """Import a tooling module without executing its command-line entry point."""
    spec = importlib.util.spec_from_file_location(name, ROOT / path)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


gate = module("mutation_gate", "scripts/mutation-gate.py")


def write_inventory(out, names, buckets):
    """Create cargo-mutants-shaped identity and outcome fixtures for independent gate tests."""
    out.mkdir(parents=True, exist_ok=True)
    # Match cargo-mutants 27.1.0's real format, not a made-up `id` field.
    (out / "mutants.json").write_text(json.dumps([{"name": n, "file": "src/main.rs"} for n in names]))
    for state in gate.STATES:
        (out / f"{state}.txt").write_text("".join(n + "\n" for n in buckets.get(state, [])))


def write_shard(root, index, names, buckets, full=None):
    """Create one shard's evidence directory, including the recorded full inventory."""
    out = root / f"run-shard{index}"
    write_inventory(out, names, buckets)
    if full is None:
        full = names
    (out / "full-mutants.json").write_text(json.dumps([{"name": n} for n in full]))
    return out


class MutationTests(unittest.TestCase):
    def test_complete_real_format(self):
        """Accept complete evidence using the name field emitted by cargo-mutants."""
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp)
            names = [f"src/main.rs:{i}:1: replace > with >= in scan" for i in range(4)]
            write_inventory(out, names, {"caught": names[:2], "timeout": names[2:3], "unviable": names[3:]})
            counts, total = gate.read_inventory(out)
            self.assertEqual(total, 4)
            self.assertEqual(counts, dict(caught=2, missed=0, timeout=1, unviable=1))

    def test_incomplete_duplicate_unknown_and_overlapping_outcomes_fail(self):
        """Reject incomplete or ambiguous evidence instead of calculating a misleading score."""
        cases = [
            (["a", "b"], {"caught": ["a"]}),
            (["a", "b"], {"caught": ["a", "a"], "missed": ["b"]}),
            (["a", "b"], {"caught": ["a", "b", "other"]}),
            (["a", "b"], {"caught": ["a", "b"], "timeout": ["b"]}),
            (["a", "a"], {"caught": ["a"]}),
            ([], {}),
        ]
        for names, buckets in cases:
            with self.subTest(names=names, buckets=buckets), tempfile.TemporaryDirectory() as tmp:
                out = Path(tmp)
                write_inventory(out, names, buckets)
                with self.assertRaises(ValueError):
                    gate.read_inventory(out)

    def test_threshold_is_strict_and_timeouts_are_not_kills(self):
        """Exercise exact-threshold rejection, timeout accounting, and all-unviable inventory handling."""
        for counts in [dict(caught=9, missed=1, timeout=0, unviable=0),
                       dict(caught=9, missed=0, timeout=1, unviable=100),
                       dict(caught=0, missed=0, timeout=0, unviable=4)]:
            with self.subTest(counts=counts), self.assertRaises(ValueError):
                gate.check_score(counts)
        self.assertAlmostEqual(gate.check_score(dict(caught=10, missed=1, timeout=0, unviable=100)), 1000 / 11)

    def test_concurrent_invocations_read_only_their_own_evidence(self):
        """Verify simultaneous gate calls consume only their independently reserved output directories."""
        with tempfile.TemporaryDirectory() as tmp:
            def runner(command, **kwargs):
                """Emit realistic output inside the invocation-specific directory supplied to the runner."""
                parent = Path(command[command.index("--output") + 1])
                name = parent.name
                write_inventory(parent / "mutants.out", [name], {"caught": [name]})
                return SimpleNamespace(returncode=0)
            def run(_):
                """Execute one independently reserved test gate invocation."""
                return gate.run_gate(runner=runner, evidence_root=Path(tmp))
            with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
                outputs = list(pool.map(run, range(2)))
            self.assertNotEqual(outputs[0], outputs[1])
            for out in outputs:
                self.assertEqual(gate.read_inventory(out), (dict(caught=1, missed=0, timeout=0, unviable=0), 1))

    def test_failed_runner_never_consumes_stale_evidence(self):
        """Reject a failed subprocess before any previous mutation evidence can be consumed."""
        with tempfile.TemporaryDirectory() as tmp:
            with self.assertRaisesRegex(ValueError, "runner failed"):
                gate.run_gate(runner=lambda *a, **k: SimpleNamespace(returncode=1), evidence_root=Path(tmp))

    def test_retention_preserves_current_and_active_runs(self):
        """Prune completed historical runs without deleting current or active evidence."""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            active = root / "run-active"
            active.mkdir()
            def runner(command, **kwargs):
                """Emit realistic output inside the invocation-specific directory supplied to the runner."""
                parent = Path(command[command.index("--output") + 1])
                write_inventory(parent / "mutants.out", [parent.name], {"caught": [parent.name]})
                return SimpleNamespace(returncode=0)
            for _ in range(4):
                latest = gate.run_gate(runner=runner, evidence_root=root, keep_runs=2)
            self.assertTrue(latest.exists())
            self.assertTrue(active.exists())
            self.assertEqual(len(list(root.glob("run-*/.complete"))), 2)

    def test_failed_and_incomplete_runs_do_not_evict_valid_evidence(self):
        """Keep the last complete run when baseline or schema failures occupy the failed pool."""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            def valid_runner(command, **kwargs):
                """Write a complete real-format inventory for a successful tool invocation."""
                parent = Path(command[command.index("--output") + 1])
                write_inventory(parent / "mutants.out", ["one"], {"caught": ["one"]})
                return SimpleNamespace(returncode=0)
            valid = gate.run_gate(runner=valid_runner, evidence_root=root, keep_runs=1)
            for code in (1, 4):
                with self.assertRaises(ValueError):
                    gate.run_gate(runner=lambda *a, **k: SimpleNamespace(returncode=code), evidence_root=root, keep_runs=1)
                self.assertTrue(valid.exists())
            def incomplete_runner(command, **kwargs):
                """Simulate a nominal process exit whose output does not cover the inventory."""
                parent = Path(command[command.index("--output") + 1])
                write_inventory(parent / "mutants.out", ["one", "two"], {"caught": ["one"]})
                return SimpleNamespace(returncode=0)
            with self.assertRaises(ValueError):
                gate.run_gate(runner=incomplete_runner, evidence_root=root, keep_runs=1)
            self.assertTrue(valid.exists())
            self.assertEqual(len(list(root.glob("run-*/.complete"))), 1)
            self.assertEqual(len(list(root.glob("run-*/.failed"))), 1)
            self.assertEqual(len(list(root.glob("run-*/failure.txt"))), 1)

    def test_low_score_still_produces_validated_evidence(self):
        """Retain an honest complete failing score as evidence rather than a baseline failure."""
        with tempfile.TemporaryDirectory() as tmp:
            def runner(command, **kwargs):
                """Write a complete inventory with a single missed mutant."""
                parent = Path(command[command.index("--output") + 1])
                write_inventory(parent / "mutants.out", ["one"], {"missed": ["one"]})
                return SimpleNamespace(returncode=2)
            with self.assertRaisesRegex(ValueError, "must exceed"):
                gate.run_gate(runner=runner, evidence_root=Path(tmp), keep_runs=1)
            self.assertEqual(len(list(Path(tmp).glob("run-*/.complete"))), 1)
            self.assertEqual(len(list(Path(tmp).glob("run-*/.failed"))), 0)

    def test_concurrent_cleanup_ignores_only_missing_directories(self):
        """Treat another pruner's deletion as success without hiding permission failures."""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            old, current = root / "run-old", root / "run-current"
            for directory in (old, current):
                directory.mkdir()
                (directory / ".complete").write_text("dotnet-crap-mutation-v1")
            __import__('os').utime(old / ".complete", (1, 1))
            with mock.patch.object(gate.shutil, "rmtree", side_effect=FileNotFoundError) as remove:
                gate.prune_evidence(root, current, 1)
                remove.assert_called_once_with(old)
            with mock.patch.object(gate.shutil, "rmtree", side_effect=PermissionError):
                with self.assertRaises(PermissionError):
                    gate.prune_evidence(root, current, 1)


class ShardRunTests(unittest.TestCase):
    def test_shard_selection_parses_cargo_mutants_form(self):
        """Accept the k/N form used by cargo-mutants and reject out-of-range or malformed selections."""
        self.assertEqual(gate.shard_value("0/4"), (0, 4))
        self.assertEqual(gate.shard_value("3/4"), (3, 4))
        for text in ["", "4", "1/4/5", "a/b", "4/4", "2/0", "-1/4", "1/", "/4"]:
            with self.subTest(text=text), self.assertRaises(argparse.ArgumentTypeError):
                gate.shard_value(text)

    def test_shard_run_passes_flag_and_records_full_inventory(self):
        """A sharded run passes --shard k/N to cargo-mutants and records the complete inventory."""
        with tempfile.TemporaryDirectory() as tmp:
            commands = []
            def runner(command, **kwargs):
                """Serve the listing to stdout and emit evidence for the sharded run."""
                commands.append(list(command))
                if "--list" in command:
                    return SimpleNamespace(returncode=0, stderr="",
                                           stdout=json.dumps([{"name": n} for n in ("a", "b", "c")]))
                parent = Path(command[command.index("--output") + 1])
                write_inventory(parent / "mutants.out", ["b"], {"caught": ["b"]})
                return SimpleNamespace(returncode=0)
            out = gate.run_gate(runner=runner, evidence_root=Path(tmp), keep_runs=1, shard=(1, 2))
            self.assertEqual(commands[1][commands[1].index("--shard") + 1], "1/2")
            self.assertEqual(commands[0], ["cargo", "mutants", "--list", "--json"])
            full = json.loads((out.parent / "full-mutants.json").read_text())
            self.assertEqual([mutant["name"] for mutant in full], ["a", "b", "c"])
            self.assertEqual(gate.read_inventory(out), (dict(caught=1, missed=0, timeout=0, unviable=0), 1))

    def test_shard_run_defers_score_to_aggregate(self):
        """A shard validates evidence completeness only; the union score is the aggregate job's verdict."""
        with tempfile.TemporaryDirectory() as tmp:
            def runner(command, **kwargs):
                """Emit a complete but zero-score shard inventory."""
                if "--list" in command:
                    return SimpleNamespace(returncode=0, stderr="", stdout=json.dumps([{"name": "one"}]))
                parent = Path(command[command.index("--output") + 1])
                write_inventory(parent / "mutants.out", ["one"], {"missed": ["one"]})
                return SimpleNamespace(returncode=2)
            out = gate.run_gate(runner=runner, evidence_root=Path(tmp), keep_runs=1, shard=(0, 1))
            self.assertTrue((out.parent / ".complete").exists())
            self.assertEqual(len(list(Path(tmp).glob("run-*/.failed"))), 0)

    def test_failed_listing_blocks_the_run(self):
        """A failed inventory listing aborts before any mutation run starts."""
        with tempfile.TemporaryDirectory() as tmp:
            runs = []
            def runner(command, **kwargs):
                """Record every invocation and report a failing listing."""
                runs.append(list(command))
                if "--list" in command:
                    return SimpleNamespace(returncode=1, stderr="boom", stdout="")
                return SimpleNamespace(returncode=0)
            with self.assertRaisesRegex(ValueError, "--list failed with status 1"):
                gate.run_gate(runner=runner, evidence_root=Path(tmp), keep_runs=1, shard=(0, 2))
            self.assertEqual(len(runs), 1)
            self.assertEqual(len(list(Path(tmp).glob("run-*/*"))), 0)


class AggregateTests(unittest.TestCase):
    def test_aggregate_scores_the_union_with_timeouts_in_denominator(self):
        """Sum shard outcomes, include timeouts in the denominator, and pass above the threshold."""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            full = ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k"]
            first = write_shard(root, 0, full[:9], {"caught": full[:9]}, full=full)
            second = write_shard(root, 1, full[9:], {"caught": ["j"], "timeout": ["k"]}, full=full)
            counts, total = gate.aggregate_shards([str(first), second])
            self.assertEqual(total, 11)
            self.assertEqual(counts, dict(caught=10, missed=0, timeout=1, unviable=0))

    def test_aggregate_rejects_the_exact_90_percent_boundary(self):
        """A union score of exactly ninety percent fails; only strictly more passes."""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            full = ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"]
            first = write_shard(root, 0, full[:9], {"caught": full[:9]}, full=full)
            second = write_shard(root, 1, full[9:], {"missed": ["j"]}, full=full)
            with self.assertRaisesRegex(ValueError, "must exceed"):
                gate.aggregate_shards([first, second])

    def test_aggregate_rejects_duplicate_gap_unexpected_and_disagreeing_shards(self):
        """Fail closed when shards overlap, leave gaps, drift from the full list, or disagree."""
        cases = [
            (lambda root: [write_shard(root, 0, ["a"], {"caught": ["a"]}),
                           write_shard(root, 1, ["a"], {"caught": ["a"]})], "more than one shard"),
            (lambda root: [write_shard(root, 0, ["a"], {"caught": ["a"]}, full=["a", "b", "c"]),
                           write_shard(root, 1, ["c"], {"caught": ["c"]},
                                       full=["a", "b", "c"])], "1 missing"),
            (lambda root: [write_shard(root, 0, ["a", "x"], {"caught": ["a", "x"]},
                                       full=["a", "b"])], "unexpected"),
            (lambda root: [write_shard(root, 0, ["a"], {"caught": ["a"]}, full=["a", "b"]),
                           write_shard(root, 1, ["b"], {"caught": ["b"]}, full=["a"])], "disagree"),
            (lambda root: [write_shard(root, 0, ["a"], {"caught": ["a"]}),
                           root / "run-absent"], "No such file"),
        ]
        for build, message in cases:
            with self.subTest(message=message), tempfile.TemporaryDirectory() as tmp:
                shards = build(Path(tmp))
                with self.assertRaisesRegex((ValueError, OSError), message):
                    gate.aggregate_shards(shards)
        with self.assertRaisesRegex(ValueError, "requires one or more"):
            gate.aggregate_shards([])

    def test_aggregate_rejects_incomplete_shard_evidence(self):
        """Reuse the single-run inventory validation on every shard before scoring."""
        with tempfile.TemporaryDirectory() as tmp:
            out = write_shard(Path(tmp), 0, ["a", "b"], {"caught": ["a"]})
            with self.assertRaisesRegex(ValueError, "do not account"):
                gate.aggregate_shards([out])

    def test_aggregate_requires_recorded_full_inventory(self):
        """Older evidence without the recorded full list cannot be aggregated."""
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp) / "run-plain"
            write_inventory(out, ["a"], {"caught": ["a"]})
            with self.assertRaisesRegex(FileNotFoundError, "full-mutants.json"):
                gate.aggregate_shards([out])

    def test_cli_routes_run_shard_and_aggregate_modes(self):
        """Route default and explicit commands and reject options that do not apply."""
        with mock.patch.object(gate, "run_gate") as run_gate_mock:
            with mock.patch("sys.argv", ["mutation-gate.py"]):
                gate.main()
            run_gate_mock.assert_called_once_with(keep_runs=5, shard=None)
            run_gate_mock.reset_mock()
            with mock.patch("sys.argv", ["mutation-gate.py", "--shard", "1/4", "--keep-runs", "2"]):
                gate.main()
            run_gate_mock.assert_called_once_with(keep_runs=2, shard=(1, 4))
        with mock.patch.object(gate, "aggregate_shards") as aggregate_mock:
            with mock.patch("sys.argv", ["mutation-gate.py", "aggregate", "a", "b"]):
                gate.main()
            aggregate_mock.assert_called_once_with(["a", "b"])
        for argv in [["mutation-gate.py", "--keep-runs", "2", "aggregate", "a"],
                     ["mutation-gate.py", "run", "a"],
                     ["mutation-gate.py", "--shard", "4/4"]]:
            with self.subTest(argv=argv):
                with mock.patch("sys.argv", argv), self.assertRaises(SystemExit):
                    gate.main()


if __name__ == "__main__":
    unittest.main()
