import copy
import hashlib
import importlib.util
import json
import os
import shlex
import subprocess
import tempfile
import unittest
import xml.etree.ElementTree as ET
from pathlib import Path
from unittest import mock

ROOT = Path(__file__).resolve().parents[1]


def module(name, path):
    """Load a tooling script without invoking its command-line entry point."""
    spec = importlib.util.spec_from_file_location(name, ROOT / path)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


bench = module("bench_workloads", "scripts/bench_workloads.py")

SMALL_SCALES = {"small-files": 3, "dense-file": 9, "many-callables": 10,
                "deep-nesting": 4, "solution": 2}
DECOY_MARKERS = ("/bin/", "/obj/", ".g.cs", ".generated.cs", ".Designer.cs")
SMALL_FILE_TEXT = (
    "class C0 {\n"
    " int F(int x) {\n"
    "  if (x > 0 && x < 10) return x;\n"
    "  return 0;\n"
    " }\n"
    " int G(int x) => x > 0 ? x : -x;\n"
    "}\n"
)


def item(file, name, kind, line, end_line, complexity, coverage, crap):
    return {"file": file, "name": name, "kind": kind, "line": line, "end_line": end_line,
            "complexity": complexity, "coverage": coverage, "crap": crap}


def scanner_report(workload, has_coverage):
    """A report exactly as the scanner would emit it for the workload's expectations."""
    functions = []
    for want in workload.expected:
        got = dict(want)
        if not has_coverage:
            got["coverage"] = None
            got["crap"] = float(want["complexity"] ** 2 + want["complexity"])
        functions.append(got)
    complete = has_coverage and all(want["coverage"] is not None for want in workload.expected)
    return {"threshold": 1e12, "files_scanned": workload.files, "coverage_complete": complete,
            "functions": functions, "failed": False}


