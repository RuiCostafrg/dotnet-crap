#!/usr/bin/env python3
"""Deterministic C# benchmark workloads with independently derived expectations.

Each generator writes a C# corpus and returns the records the scanner must
report. Expected values come from the generator's own layout: line numbers,
complexity by construction (base 1 plus the documented decisions), and coverage
ownership (a callable owns its coverage points except lines inside nested
callable spans). Nothing here runs the scanner.

Coverage points use a fixed hit pattern, ``hits = (line * 7) % 3``, unless a
generator states explicit hits (small-files keeps the historical 3/4/6 pattern).
"""
import dataclasses
import hashlib
import math
import pathlib
import xml.etree.ElementTree as ET

WORKLOADS = ("small-files", "dense-file", "many-callables", "deep-nesting", "solution",
             "many-medium", "skew")

# Release-scale defaults, sized so one release scan takes roughly 0.2-2 seconds.
DEFAULT_SCALE = {
    "small-files": 500,  # files; ~0.6 s scan
    "dense-file": 4000,  # methods in one file; ~0.6 s scan
    "many-callables": 8000,  # callable slots in one file; ~0.5 s scan
    "deep-nesting": 3000,  # nested lambdas; ~0.7 s scan
    "solution": 100,  # projects (5 selected files each); ~1 s scan
    "many-medium": 64,  # files of about 35 KB each, parse-bound
    "skew": 63,  # small files next to one large file
}
# Filler statements in the innermost deep-nesting lambda (module global so tests can shrink it).
DEEP_FILLER = 2000
# Methods (dense-file) or callable slots (many-callables) per emitted C# class.
METHODS_PER_CLASS = 8
# Methods per file in many-medium (about 35 KB of source), and in skew's one large
# file and each of its small files. Module globals so tests can shrink them.
MEDIUM_METHODS = 256
SKEW_LARGE_METHODS = 4000
SKEW_SMALL_METHODS = 32
REPORT_TIMESTAMP = 1700000000
SINGLE_REPORT = "00000000-0000-4000-8000-000000000001/coverage.cobertura.xml"
_CSPROJ = (
    b'<Project Sdk="Microsoft.NET.Sdk">\n'
    b"  <PropertyGroup>\n"
    b"    <TargetFramework>net8.0</TargetFramework>\n"
    b"  </PropertyGroup>\n"
    b"</Project>\n"
)


@dataclasses.dataclass
class Workload:
    """A generated corpus plus the expectations a scan must satisfy."""

    name: str
    scale: int
    files: int
    source_bytes: int
    corpus_sha256: str
    native_coverage: dict
    expected: list
    cobertura_reports: list


@dataclasses.dataclass(frozen=True)
class _Callable:
    owner: str
    name: str
    kind: str
    line: int
    end_line: int
    complexity: int
    owned: tuple


def _hits(line: int) -> int:
    """Deterministic hit pattern: 0, 1 or 2 hits on an executable line."""
    return (line * 7) % 3


def _path_key(rel: str) -> tuple:
    """Order paths by components, matching the scanner's BTreeSet of PathBuf."""
    return tuple(rel.split("/"))


class _Source:
    """One C# file under construction: text, coverage points and callable records."""

    def __init__(self, rel: str, selected: bool = True) -> None:
        self.rel = rel
        self.selected = selected
        self.text: list = []
        self.owner: dict = {}
        self.points: dict = {}
        self.conditions: set = set()
        self.classes: list = []
        self.callables: list = []

    @property
    def next_line(self) -> int:
        return len(self.text) + 1

    def declare(self, qualified: str) -> None:
        self.classes.append(qualified)

    def add(self, text: str, owner=None, hits=None, conditions: bool = False) -> int:
        """Append a line; ``hits`` makes it a coverage point owned by ``owner``."""
        if hits is not None and owner is None:
            raise ValueError("coverage points need an owning class")
        number = self.next_line
        self.text.append(text)
        if owner is not None:
            self.owner[number] = owner
        if hits is not None:
            self.points[number] = hits
        if conditions:
            self.conditions.add(number)
        return number

    def point(self, text: str, owner: str, conditions: bool = False) -> int:
        """Append an executable line with the deterministic hit pattern."""
        return self.add(text, owner, hits=_hits(self.next_line), conditions=conditions)

    def record(self, owner, name, kind, line, end_line, complexity, owned) -> None:
        self.callables.append(_Callable(owner, name, kind, line, end_line, complexity, tuple(owned)))

    def render(self) -> bytes:
        return ("\n".join(self.text) + "\n").encode("utf-8")


