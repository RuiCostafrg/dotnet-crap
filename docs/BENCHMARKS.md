# Release benchmark

Build the release binary, then run `python3 scripts/benchmark.py`. Without options
it measures the historical `small-files` corpus in `scan` mode, so earlier results
stay comparable.

```sh
cargo build --locked --release
python3 scripts/benchmark.py --workload dense-file --mode scan-coverage --runs 7 \
  --output target/bench/dense.json
# Same-runner comparison: alternate two binaries on one corpus.
python3 scripts/benchmark.py --workload deep-nesting --mode scan-coverage \
  --baseline /path/to/base/dotnet-crap --output target/bench/deep-compare.json
```

## Workloads

`scripts/bench_workloads.py` generates every corpus deterministically and derives
the expected callable list from its own construction: identity and order
(file, name, kind, line, end line), complexity, coverage and CRAP. Every measured
invocation is checked against that list; a timing is never recorded for a wrong
report. Default scales are listed in `DEFAULT_SCALE`.

| workload | shape | stresses |
|---|---|---|
| `small-files` | 500 files with two callables each (F=3, G=2) | per-file overhead (historical corpus, byte-identical) |
| `dense-file` | one file, 4000 methods, 8 per class, every statement covered | parsing and dense coverage in one file; many Coverlet `<class>` records for one file |
| `many-callables` | one file, 8000 callables rotating method, constructor, local function, lambda, arrow property, accessors, operator | callable recognition and naming |
| `deep-nesting` | one method with 3000 nested lambdas around 2000 statements | repeated coverage-point visits across nested callables |
| `solution` | 100 projects, each with `src/` and `tests/` trees (500 selected files), plus `bin/`, `obj/`, `*.g.cs`, `*.generated.cs`, `*.Designer.cs` decoys | directory walk and exclusion rules; one Cobertura report per project that also covers real unselected decoys, which import must ignore |

The independently derived small-files complexities are F=3 (base, if, &&) and G=2 (base, ternary).
With the generated hits, F has coverage 1/2 and CRAP 4.125. G is fully covered and
scores 2. Without coverage they score 12 and 6.

## Modes

- `scan`: `--format json` without coverage.
- `scan-coverage` (or `--coverage`): native v1 coverage with `--require-coverage`.
- `collect`: `dotnet-crap collect` with a tooling-only trusted command that copies
  pre-generated Coverlet-shaped Cobertura reports into the results directory. The
  reports follow the real Coverlet layout (`<sources>` absolute root, one `<class>`
  per C# class, method-level and class-level `<lines>`, branch conditions). The
  collected native evidence must equal the generator's line map. This mode times
  validation, Cobertura import, the internal verification scan and the evidence
  write. It does not run `dotnet test`. The collector polls its child every 10 ms,
  so collect timings carry that granularity.

## Recorded data

Each output (`"version": 2`) records:

- **Provenance:** UTC time, platform, machine, CPU, Python, `rustc -vV`, source
  commit and dirty flag, and the SHA-256 of the private executable copy actually run.
- **Corpus:** workload, scale, corpus digest (every generated file, including
  decoys), files, callables, coverage points, report count, source bytes, and
  `span_point_visits`.
- **Samples:** every warm sample's wall time and the child's own peak resident set
  size from `wait4` (KiB on both Linux and macOS), plus median, minimum and maximum.

`span_point_visits` is the number of coverage points that fall inside each
callable's full line span, summed over callables. It is a deterministic work
measure: it equals the per-point visits made by a scorer that walks each
callable's whole span before excluding nested callables.

With `--baseline`, both executables run on the same corpus in alternating ABBA
order. Every run's outputs must be byte-identical (JSON stdout, or the collected
evidence file), and the result gains a `baseline` block.

## Comparison rules

Compare only measurements taken on one runner with one toolchain and one corpus,
preferably within one `--baseline` invocation. Timings from shared CI runners are
observations, not budgets. The `Benchmark` workflow builds the PR base and head in
the same job on Linux and macOS and uploads JSON plus a Linux `perf` profile. Do
not compare numbers across its jobs. No timing budget is enforced. `--max-median-ms`
exists for locally controlled experiments only.

## Safety

`--timeout` sets a finite positive per-invocation timeout (120 seconds by default).
Executables are copied into a private temporary directory before runs, and their
digests are recorded. `--output` is written by atomic replacement. Its parent
directory must be writable, and it cannot be a symlink or alias a measured
executable. Other external output paths are supported.

The `--binary` and `--baseline` options intentionally execute the selected local
programs. Commands use an argv list with `shell=False`; no shell interprets path
or argument text. The two generic dynamic-subprocess audit warnings are
suppressed only at the reviewed call site for that reason. Quoting argv elements
with shlex would alter literal paths and is not a security fix for this execution
model.
