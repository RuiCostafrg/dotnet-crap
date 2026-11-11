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
| `dense-file` | one file, 4000 methods, 8 per class, a coverage point on every statement (hits vary 0/1/2) | parsing and dense coverage in one file; many Coverlet `<class>` records for one file |
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
  commit and dirty flag, the SHA-256 of the private executable copy actually run,
  and the load average (informational only; PRoot and some containers fake it).
- **Corpus:** workload, scale, corpus digest (every generated file, including
  decoys), files, callables, coverage points, report count, source bytes, and
  `span_point_visits`.
- **Samples:** every warm sample's wall time and the CLI's peak resident set size
  (KiB on both Linux and macOS), plus median, minimum and maximum, and
  `rss_floor_kib`.

Each invocation goes through a small spawner (`python -I -S`). It forks the CLI
into its own process group and times it. It kills the whole group on timeout and
reads the CLI's own `ru_maxrss` from `wait4`. On Linux, exec folds the spawning
address space's RSS high-water mark into the new process. Spawning directly from
the harness, which holds the generated corpus, would therefore floor every sample
at the harness's size. `rss_floor_kib` is the value reported for `true` through
the same spawner. A sample at or near the floor is an upper bound, not a
measurement of the CLI. Spawner start-up is excluded from wall time.

`span_point_visits` is the number of coverage points that fall inside each
callable's full line span, summed over callables. It is a deterministic work
measure: it equals the per-point visits made by a scorer that walks each
callable's whole span before excluding nested callables.

With `--baseline`, both executables run on the same corpus in alternating ABBA
order (an even `--runs` is required so neither binary runs first more often). Every run's outputs must be byte-identical (JSON stdout, or the collected
evidence file), and the result gains a `baseline` block.

## Comparison rules

Compare only measurements taken on one runner with one toolchain and one corpus,
preferably within one `--baseline` invocation. Timings from shared CI runners are
observations, not budgets. The `Benchmark` workflow builds the PR base and head in
the same job on Linux and macOS and uploads JSON plus a Linux `perf` profile. Do
not compare numbers across its jobs. No timing budget is enforced. `--max-median-ms`
exists for locally controlled experiments only.

## Platforms

The harness supports Linux and macOS. It rejects Windows, which lacks `wait4`.

## Safety

`--timeout` sets a finite positive per-invocation timeout (120 seconds by default).
Executables are copied into a private temporary directory before runs, and their
digests are recorded. `--output` is written by atomic replacement. Its parent
directory must be writable, and it cannot be a symlink or alias a measured
executable. Other external output paths are supported.

The `--binary` and `--baseline` options intentionally execute the selected local
programs, and `--rustc` names the compiler whose `rustc -vV` output is recorded.
Commands use argv lists with `shell=False`; no shell interprets path or argument
text. The generic dynamic-subprocess audit warnings are suppressed only at
reviewed sites, each with an inline reason:

- the measuring spawner invocation;
- the provenance probe helper (`git`, `sysctl`, `--rustc`);
- the line that constructs the `TimeoutExpired` exception it reports, which runs
  nothing.

Quoting argv elements with shlex would alter literal paths and is not a security
fix for this execution model.