# ---------------------------------------------------------------- expectations

def _coverage(src: _Source, owned: tuple):
    """Hit points over owned points, as an exact ratio; None without points."""
    if not owned:
        return None
    hit = sum(1 for number in owned if src.points[number] > 0)
    return hit / len(owned)


def _crap(complexity: int, coverage) -> float:
    missed = 1.0 - (coverage if coverage is not None else 0.0)
    return float(complexity ** 2 * missed ** 3 + complexity)


def _expected(sources: list) -> list:
    items = []
    for src in sources:
        for call in src.callables:
            coverage = _coverage(src, call.owned)
            items.append({
                "file": src.rel,
                "name": call.name,
                "kind": call.kind,
                "line": call.line,
                "end_line": call.end_line,
                "complexity": call.complexity,
                "coverage": coverage,
                "crap": _crap(call.complexity, coverage),
            })
    return items


def _corpus_digest(files: dict) -> str:
    digest = hashlib.sha256()
    for rel in sorted(files):
        digest.update(f"{rel}\0{hashlib.sha256(files[rel]).hexdigest()}\n".encode("utf-8"))
    return digest.hexdigest()


def _native(sources: list, files: dict) -> dict:
    return {
        "version": 1,
        "files": {
            src.rel: {
                "sha256": hashlib.sha256(files[src.rel]).hexdigest(),
                "lines": {str(number): hits for number, hits in sorted(src.points.items())},
            }
            for src in sources
        },
    }


# ------------------------------------------------------------------- cobertura

def _rate(covered: int, valid: int) -> str:
    return f"{covered / valid:.4f}" if valid else "1"


def _branch_totals(src: _Source, numbers) -> tuple:
    covered = valid = 0
    for number in numbers:
        if number in src.conditions:
            covered += min(src.points[number], 2)
            valid += 2
    return covered, valid


def _fill_lines(parent: ET.Element, src: _Source, numbers) -> None:
    for number in numbers:
        hits = src.points[number]
        if number not in src.conditions:
            ET.SubElement(parent, "line", number=str(number), hits=str(hits), branch="False")
            continue
        covered = min(hits, 2)
        percent = f"{covered * 50}%"
        line = ET.SubElement(parent, "line", number=str(number), hits=str(hits), branch="True",
                             **{"condition-coverage": f"{covered * 50}% ({covered}/2)"})
        conditions = ET.SubElement(line, "conditions")
        ET.SubElement(conditions, "condition", number=str(number), type="jump", coverage=percent)


