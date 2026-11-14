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
# Sweep argument sets for the candidate only, against one baseline (see "Sweeping candidate arguments").
python3 scripts/benchmark.py --workload many-medium --runs 10 --baseline /path/to/base/dotnet-crap \
  --candidate-args= --candidate-args=--fail-on-empty --output target/bench/sweep.json
# A real repository checked out elsewhere (see "Real repositories").
python3 scripts/benchmark.py --repo /path/to/checkout --repo-name owner/name@sha \
  --baseline /path/to/base/dotnet-crap --output target/bench/repo.json
```

Release archives are built with `cargo build --profile dist` (thin LTO, defined in
`Cargo.toml`), so that is the build users run. The `Benchmark` workflow measures it;
the commands above work with either profile's binary. Compare only binaries built
with the same profile and toolchain.

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
| `many-medium` | 64 files (`M000.cs`...), 256 dense methods each, about 35 KB per file (the `dense-file` method shape, so every statement is a coverage point) | CPU-bound parsing spread over files: the case for per-file parallelism. Scale is the file count |
| `skew` | `Small/S000.cs`... (32 methods, about 4 KB each) plus one 4000-method file of about 556 KB, `ZLarge.cs`, which sorts last in output order on purpose | one large file next to many small ones: a scheduler that takes files in output order meets the large file as the tail, and peak memory is set by that file. Scale is the number of small files |

The independently derived small-files complexities are F=3 (base, if, &&) and G=2 (base, ternary).
With the generated hits, F has coverage 1/2 and CRAP 4.125. G is fully covered and
scores 2. Without coverage they score 12 and 6.

Callable and class names are unique across the files of `many-medium` and `skew`, so
a Cobertura report for the whole corpus has one `<class>` per C# class. The
`dense-file` generator shares its method builder with these two; the digests of the
five older corpora are pinned in `tests/test_bench_workloads.py` so a refactor cannot
change them silently.

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

Each output (`"version": 3`) records:

- **Provenance:** UTC time, platform, machine, CPU, Python, `rustc -vV`, source
  commit and dirty flag, the SHA-256 of the private executable copy actually run,
  and the load average (informational only; PRoot and some containers fake it).
  `cpu_count` is `os.cpu_count()` and `usable_cpus` the number of CPUs this process
  may run on (`sched_getaffinity` where it exists), which is the figure that bounds
  thread scaling. The workflow summary prints it.
- **Corpus:** workload, scale, corpus digest (every generated file, including
  decoys), files, callables, coverage points, report count, source bytes, and
  `span_point_visits`.
- **Samples:** every warm sample's wall time, the CLI's CPU time (`cpu_samples_ms`,
  user plus system, from the same `wait4` call, so it includes any child the CLI
  waited for such as the collector's replay shell) and the CLI's peak resident set
  size (KiB on both Linux and macOS), plus median, minimum and maximum of each, and
  `rss_floor_kib`. CPU time next to wall time shows whether a change saved work or
  only overlapped it: parallel scanning lowers wall time and leaves CPU time flat or
  higher.
- **Layout:** the candidate's samples are under `candidates`, a list with one entry per
  `--candidate-args` set (`args`, `label`, the samples and statistics above). With
  `--baseline` each entry also has `outputs_byte_identical` and a `comparison` block
  for `wall_ms` and `cpu_ms` (the candidate-over-baseline ratio of medians and
  `separated`, below), and a top-level `baseline` block carries the baseline's
  digest, samples and statistics.

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

With `--baseline`, the executables run on the same corpus in rotating order: with one
candidate set that is plain alternation (an even `--runs` is required so neither
binary runs first more often), and with `N` candidate sets each of the `N + 1`
executables starts the round equally often (`--runs` must be a multiple of `N + 1`).
Every run's outputs must be byte-identical (JSON stdout, or the collected evidence
file), and the result gains a `baseline` block.

### Sweeping candidate arguments

`--candidate-args` passes extra arguments to the candidate only; the baseline never
sees them. The value is split like a shell word list, and the form with an equals
sign is required because argparse would otherwise read a value starting with `--` as
an option: `--candidate-args='--jobs 4'`. Repeat it to sweep several argument sets
against one baseline, for example `--candidate-args='--jobs 1' --candidate-args='--jobs 2'
--candidate-args='--jobs 4' --candidate-args='--jobs 8'` with `--runs 10`. An empty
`--candidate-args=` is a set with no extra arguments. In `collect` mode the arguments
go before the `--` that starts the trusted command. The harness rejects arguments it
supplies itself (`--root`, `--threshold`, `--format`, `--output`, `--coverage`,
`--require-coverage`, `--expected-reports`) and a bare `--`.

Output identity is enforced for every set: each must produce output byte-identical
to the baseline's in every run, or the harness stops. A sweep therefore cannot
include an option that changes the output. Each set gets its own `comparison`, and
the workflow summary prints one row per set.

### The `separated` rule

A speedup is claimed only when every candidate sample beats every baseline sample:
`comparison.<metric>.separated` is `"lower"` when the slowest candidate sample is
faster than the fastest baseline sample (and likewise for CPU time), `"higher"` when
it is the other way round, and `null` otherwise. Touching ranges are not separated.
The summary marks such rows `separated`; every other row is an observation, not a
claim. Peak RSS is reported by median only.

## Real repositories

`--repo DIR` replaces the generated corpus with an existing checkout. It needs
`--baseline`, supports scan mode only, and the harness never writes into `DIR`.
Nothing there is built or run: the scanner only reads `.cs` files.

The harness records `files` and `source_bytes` by walking `DIR` with the scanner's
selection rules (skip `bin`, `obj`, `.git`, symbolic links, `*.g.cs`,
`*.generated.cs` and `*.designer.cs`), next to the scanner's own `files_scanned`.
The summary flags a disagreement. There is no expected-score validation: the only
check is that base and head agree.

A repository the scanner rejects (exit 2, for example `invalid C# syntax`) is a
finding about the scanner on real code, not a harness failure, provided every
executable fails identically (same exit status, stdout and stderr). The failure is
recorded under `repo.scan_failure` and shown in the summary row. Its timings cover
only the work up to the failure, since a scan stops at the first failing file. A
disagreement between executables, a crash (any exit status other than 0 or 2), a
malformed report or a timeout is an error.