class ExpectedValueTests(unittest.TestCase):
    """Literal numbers worked out by hand from each layout, not from the generator's code."""

    def assertScores(self, actual, expected):
        """Exact identity and coverage; CRAP compared within floating-point tolerance."""
        self.assertEqual(len(actual), len(expected))
        for got, want in zip(actual, expected):
            for key in ("file", "name", "kind", "line", "end_line", "complexity", "coverage"):
                self.assertEqual(got[key], want[key], key)
            self.assertAlmostEqual(got["crap"], want["crap"], places=12)

    def test_small_files_matches_historical_corpus_and_scores(self):
        with tempfile.TemporaryDirectory() as tmp:
            wl = bench.generate("small-files", Path(tmp) / "root", 2)
            self.assertEqual((Path(tmp) / "root" / "C000000.cs").read_text(), SMALL_FILE_TEXT)
            self.assertEqual(wl.files, 2)
            self.assertEqual(wl.source_bytes, 2 * len(SMALL_FILE_TEXT))
            self.assertEqual(wl.native_coverage["files"]["C000000.cs"]["lines"],
                             {"3": 1, "4": 0, "6": 1})
            self.assertScores(wl.expected, [
                item("C000000.cs", "F", "method_declaration", 2, 5, 3, 0.5, 4.125),
                item("C000000.cs", "G", "method_declaration", 6, 6, 2, 1.0, 2.0),
                item("C000001.cs", "F", "method_declaration", 2, 5, 3, 0.5, 4.125),
                item("C000001.cs", "G", "method_declaration", 6, 6, 2, 1.0, 2.0),
            ])

    def test_dense_file_single_method(self):
        with tempfile.TemporaryDirectory() as tmp:
            wl = bench.generate("dense-file", Path(tmp) / "root", 1)
            # Lines: 2 signature, 4 init (hits 1), 5 if (hits 2), 6 for (hits 0), 7 return (hits 1), 8 close.
            self.assertScores(wl.expected, [
                item("Dense.cs", "M00000", "method_declaration", 2, 8, 4, 0.75, 4.25),
            ])
            self.assertEqual(wl.native_coverage["files"]["Dense.cs"]["lines"],
                             {"4": 1, "5": 2, "6": 0, "7": 1})

    def test_many_callables_constructor_and_hosted_local(self):
        with tempfile.TemporaryDirectory() as tmp:
            one = bench.generate("many-callables", Path(tmp) / "one", 1)
            self.assertScores(one.expected, [
                item("Many.cs", "K0000", "constructor_declaration", 3, 6, 1, 1.0, 1.0),
            ])
            two = bench.generate("many-callables", Path(tmp) / "two", 2)
            # Local function on line 6 has hits 0 (42 % 3), so it scores 1 + 1 = 2.
            self.assertScores(two.expected, [
                item("Many.cs", "K0000", "constructor_declaration", 3, 7, 1, 1.0, 1.0),
                item("Many.cs", "Local", "local_function_statement", 6, 6, 1, 0.0, 2.0),
            ])

    def test_deep_nesting_owns_only_opener_and_closer(self):
        with tempfile.TemporaryDirectory() as tmp, mock.patch.object(bench, "DEEP_FILLER", 3):
            wl = bench.generate("deep-nesting", Path(tmp) / "root", 2)
            # 4,5 openers; 6-8 filler; 9 closer of lambda 2; 10 closer of lambda 1.
            self.assertScores(wl.expected, [
                item("Deep.cs", "Run", "method_declaration", 2, 12, 1, 0.5, 1.125),
                item("Deep.cs", "lambda_expression", "lambda_expression", 4, 10, 1, 1.0, 1.0),
                item("Deep.cs", "lambda_expression", "lambda_expression", 5, 9, 1, 0.6, 1.064),
            ])

    def test_solution_first_project_literals(self):
        with tempfile.TemporaryDirectory() as tmp:
            wl = bench.generate("solution", Path(tmp) / "root", 1)
            self.assertEqual(wl.files, 5)
            self.assertEqual([(e["file"], e["name"]) for e in wl.expected], [
                ("src/Proj001/Core/Model001.cs", "Scale"),
                ("src/Proj001/Core/Model001.cs", "Sum"),
                ("src/Proj001/Services/Calculator001.cs", "Run"),
                ("src/Proj001/Services/Calculator001.cs", "lambda_expression"),
                ("src/Proj001/Services/Formatter001.cs", "Label"),
                ("tests/Proj001.Tests/CalculatorTests001.cs", "Runs"),
                ("tests/Proj001.Tests/CalculatorTests001.cs", "lambda_expression"),
                ("tests/Proj001.Tests/ModelTests001.cs", "ScaleHandlesPositive"),
            ])
            by_name = {(e["file"], e["name"]): e for e in wl.expected}
            sum_item = by_name[("src/Proj001/Core/Model001.cs", "Sum")]
            self.assertEqual((sum_item["line"], sum_item["end_line"], sum_item["complexity"]), (6, 11, 3))
            self.assertAlmostEqual(sum_item["coverage"], 2 / 3, places=15)
            self.assertAlmostEqual(sum_item["crap"], 3 + 1 / 3, places=12)
            label = by_name[("src/Proj001/Services/Formatter001.cs", "Label")]
            self.assertEqual((label["line"], label["end_line"], label["complexity"]), (5, 13, 3))
            self.assertAlmostEqual(label["crap"], 3 + 9 / 64, places=12)
            run = by_name[("src/Proj001/Services/Calculator001.cs", "Run")]
            self.assertEqual((run["line"], run["end_line"], run["complexity"], run["coverage"]), (5, 14, 2, 1.0))
            lam = by_name[("src/Proj001/Services/Calculator001.cs", "lambda_expression")]
            self.assertEqual((lam["line"], lam["complexity"], lam["coverage"], lam["crap"]), (12, 2, 0.0, 6.0))
            runs = by_name[("tests/Proj001.Tests/CalculatorTests001.cs", "Runs")]
            self.assertEqual((runs["complexity"], runs["coverage"]), (2, 0.75))
            self.assertAlmostEqual(runs["crap"], 2 + 4 / 64, places=12)


class ValidateTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls._tmp = tempfile.TemporaryDirectory()
        cls.wl = bench.generate("small-files", Path(cls._tmp.name) / "root", 2)
        cls.dense = bench.generate("dense-file", Path(cls._tmp.name) / "dense", 9)

    @classmethod
    def tearDownClass(cls):
        cls._tmp.cleanup()

    def test_exact_report_passes_with_and_without_coverage(self):
        bench.validate(scanner_report(self.wl, True), self.wl, True)
        bench.validate(scanner_report(self.wl, False), self.wl, False)

    def test_rejects_dropped_item(self):
        report = scanner_report(self.wl, True)
        report["functions"].pop()
        with self.assertRaises(ValueError):
            bench.validate(report, self.wl, True)

    def test_rejects_swapped_order(self):
        report = scanner_report(self.wl, True)
        report["functions"][0], report["functions"][1] = report["functions"][1], report["functions"][0]
        with self.assertRaises(ValueError):
            bench.validate(report, self.wl, True)

    def test_rejects_wrong_complexity(self):
        report = scanner_report(self.wl, True)
        report["functions"][0]["complexity"] = 4
        with self.assertRaises(ValueError):
            bench.validate(report, self.wl, True)

    def test_rejects_wrong_coverage(self):
        report = scanner_report(self.wl, True)
        report["functions"][0]["coverage"] = 1.0
        with self.assertRaises(ValueError):
            bench.validate(report, self.wl, True)

    def test_rejects_absent_coverage_when_present_is_expected(self):
        report = scanner_report(self.wl, True)
        report["functions"][1]["coverage"] = None
        with self.assertRaises(ValueError):
            bench.validate(report, self.wl, True)

    def test_rejects_wrong_crap(self):
        report = scanner_report(self.wl, True)
        report["functions"][0]["crap"] = 4.125 + 1e-9
        with self.assertRaises(ValueError):
            bench.validate(report, self.wl, True)

    def test_rejects_crap_from_coverage_when_absent(self):
        report = scanner_report(self.wl, False)
        report["functions"][0]["crap"] = 4.125
        with self.assertRaises(ValueError):
            bench.validate(report, self.wl, False)

    def test_rejects_wrong_files_scanned(self):
        report = scanner_report(self.wl, True)
        report["files_scanned"] = 3
        with self.assertRaises(ValueError):
            bench.validate(report, self.wl, True)

    def test_rejects_failed_gate_and_wrong_completeness(self):
        report = scanner_report(self.wl, True)
        report["failed"] = True
        with self.assertRaises(ValueError):
            bench.validate(report, self.wl, True)
        report = scanner_report(self.wl, True)
        report["coverage_complete"] = False
        with self.assertRaises(ValueError):
            bench.validate(report, self.wl, True)

    def test_accepts_copy_with_no_mutation(self):
        bench.validate(copy.deepcopy(scanner_report(self.dense, True)), self.dense, True)