def _cobertura(root_abs: str, package: str, sources: list) -> str:
    """Coverlet-shaped report; class-level lines are the only evidence the importer reads."""
    coverage = ET.Element("coverage")
    source = ET.SubElement(ET.SubElement(coverage, "sources"), "source")
    source.text = root_abs
    package_el = ET.SubElement(ET.SubElement(coverage, "packages"), "package", name=package)
    classes_el = ET.SubElement(package_el, "classes")
    line_covered = line_valid = branch_covered = branch_valid = package_complexity = 0
    for src in sources:
        for cls in src.classes:
            numbers = sorted(n for n in src.points if src.owner[n] == cls)
            owned_calls = [c for c in src.callables if c.owner == cls]
            class_covered = sum(1 for n in numbers if src.points[n] > 0)
            class_branch = _branch_totals(src, numbers)
            class_complexity = sum(c.complexity for c in owned_calls)
            package_complexity += class_complexity
            line_covered += class_covered
            line_valid += len(numbers)
            branch_covered += class_branch[0]
            branch_valid += class_branch[1]
            class_el = ET.SubElement(
                classes_el, "class", name=cls, filename=src.rel,
                **{"line-rate": _rate(class_covered, len(numbers)),
                   "branch-rate": _rate(*class_branch), "complexity": str(class_complexity)})
            methods_el = ET.SubElement(class_el, "methods")
            for call in owned_calls:
                owned = sorted(call.owned)
                method_covered = sum(1 for n in owned if src.points[n] > 0)
                method = ET.SubElement(
                    methods_el, "method", name=call.name, signature="()",
                    **{"line-rate": _rate(method_covered, len(owned)),
                       "branch-rate": _rate(*_branch_totals(src, owned)),
                       "complexity": str(call.complexity)})
                _fill_lines(ET.SubElement(method, "lines"), src, owned)
            _fill_lines(ET.SubElement(class_el, "lines"), src, numbers)
    package_el.set("complexity", str(package_complexity))
    package_el.set("line-rate", _rate(line_covered, line_valid))
    package_el.set("branch-rate", _rate(branch_covered, branch_valid))
    coverage.set("line-rate", _rate(line_covered, line_valid))
    coverage.set("branch-rate", _rate(branch_covered, branch_valid))
    coverage.set("version", "1.9")
    coverage.set("timestamp", str(REPORT_TIMESTAMP))
    coverage.set("lines-covered", str(line_covered))
    coverage.set("lines-valid", str(line_valid))
    coverage.set("branches-covered", str(branch_covered))
    coverage.set("branches-valid", str(branch_valid))
    ET.indent(coverage, space="  ")
    return '<?xml version="1.0" encoding="utf-8"?>\n' + ET.tostring(coverage, encoding="unicode") + "\n"


# ------------------------------------------------------------------ generators

def _build_small_files(scale: int):
    """The historical corpus: byte-identical to scripts/benchmark.py's generator."""
    sources = []
    for index in range(scale):
        cls = f"C{index}"
        src = _Source(f"C{index:06}.cs")
        src.declare(cls)
        src.add(f"class {cls} {{", cls)
        f_sig = src.add(" int F(int x) {", cls)
        if_line = src.add("  if (x > 0 && x < 10) return x;", cls, hits=1, conditions=True)
        zero = src.add("  return 0;", cls, hits=0)
        src.add(" }", cls)
        g_line = src.add(" int G(int x) => x > 0 ? x : -x;", cls, hits=1, conditions=True)
        src.add("}", cls)
        src.record(cls, "F", "method_declaration", f_sig, 5, 3, [if_line, zero])
        src.record(cls, "G", "method_declaration", g_line, g_line, 2, [g_line])
        sources.append(src)
    return sources, {}, [(SINGLE_REPORT, "Bench.small-files", sources)]