The `Benchmark` workflow measures three repositories after the generated
workloads, each pinned by full commit SHA and fetched with `persist-credentials:
false` and a sparse checkout of `*.cs` only:

| directory | repository | role | selected files |
|---|---|---|---|
| `FluentValidation` | `FluentValidation/FluentValidation` | library | 218 |
| `eShopOnWeb` | `dotnet-architecture/eShopOnWeb` | web application | 250 |
| `CleanArchitecture` | `ardalis/CleanArchitecture` | multi-project solution | 364 |

The pins live in `.github/workflows/benchmark.yml`; update a pin there and nothing
else. The three were chosen because the scanner reads all of them without error, so
the rows measure a whole scan. Most open-source C# repositories tried first did not
scan: at the default-branch tips of `JamesNK/Newtonsoft.Json`, `dotnet/eShop`,
`serilog/serilog`, `Humanizr/Humanizer`, `DapperLib/Dapper`, `App-vNext/Polly`,
`StackExchange/StackExchange.Redis`, `JoshClose/CsvHelper`, `restsharp/RestSharp`
and `bchavez/Bogus` on 2026-11-13 the scanner stops with
`invalid C# syntax` on the first file that fails, and `mbdavid/LiteDB` stops on a
decoding error. That is a finding for the scanner, not something this harness
works around. A failing repository can be added to the workflow and will appear
in the summary as a failure row.

## Comparison rules

Compare only measurements taken on one runner with one toolchain and one corpus,
preferably within one `--baseline` invocation. Timings from shared CI runners are
observations, not budgets. The `Benchmark` workflow builds the PR base and head in
the same job on Linux and macOS and uploads JSON plus a Linux `perf` profile. It builds
both with the `dist` profile when both define it and with `release` otherwise (so the
two are always built alike), and the summary names the profile. Do not compare
numbers across its jobs. No timing budget is enforced. `--max-median-ms`
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