class GenerationTests(unittest.TestCase):
    def test_deterministic_across_roots(self):
        for name, scale in SMALL_SCALES.items():
            with self.subTest(workload=name), tempfile.TemporaryDirectory() as first, \
                    tempfile.TemporaryDirectory() as second:
                a = bench.generate(name, Path(first) / "r", scale)
                b = bench.generate(name, Path(second) / "r", scale)
                self.assertEqual(a.corpus_sha256, b.corpus_sha256)
                self.assertEqual(a.native_coverage, b.native_coverage)
                self.assertEqual(a.expected, b.expected)
                self.assertEqual(a.source_bytes, b.source_bytes)
                self.assertEqual(len(a.cobertura_reports), len(b.cobertura_reports))
                for (rel_a, xml_a), (rel_b, xml_b) in zip(a.cobertura_reports, b.cobertura_reports):
                    self.assertEqual(rel_a, rel_b)
                    self.assertEqual(xml_a.replace(str(Path(first) / "r"), "ROOT"),
                                     xml_b.replace(str(Path(second) / "r"), "ROOT"))

    def test_corpus_digest_covers_ignored_files(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp) / "root"
            wl = bench.generate("solution", root, 1)
            pairs = []
            for path in sorted(p for p in root.rglob("*") if p.is_file()):
                pairs.append((path.relative_to(root).as_posix(),
                              hashlib.sha256(path.read_bytes()).hexdigest()))
            digest = hashlib.sha256()
            for rel, sha in sorted(pairs):
                digest.update(f"{rel}\0{sha}\n".encode("utf-8"))
            self.assertEqual(wl.corpus_sha256, digest.hexdigest())
            self.assertTrue(any(rel.endswith(".g.cs") for rel, _ in pairs))

    def test_solution_decoys_are_excluded_from_expectations(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp) / "root"
            wl = bench.generate("solution", root, 2)
            decoys = [
                "src/Proj001/bin/Debug/net8.0/Bin001.cs",
                "src/Proj001/obj/Debug/net8.0/Obj001.cs",
                "src/Proj001/Core/Model001.g.cs",
                "src/Proj001/Services/Formatter001.generated.cs",
                "src/Proj001/Services/Form001.Designer.cs",
                "tests/Proj001.Tests/obj/Gen001.cs",
                "bin/Release/Root.cs",
            ]
            for rel in decoys:
                self.assertTrue((root / rel).is_file(), rel)
            selected = set(wl.native_coverage["files"])
            self.assertEqual(wl.files, len(selected))
            self.assertEqual(wl.files, 10)
            self.assertEqual(len(wl.expected), 16)
            for entry in wl.expected:
                self.assertIn(entry["file"], selected)
                self.assertFalse(any(marker in "/" + entry["file"] for marker in DECOY_MARKERS))
            for rel in selected:
                self.assertFalse(any(marker in "/" + rel for marker in DECOY_MARKERS), rel)

    def test_rejects_bad_arguments(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp) / "root"
            with self.assertRaises(ValueError):
                bench.generate("no-such-workload", root, 1)
            with self.assertRaises(ValueError):
                bench.generate("small-files", root, 0)
            with self.assertRaises(ValueError):
                bench.generate("small-files", root, True)
            bench.generate("small-files", root, 1)
            with self.assertRaises(ValueError):
                bench.generate("small-files", root, 1)

    def test_path_order_is_component_wise_like_the_scanner(self):
        # The scanner keeps selected files in a BTreeSet<PathBuf>, so a/b.cs sorts before a.cs
        # (raw byte order would put a.cs first because '.' < '/').
        self.assertEqual(sorted(["a.cs", "a/b.cs"], key=bench._path_key), ["a/b.cs", "a.cs"])

    def test_default_scales_are_positive(self):
        self.assertEqual(set(bench.DEFAULT_SCALE), set(bench.WORKLOADS))
        self.assertTrue(all(isinstance(v, int) and v >= 1 for v in bench.DEFAULT_SCALE.values()))