def _dense_methods(src: _Source, class_prefix: str, first_class: int, first_method: int, count: int) -> None:
    """Append ``count`` dense methods, METHODS_PER_CLASS per class, numbered from the given starts."""
    index = 0
    for c in range(-(-count // METHODS_PER_CLASS)):
        cls = f"{class_prefix}{first_class + c:04}"
        src.declare(cls)
        src.add(f"class {cls} {{", cls)
        for _ in range(min(METHODS_PER_CLASS, count - index)):
            i = first_method + index
            index += 1
            name = f"M{i:05}"
            sig = src.add(f"  public int {name}(int x)", cls)
            src.add("  {", cls)
            init = src.point("    int s = 0;", cls)
            branch = src.point(f"    if (x > {i % 7} && x < {i % 11 + 20}) s += x;", cls, conditions=True)
            loop = src.point(f"    for (int j = 0; j < {i % 5 + 2}; j++) s += j;", cls)
            ret = src.point("    return s;", cls)
            end = src.add("  }", cls)
            # base 1 + if + && + for
            src.record(cls, name, "method_declaration", sig, end, 4, [init, branch, loop, ret])
        src.add("}", cls)


def _classes_for(methods: int) -> int:
    return -(-methods // METHODS_PER_CLASS)


def _build_dense(scale: int):
    """One file of many methods, grouped METHODS_PER_CLASS per class."""
    src = _Source("Dense.cs")
    _dense_methods(src, "D", 0, 0, scale)
    return [src], {}, [(SINGLE_REPORT, "Bench.dense-file", [src])]


def _build_many_medium(scale: int):
    """``scale`` files of MEDIUM_METHODS dense methods each: parse-bound work spread over files."""
    sources = []
    for f in range(scale):
        src = _Source(f"M{f:03}.cs")
        _dense_methods(src, "D", f * _classes_for(MEDIUM_METHODS), f * MEDIUM_METHODS, MEDIUM_METHODS)
        sources.append(src)
    return sources, {}, [(SINGLE_REPORT, "Bench.many-medium", sources)]


def _build_skew(scale: int):
    """One large file next to ``scale`` small ones.

    The large file sorts last (``ZLarge.cs`` after ``Small/``) so a pool that takes
    files in output order meets it as the tail, the worst case for scheduling.
    """
    small = []
    for f in range(scale):
        src = _Source(f"Small/S{f:03}.cs")
        _dense_methods(src, "S", f * _classes_for(SKEW_SMALL_METHODS),
                       SKEW_LARGE_METHODS + f * SKEW_SMALL_METHODS, SKEW_SMALL_METHODS)
        small.append(src)
    large = _Source("ZLarge.cs")
    _dense_methods(large, "L", 0, 0, SKEW_LARGE_METHODS)
    sources = small + [large]
    return sources, {}, [(SINGLE_REPORT, "Bench.skew", sources)]


def _build_many(scale: int):
    """Callables of every supported kind in rotation, eight slots per class."""
    src = _Source("Many.cs")
    for c in range(-(-scale // METHODS_PER_CLASS)):
        cls = f"K{c:04}"
        first = c * METHODS_PER_CLASS

        def has(slot: int, first=first) -> bool:
            return first + slot < scale

        src.declare(cls)
        src.add(f"class {cls} {{", cls)
        src.add("  int _v;", cls)
        ctor_sig = src.add(f"  public {cls}()", cls)
        src.add("  {", cls)
        ctor_body = src.point(f"    _v = {c};", cls)
        local = src.point("    int Local(int y) { return y + _v; }", cls) if has(1) else None
        lam = src.point("    System.Func<int, int> F = z => z > 0 ? z : -z;", cls,
                        conditions=True) if has(2) else None
        ctor_end = src.add("  }", cls)
        # The constructor owns only its assignment; the nested callables are excluded.
        src.record(cls, cls, "constructor_declaration", ctor_sig, ctor_end, 1, [ctor_body])
        if local is not None:
            src.record(cls, "Local", "local_function_statement", local, local, 1, [local])
        if lam is not None:
            src.record(cls, "lambda_expression", "lambda_expression", lam, lam, 2, [lam])
        if has(3):
            method = src.point(f"  public int M{c:04}(int x) {{ return x > {c % 5} ? 1 : 0; }}", cls,
                               conditions=True)
            src.record(cls, f"M{c:04}", "method_declaration", method, method, 2, [method])
        if has(4):
            arrow = src.point(f"  public int P{c:04} => _v > {c % 3} ? 1 : 0;", cls, conditions=True)
            src.record(cls, f"P{c:04}", "property_declaration", arrow, arrow, 2, [arrow])
        if has(5):
            src.add(f"  public int A{c:04} {{", cls)
            getter = src.point("    get { return _v; }", cls)
            setter = src.point("    set { _v = value; }", cls) if has(6) else None
            src.add("  }", cls)
            src.record(cls, "get", "accessor_declaration", getter, getter, 1, [getter])
            if setter is not None:
                src.record(cls, "set", "accessor_declaration", setter, setter, 1, [setter])
        if has(7):
            op = src.point(f"  public static {cls} operator +({cls} a, int b) => a;", cls)
            src.record(cls, f"operator +({cls},int)", "operator_declaration", op, op, 1, [op])
        src.add("}", cls)
    return [src], {}, [(SINGLE_REPORT, "Bench.many-callables", [src])]


def _build_deep(scale: int):
    """One method holding ``scale`` nested lambdas; the innermost owns DEEP_FILLER statements."""
    src = _Source("Deep.cs")
    src.declare("Deep")
    src.add("class Deep {", "Deep")
    run_sig = src.add("  public int Run() {", "Deep")
    total = src.point("    int total = 0;", "Deep")
    openers = [src.point(f"    System.Action a{k} = () => {{", "Deep") for k in range(1, scale + 1)]
    filler = [src.point(f"    total += {j};", "Deep") for j in range(1, DEEP_FILLER + 1)]
    # closers[0] closes the innermost lambda; closers[scale - k] closes lambda k.
    closers = [src.point("    };", "Deep") for _ in range(scale)]
    ret = src.point("    return total;", "Deep")
    run_end = src.add("  }", "Deep")
    src.add("}", "Deep")
    src.record("Deep", "Run", "method_declaration", run_sig, run_end, 1, [total, ret])
    for k in range(1, scale + 1):
        opener = openers[k - 1]
        closer = closers[scale - k]
        # Each outer lambda keeps only its opener and closer; the innermost keeps the filler.
        owned = [opener, closer] if k < scale else [opener, *filler, closer]
        src.record("Deep", "lambda_expression", "lambda_expression", opener, closer, 1, owned)
    return [src], {}, [(SINGLE_REPORT, "Bench.deep-nesting", [src])]


def _model(src: _Source, tag: str, ns: str) -> None:
    owner = f"{ns}.Core.Model{tag}"
    src.declare(owner)
    src.add(f"namespace {ns}.Core", None)
    src.add("{", None)
    src.add(f"    public class Model{tag}", owner)
    src.add("    {", owner)
    scale = src.point("        public int Scale(int x) => x > 0 ? x : 0;", owner, conditions=True)
    sig = src.add("        public int Sum(int a, int b)", owner)
    src.add("        {", owner)
    total = src.point("            int total = a + b;", owner)
    branch = src.point("            if (total > 0 && a > 0) return total;", owner, conditions=True)
    ret = src.point("            return 0;", owner)
    end = src.add("        }", owner)
    src.add("    }", owner)
    src.add("}", owner)
    src.record(owner, "Scale", "method_declaration", scale, scale, 2, [scale])
    src.record(owner, "Sum", "method_declaration", sig, end, 3, [total, branch, ret])


def _formatter(src: _Source, tag: str, ns: str) -> None:
    owner = f"{ns}.Services.Formatter{tag}"
    src.declare(owner)
    src.add(f"namespace {ns}.Services", None)
    src.add("{", None)
    src.add(f"    public static class Formatter{tag}", owner)
    src.add("    {", owner)
    sig = src.add("        public static string Label(int x)", owner)
    src.add("        {", owner)
    switch = src.point("            switch (x)", owner)
    src.add("            {", owner)
    first_case = src.point('                case 0: return "zero";', owner)
    second_case = src.point('                case 1: return "one";', owner)
    default = src.point('                default: return "many";', owner)
    src.add("            }", owner)
    end = src.add("        }", owner)
    src.add("    }", owner)
    src.add("}", owner)
    # base 1 + two non-default case labels; default adds nothing.
    src.record(owner, "Label", "method_declaration", sig, end, 3, [switch, first_case, second_case, default])


def _calculator(src: _Source, tag: str, ns: str) -> None:
    owner = f"{ns}.Numerics.Calculator{tag}"
    src.declare(owner)
    src.add(f"namespace {ns}.Numerics", None)
    src.add("{", None)
    src.add(f"    public class Calculator{tag}", owner)
    src.add("    {", owner)
    sig = src.add("        public int Run(int n)", owner)
    src.add("        {", owner)
    acc = src.point("            int acc = 0;", owner)
    loop = src.point("            for (int i = 0; i < n; i++)", owner)
    src.add("            {", owner)
    body = src.point("                acc += i;", owner)
    src.add("            }", owner)
    lam = src.point("            System.Func<int, int> g = v => v > 0 ? v : 0;", owner, conditions=True)
    ret = src.point("            return acc + g(n);", owner)
    end = src.add("        }", owner)
    src.add("    }", owner)
    src.add("}", owner)
    # base 1 + for; the lambda's ?: belongs to the lambda.
    src.record(owner, "Run", "method_declaration", sig, end, 2, [acc, loop, body, ret])
    src.record(owner, "lambda_expression", "lambda_expression", lam, lam, 2, [lam])


def _model_tests(src: _Source, tag: str, ns: str) -> None:
    owner = f"{ns}.Tests.ModelTests{tag}"
    src.declare(owner)
    src.add(f"namespace {ns}.Tests", None)
    src.add("{", None)
    src.add(f"    public class ModelTests{tag}", owner)
    src.add("    {", owner)
    sig = src.add("        public void ScaleHandlesPositive()", owner)
    src.add("        {", owner)
    model = src.point(f"            var m = new {ns}.Core.Model{tag}();", owner)
    check = src.point('            if (m.Scale(2) != 2) throw new System.Exception("scale");', owner)
    end = src.add("        }", owner)
    src.add("    }", owner)
    src.add("}", owner)
    src.record(owner, "ScaleHandlesPositive", "method_declaration", sig, end, 2, [model, check])


def _calculator_tests(src: _Source, tag: str, ns: str) -> None:
    owner = f"{ns}.Tests.CalculatorTests{tag}"
    src.declare(owner)
    src.add(f"namespace {ns}.Tests", None)
    src.add("{", None)
    src.add(f"    public class CalculatorTests{tag}", owner)
    src.add("    {", owner)
    sig = src.add("        public void Runs()", owner)
    src.add("        {", owner)
    calc = src.point(f"            var c = new {ns}.Numerics.Calculator{tag}();", owner)
    run = src.point("            int r = c.Run(3);", owner)
    ok = src.point("            bool ok = r > 0 && r < 100;", owner, conditions=True)
    check = src.point('            System.Action check = () => { if (!ok) throw new System.Exception("run"); };', owner)
    call = src.point("            check();", owner)
    end = src.add("        }", owner)
    src.add("    }", owner)
    src.add("}", owner)
    src.record(owner, "Runs", "method_declaration", sig, end, 2, [calc, run, ok, call])
    src.record(owner, "lambda_expression", "lambda_expression", check, check, 2, [check])


def _decoy(src: _Source, cls: str) -> _Source:
    """Generated or build-output C# that the scanner must not select."""
    src.declare(cls)
    src.add(f"class {cls} {{", cls)
    src.add("  int M(int x)", cls)
    src.add("  {", cls)
    src.point("    return x > 0 ? x : 0;", cls, conditions=True)
    src.add("  }", cls)
    src.add("}", cls)
    return src


def _build_solution(scale: int):
    """``scale`` projects with src/tests trees, decoys the scanner must skip, one report each."""
    sources, extras, reports = [], {}, []
    extras["bin/Release/Root.cs"] = b"class RootDecoy\n{\n}\n"
    extras["Bench.sln"] = b"Microsoft Visual Studio Solution File, Format Version 12.00\n"
    for index in range(scale):
        number = index + 1
        tag = f"{number:03}"
        ns = f"Bench.Proj{tag}"
        base = f"src/Proj{tag}"
        tests = f"tests/Proj{tag}.Tests"
        extras[f"{base}/Proj{tag}.csproj"] = _CSPROJ
        extras[f"{tests}/Proj{tag}.Tests.csproj"] = _CSPROJ
        model = _Source(f"{base}/Core/Model{tag}.cs")
        _model(model, tag, ns)
        formatter = _Source(f"{base}/Services/Formatter{tag}.cs")
        _formatter(formatter, tag, ns)
        calc = _Source(f"{base}/Services/Calculator{tag}.cs")
        _calculator(calc, tag, ns)
        model_tests = _Source(f"{tests}/ModelTests{tag}.cs")
        _model_tests(model_tests, tag, ns)
        calc_tests = _Source(f"{tests}/CalculatorTests{tag}.cs")
        _calculator_tests(calc_tests, tag, ns)
        decoys = [
            _decoy(_Source(f"{base}/bin/Debug/net8.0/Bin{tag}.cs", False), f"Bin{tag}"),
            _decoy(_Source(f"{base}/obj/Debug/net8.0/Obj{tag}.cs", False), f"Obj{tag}"),
            _decoy(_Source(f"{base}/Core/Model{tag}.g.cs", False), f"ModelGen{tag}"),
            _decoy(_Source(f"{base}/Services/Formatter{tag}.generated.cs", False), f"FormatterGen{tag}"),
            _decoy(_Source(f"{base}/Services/Form{tag}.Designer.cs", False), f"FormDesigner{tag}"),
            _decoy(_Source(f"{tests}/obj/Gen{tag}.cs", False), f"TestGen{tag}"),
        ]
        project = [model, formatter, calc, model_tests, calc_tests]
        sources.extend(project + decoys)
        guid = f"00000000-0000-4000-8000-{number:012x}/coverage.cobertura.xml"
        reports.append((guid, f"Proj{tag}", project + decoys))
    return sources, extras, reports


_BUILDERS = {
    "small-files": _build_small_files,
    "dense-file": _build_dense,
    "many-callables": _build_many,
    "deep-nesting": _build_deep,
    "solution": _build_solution,
    "many-medium": _build_many_medium,
    "skew": _build_skew,
}


def generate(workload: str, root: pathlib.Path, scale: int) -> Workload:
    """Write the corpus for ``workload`` under the empty directory ``root``."""
    if workload not in _BUILDERS:
        raise ValueError(f"unknown workload: {workload!r}")
    if isinstance(scale, bool) or not isinstance(scale, int) or scale < 1:
        raise ValueError("scale must be a positive integer")
    root = pathlib.Path(root)
    root.mkdir(parents=True, exist_ok=True)
    if any(root.iterdir()):
        raise ValueError("workload root must be empty")
    sources, extras, reports = _BUILDERS[workload](scale)
    files = dict(extras)
    for src in sources:
        if src.rel in files:
            raise ValueError(f"duplicate workload path: {src.rel}")
        files[src.rel] = src.render()
    for rel, data in files.items():
        path = root / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
    selected = sorted((s for s in sources if s.selected), key=lambda s: _path_key(s.rel))
    root_abs = str(root.resolve()) + "/"
    return Workload(
        name=workload,
        scale=scale,
        files=len(selected),
        source_bytes=sum(len(files[s.rel]) for s in selected),
        corpus_sha256=_corpus_digest(files),
        native_coverage=_native(selected, files),
        expected=_expected(selected),
        cobertura_reports=[
            (relpath, _cobertura(root_abs, package, report_sources))
            for relpath, package, report_sources in reports
        ],
    )


def validate(report: dict, workload: Workload, has_coverage: bool) -> None:
    """Raise ValueError unless ``report`` matches the workload's expectations exactly."""
    if not isinstance(report, dict) or not isinstance(report.get("functions"), list):
        raise ValueError("report must contain a functions list")
    functions = report["functions"]
    expected = workload.expected
    if len(functions) != len(expected):
        raise ValueError(f"expected {len(expected)} callables, found {len(functions)}")
    for position, (got, want) in enumerate(zip(functions, expected)):
        if not isinstance(got, dict):
            raise ValueError(f"callable {position} is not an object")
        identity = ("file", "name", "kind", "line", "end_line")
        if any(got.get(key) != want[key] for key in identity):
            raise ValueError(
                f"callable {position} differs in order or identity: "
                f"expected {tuple(want[key] for key in identity)}, "
                f"found {tuple(got.get(key) for key in identity)}")
        complexity = got.get("complexity")
        if isinstance(complexity, bool) or complexity != want["complexity"]:
            raise ValueError(f"callable {position} complexity differs")
        coverage = want["coverage"] if has_coverage else None
        if got.get("coverage") != coverage:
            raise ValueError(f"callable {position} coverage differs")
        if has_coverage:
            crap_expected = want["crap"]
        else:
            crap_expected = float(want["complexity"] ** 2 + want["complexity"])
        crap = got.get("crap")
        if (isinstance(crap, bool) or not isinstance(crap, (int, float))
                or not math.isclose(crap, crap_expected, rel_tol=0, abs_tol=1e-12)):
            raise ValueError(f"callable {position} CRAP score differs")
    files_scanned = report.get("files_scanned")
    if isinstance(files_scanned, bool) or files_scanned != workload.files:
        raise ValueError("files_scanned differs from the generated corpus")
    complete = has_coverage and all(want["coverage"] is not None for want in expected)
    if report.get("coverage_complete") is not complete:
        raise ValueError("coverage_complete differs from the generated evidence")
    if report.get("failed") is not False:
        raise ValueError("workload must pass its threshold gate")
