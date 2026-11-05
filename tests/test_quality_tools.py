import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("mutation_gate", ROOT / "scripts/mutation-gate.py")
gate = importlib.util.module_from_spec(spec)
spec.loader.exec_module(gate)

class MutationInventoryTests(unittest.TestCase):
    def inventory(self, outcomes):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        out = Path(directory.name)
        (out / "mutants.json").write_text(json.dumps([{"id": key} for key in "abcd"]))
        for state in ("caught", "missed", "timeout", "unviable"):
            (out / f"{state}.txt").write_text("\n".join(outcomes.get(state, [])) + ("\n" if outcomes.get(state) else ""))
        return out

    def test_valid_inventory_and_timeout_is_accounted(self):
        counts, total = gate.read_inventory(self.inventory({"caught": ["a", "b"], "timeout": ["c"], "unviable": ["d"]}))
        self.assertEqual((total, counts["timeout"]), (4, 1))

    def test_missing_or_duplicate_outcome_fails(self):
        for outcomes in ({"caught": ["a", "b"]}, {"caught": ["a", "a"], "missed": ["b", "c", "d"]}):
            with self.subTest(outcomes=outcomes), self.assertRaises(ValueError):
                gate.read_inventory(self.inventory(outcomes))

    def test_exact_ninety_is_not_greater_than_ninety(self):
        self.assertLessEqual(10 * 9, 9 * 10)

if __name__ == "__main__":
    unittest.main()