class CoberturaTests(unittest.TestCase):
    def test_class_lines_equal_native_coverage_and_source_is_root(self):
        for name, scale in SMALL_SCALES.items():
            with self.subTest(workload=name), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp) / "root"
                wl = bench.generate(name, root, scale)
                union = {}
                root_text = str(root.resolve()) + "/"
                self.assertTrue(wl.cobertura_reports)
                for rel, xml_text in wl.cobertura_reports:
                    self.assertEqual(rel.count("/"), 1)
                    coverage = ET.fromstring(xml_text)
                    self.assertEqual(coverage.get("version"), "1.9")
                    self.assertEqual(coverage.find("sources/source").text, root_text)
                    for cls in coverage.iter("class"):
                        filename = cls.get("filename")
                        lines = cls.find("lines")
                        self.assertIsNotNone(lines)
                        for line in lines.findall("line"):
                            number, hits = line.get("number"), int(line.get("hits"))
                            # Duplicate lines across classes must agree, as the importer requires.
                            current = union.setdefault(filename, {}).setdefault(number, hits)
                            self.assertEqual(current, hits, (filename, number))
                native = wl.native_coverage["files"]
                for filename in native:
                    self.assertEqual(union.get(filename), native[filename]["lines"], filename)
                extra = set(union) - set(native)
                self.assertTrue(all(any(m in "/" + f for m in DECOY_MARKERS) for f in extra), extra)
                if name == "solution":
                    self.assertEqual(len(wl.cobertura_reports), scale)
                    self.assertEqual(len(extra), 6 * scale)
                else:
                    self.assertEqual(len(wl.cobertura_reports), 1)
                    self.assertEqual(extra, set())

    def test_method_lines_and_conditions_are_coverlet_shaped(self):
        with tempfile.TemporaryDirectory() as tmp:
            wl = bench.generate("small-files", Path(tmp) / "root", 1)
            coverage = ET.fromstring(wl.cobertura_reports[0][1])
            line = coverage.find(".//class/lines/line[@number='3']")
            self.assertEqual(line.get("branch"), "True")
            self.assertEqual(line.get("condition-coverage"), "50% (1/2)")
            self.assertEqual(line.find("conditions/condition").get("coverage"), "50%")
            method = coverage.find(".//method[@name='F']")
            self.assertEqual(method.get("complexity"), "3")
            self.assertEqual([m.get("number") for m in method.findall("lines/line")], ["3", "4"])


@unittest.skipUnless(os.environ.get("DOTNET_CRAP_BENCH_BINARY"),
                     "set DOTNET_CRAP_BENCH_BINARY to a built dotnet-crap binary")
class EndToEndTests(unittest.TestCase):
    def run_scan(self, binary, root, coverage=None):
        command = [str(binary), "--root", str(root), "--format", "json", "--threshold", "1e12"]
        if coverage is not None:
            command += ["--coverage", str(coverage)]
        result = subprocess.run(command, capture_output=True, text=True, timeout=120, check=True)
        return json.loads(result.stdout)

    def test_every_workload_scans_and_collects(self):
        binary = Path(os.environ["DOTNET_CRAP_BENCH_BINARY"]).resolve(strict=True)
        for name, scale in SMALL_SCALES.items():
            with self.subTest(workload=name), tempfile.TemporaryDirectory() as tmp:
                tmp = Path(tmp)
                root = tmp / "root"
                wl = bench.generate(name, root, scale)
                coverage = tmp / "native.json"
                coverage.write_text(json.dumps(wl.native_coverage))
                bench.validate(self.run_scan(binary, root), wl, False)
                bench.validate(self.run_scan(binary, root, coverage), wl, True)

                stage = tmp / "stage"
                for rel, xml_text in wl.cobertura_reports:
                    path = stage / rel
                    path.parent.mkdir(parents=True, exist_ok=True)
                    path.write_text(xml_text, encoding="utf-8")
                script = tmp / "copy-reports.sh"
                script.write_text(f"#!/bin/sh\nset -e\ncp -R {shlex.quote(str(stage))}/. \"$1\"/\n")
                output = tmp / "collected.json"
                command = [str(binary), "collect", "--root", str(root), "--output", str(output),
                           "--expected-reports", str(len(wl.cobertura_reports)),
                           "--", "sh", str(script), "{results}"]
                subprocess.run(command, capture_output=True, text=True, timeout=300, check=True)
                collected = json.loads(output.read_text())
                self.assertEqual(collected["version"], 1)
                self.assertEqual(collected["files"], wl.native_coverage["files"])
                bench.validate(self.run_scan(binary, root, output), wl, True)


if __name__ == "__main__":
    unittest.main()
