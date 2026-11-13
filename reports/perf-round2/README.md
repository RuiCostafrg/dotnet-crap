# Performance round 2: prototype evidence (main@1fc580e)

Five prototypes were built against `main` at `1fc580e` to find where CPU time and
memory go and what can be removed without changing output. This file records what
was measured. The implementation order is in
[`docs/PERFORMANCE_PLAN.md`](../../docs/PERFORMANCE_PLAN.md).

Nothing here is merged. The patches in [`prototypes/`](prototypes/) are starting
points, not reviewed code. `A`, `B`, `C-all-R1-R5`, `C-R5` and `E` each apply to
`1fc580e` on their own (`git apply --check`). `C-R1` to `C-R4` apply only in order
on top of `C-R0`, the instrumentation patch.

## Runner and claim rule

- **Runner:** Android 12, Qualcomm KONA (8 × aarch64, big.LITTLE: 4 × 1.80 GHz,
  3 × 2.42 GHz, 1 × 3.09 GHz), Ubuntu userland under PRoot, shared with other
  sessions. rustc 1.96.0, host `aarch64-linux-android`, so binaries link Android's
  bionic libc and its allocator, not glibc.
- **Baseline binary:** `cargo build --locked --release` of `1fc580e`, sha256 `6f37382f…`.
- PRoot intercepts every syscall, so file-heavy phases are 30–50× slower than on a
  native kernel. Absolute times do not transfer. Only same-invocation, alternating
  comparisons are reported.
- **Claim rule (unchanged from step 8):** a speedup is claimed only when every
  candidate sample beats every baseline sample (`separated`). Other rows are
  observations.
- Every candidate produced byte-identical output to the baseline on all five
  generated workloads, in scan and scan-coverage modes.

## Where the time goes (scan)

From the hosted-runner `perf` profile recorded in step 8
(`reports/perf-step8/ci-ubuntu-24.04/profiles/`): tree-sitter parsing is about
55–60% of a dense-file scan, cursor traversal driven by `analysis::collect` about
14–20%, and first-party code a few percent. In-process on this runner
([`evidence/A-report.md`](evidence/A-report.md)), `collect` costs 800–960 ns per
named node, roughly half of parse time.

## A. Single-pass traversal (`src/analysis.rs`)

Patch: [`prototypes/A-single-pass-traversal.patch`](prototypes/A-single-pass-traversal.patch).
One reused `TreeCursor`, dispatch on `kind_id` through a table built once per scan,
a stack of open callables, and the operator read from the `operator` field instead
of `utf8_text`. The old traversal is kept as the fallback when a resource limit is
hit, so error messages and their precedence are unchanged.

In-process, same pre-parsed trees, order rotated, 30 iterations:

| workload | reference collect min ms | single pass min ms | collect speedup | share of parse + collect removed |
|---|---|---|---|---|
| dense-file | 162.7 | 51.5 | ×3.16 | 23.6% |
| many-callables | 92.5 | 35.7 | ×2.59 | 21.4% |
| deep-nesting | 33.2 | 12.5 | ×2.67 | 21.5% |
| small-files | 18.7 | 6.8 | ×2.74 | 23.9% |
| solution | 33.4 | 12.9 | ×2.60 | 21.4% |

End to end, 12 alternating runs ([`evidence/A-cpu.log`](evidence/A-cpu.log)):

| workload (scan) | wall ratio | CPU ratio | separated |
|---|---|---|---|
| dense-file | 0.743 | 0.779 | yes |
| many-callables | 0.811 | 0.824 | no |
| deep-nesting | 0.880 | 0.881 | no |
| small-files | 1.089 | 0.972 | no (syscall-bound here) |

Two traps found on the way: `TreeCursor::depth()` is O(depth) in tree-sitter 0.27
and makes a per-step walk quadratic on deep nesting (3,963 ms vs 33 ms), and
building the kind table per file halves the gain on many-file workloads.

Correctness: a differential test compares the new pass with the reference on
about 1,600 source files and on every budget pair in 0..=40 × 0..=40 across nine
sources, including error kind and message.

## B. Parallel per-file scan (`src/lib.rs`, new `src/par.rs`)

Patch: [`prototypes/B-parallel-scan.patch`](prototypes/B-parallel-scan.patch).
`std::thread::scope`, one parser per worker, results placed by file index, lowest
failing index wins, and the final source verification runs on the same pool. No
new dependency. Output is byte-identical at 1, 2, 3 and 8 threads on all workloads.

Scaling on this runner ([`evidence/B-scaling.md`](evidence/B-scaling.md), 7
alternating runs, median wall):

| corpus | 1 thread | 2 | 4 | 8 | peak RSS 1 → 8 threads |
|---|---|---|---|---|---|
| heavy-64 (64 files × 35 KB, CPU-bound) | ×0.98 | ×1.25 | ×1.27 | ×1.13 | 28 → 50 MB |
| skew (one large file plus 63 small) | ×1.01 | ×1.10 | ×1.08 | ×0.92 | 53 → 52 MB |
| dense-file (1 file) | ×1.12 | ×1.11 | ×0.99 | ×0.93 | unchanged |
| small-files (500 files, syscall-bound here) | ×1.00 | ×1.40 | ×1.93 | ×2.46 | unchanged |

The CPU-bound scaling is poor **on this runner**, and the cause is the platform
allocator, not the design:

- User CPU for the same work rises from 1.8 s (1 thread) to 5.7 s (8 threads), and
  voluntary context switches from 1.5k to 41k. That is lock contention.
- Running independent baseline *processes* on the same corpus
  ([`evidence/B-multiproc.txt`](evidence/B-multiproc.txt)) gives throughput ×1.70
  at 2, ×2.72 at 4 and ×2.91 at 8. That is what the hardware allows without a
  shared heap, so it is the ceiling a thread pool can approach here.
- tree-sitter makes about 0.8 heap allocations per source byte (456,544 for a
  556 KB file), and this binary uses bionic's shared-pool allocator.

Release binaries target glibc, macOS and Windows, whose allocators use per-thread
caches. Scaling there is **not yet measured** and has to come from the hosted
Benchmark workflow before any speedup is claimed.

## C. `collect` redundancy (`src/collector.rs`, `src/cobertura.rs`, `src/lib.rs`)

Patches: [`C-R1`](prototypes/C-R1.patch) … [`C-R5`](prototypes/C-R5.patch), or all
at once in [`C-all-R1-R5.patch`](prototypes/C-all-R1-R5.patch). Full analysis in
[`evidence/C-report.md`](evidence/C-report.md).

Per selected file, baseline `collect` does 8 reads, 7 SHA-256 passes, 3 decodes and
2 tree-sitter parses. With R1–R5 it does 4 reads, 4 hashes, 1 decode and 1 parse.

| reduction | what | risk |
|---|---|---|
| R1 | reuse the snapshot's line counts; drop the unhashed re-read and the verify that directly follows scan's own verify | none; the line bound now comes from the hashed bytes, which is stronger |
| R2 | flag scan instead of a lower-cased copy of the whole report | none; precedence kept |
| R3 | check the staged evidence against the snapshot (same reader, validator and binding check) instead of a second full scan | semantic, see the plan |
| R4 | `BufWriter` around the staged evidence write | none; bytes identical |
| R5 | skip a duplicate canonicalization of the same candidate path | none |

End to end, 12 alternating runs, evidence output byte-identical
([`evidence/C-end-to-end.md`](evidence/C-end-to-end.md)):

| workload (collect) | wall ratio | CPU ratio |
|---|---|---|
| dense-file | 0.541 | 0.510 |
| many-callables | 0.575 | 0.541 |
| solution | 0.586 | 0.609 |
| small-files | 0.614 | 0.517 |
| deep-nesting | 0.638 | 0.553 |

Phase timings before and after are in
[`evidence/C-phase-base.md`](evidence/C-phase-base.md) and
[`evidence/C-phase-cand.md`](evidence/C-phase-cand.md). Notable single items: the
unbuffered evidence write was 169 ms on dense-file and is 4 ms buffered. Cobertura
class import on small-files went from 3.4 s to 1.0 s with R5.

The two plain-scan rows in the same table (ratios 1.04–1.16) are not separated and
are within this runner's noise. The prototype does not change the scan path's work.

## D. Release profile matrix

[`evidence/D-table.md`](evidence/D-table.md). `codegen-units=1`, thin LTO and fat
LTO were measured. **No variant separated from the default profile** on this
runner (median ratios 0.85–1.18, all overlapping). Sizes: default 7.50 MB, fat LTO
with one codegen unit 6.82 MB (−9%), plus `strip=symbols` 6.50 MB (−13%). Release
build time went from 40 s to 135 s. `panic=abort`, `opt-level` 2/s and mimalloc
were built but not timed.

**Hosted result (PR #40, closed unmerged).** The shipped `dist` profile (thin LTO)
was compared with `lto = "fat"` plus `codegen-units = 1` in the same job on
ubuntu-26.04 (run 43386807327, 21 generated rows and 3 real repositories). One row
separated: `many-medium` scan, wall and CPU ratio 0.980. Every other row was
between 0.987 and 1.007 and not separated, including all three real repositories
(0.995–1.005). On macos-26 no row separated. A 2% gain on one workload does not
meet the plan's rule, so the profile stays at thin LTO.

## E. Peak memory

[`evidence/E-table.md`](evidence/E-table.md) breaks RSS down by stage with a
counting Rust allocator and a counting tree-sitter allocator.

- **Scan peak is the syntax tree of the largest file.** Live tree memory is about
  74 bytes per source byte (41 MB for 556 KB, 82 MB for 1.1 MB, 164 MB for 2.2 MB),
  and peak RSS is about 2.3 MB plus 100× the largest file. The tree is freed before
  the next file, so many small files stay near 10 MB.
- **Rust-side data is under 10% of the peak.** Variants that shrink it barely move
  RSS ([`evidence/E-var-rss.txt`](evidence/E-var-rss.txt),
  [`evidence/E-var-rss2.txt`](evidence/E-var-rss2.txt), 5 alternating runs each):

  | variant | dense-file 4× (base 234.5 MB) | many-callables 4× (base 139.1 MB) |
  |---|---|---|
  | streamed JSON/SARIF output | +0.02 MB / −0.1 MB | +0.2 MB / +0.03 MB |
  | drop raw bytes after decode | −2.1 MB | −1.5 MB |
  | compact coverage index | −0.2 MB | −0.8 MB |

  Streaming output saves nothing because rendering happens after the tree is freed
  and reuses that memory. It is rejected.
- The current traversal allocates a further 97 MB in 378k short-lived cursor
  allocations on dense-file (`C total-alloc` 41.7 → 138.3 MB). That is churn, not
  peak, and the single-pass traversal (A) removes it.
- **`collect` peak is the Cobertura DOM.** Importing a 14.8 MB report peaks at
  113 MB of Rust heap (7.6× the report) and 141 MB RSS
  ([`evidence/E-cob.txt`](evidence/E-cob.txt)). With the 16 MiB per-report cap that
  is up to about 125 MB per report. On dense-file, `collect` peaks at 103 MB where
  plain scan peaks at 66 MB.

## Hosted results for the merged-path PRs

From the Benchmark workflow on ubuntu-26.04, base and head built and measured in
the same job, 8 alternating runs. `separated` means every head sample beat every
base sample. Outputs were byte-identical in every row.

| PR | rows separated | median wall ratio |
|---|---|---|
| #36 single-pass traversal | all 15 | scan 0.879–0.917, scan-coverage 0.890–0.918, collect 0.906–0.948 |
| #38 `collect` quick wins | the 5 `collect` rows | collect 0.693–0.846; scan rows 0.992–1.002, not separated |

On macos-26 the same runs were noisier: #36 separated only on dense-file scan
(0.820), #38 on deep-nesting collect (0.520) and many-callables collect (0.716).
The local prototype predicted 0.78 for dense-file scan from the traversal; the
hosted figure is 0.879.

## Not measured

- Thread scaling on glibc, macOS and Windows.
- Timings on real C# repositories. All timed corpora are generated; the dense
  single-file workloads are worst cases for memory and best cases for traversal
  gains. Real repositories were scanned for correctness only, see the next section.
- A streaming Cobertura reader, a custom tree-sitter allocator (`unsafe_code` is
  forbidden in this crate) and an on-disk per-file result cache were not prototyped.

## Found along the way: most real repositories do not scan

Preparing the real-repository benchmark showed that `main` rejects most well-known
C# repositories. One file with a syntax error fails the whole scan (exit 2), and
the tree-sitter grammar reports errors on valid C#. Repositories that failed:
Newtonsoft.Json, dotnet/eShop, Serilog, Humanizer, Dapper, Polly,
StackExchange.Redis, CsvHelper, RestSharp, Bogus, LiteDB. Four scanned cleanly:
FluentValidation, eShopOnWeb, ardalis/CleanArchitecture, MediatR.

Six of them were scanned file by file with the `main` binary
([`scripts/preprocess-probe.py`](scripts/preprocess-probe.py)). The probe also
re-scans each failing file after blanking `#if`/`#elif`/`#else`/`#endif` lines and
the branches that are inactive when no symbol is defined, keeping every line break.

| repository at commit | `.cs` files | failing | parse after blanking |
|---|---|---|---|
| Serilog `43105b3` | 217 | 18 | 17 |
| Newtonsoft.Json `52fa3ae` | 951 | 66 | 43 |
| Polly `88f2039` | 803 | 4 | 3 |
| Humanizer `4574b93` | 735 | 12 | 5 |
| LiteDB `b3c4406` | 1528 | 26 | 11 |
| Dapper `eb47546` | 157 | 1 | 0 |

Causes identified, each confirmed on a minimal input:

1. **`#if` that splits a declaration or statement.** The grammar accepts
   conditional compilation only around whole members or statements. Example:
   Serilog's `Task OnEmptyBatchAsync()` followed by `#if X { ... } #else ; #endif`.
   This is the majority (79 of 127 failing files parse after blanking).
2. **A directive on the last line with no trailing newline**, such as a final
   `#pragma warning restore 618`. The same file with a newline appended parses.
3. **File-based app directives** (`#:sdk`, `#:package`, `#:property`).
4. **Contextual keywords used as identifiers**: `var with = ...`,
   `required.Should()`. Valid C#, rejected by `tree-sitter-c-sharp` 0.23.
5. **Source that is not UTF-8** (two LiteDB files) fails with a decoding error.

Not every remaining failure was diagnosed. Blanking with no symbols defined is a
probe, not the fix: one Serilog file wraps its whole body in `#if FEATURE_X` and
is left with nothing valid.

Related scoring quirk, present on `main`: `#if A && B` inside a callable body
adds a decision point, because the grammar aliases preprocessor conditions to
`binary_expression`.

Recommended follow-up, as its own design and PR after this round:

- Parse as today first, so files that parse now keep byte-identical results. Only
  when that fails, resolve conditional compilation with a `--define` symbol list
  (blanking keeps offsets and line numbers, and the hash stays over the original
  bytes), supply a virtual trailing newline, and blank `#:` lines.
- Decide what a file that still cannot be parsed means for a gate. Today it fails
  the run. Cause 4 cannot be fixed on our side, so repositories that use those
  identifiers can never be scanned. An explicit opt-in that lists unparsed files in
  every output format and excludes them from scoring would make the tool usable
  there without hiding the gap.

## Reproduce

```sh
# thread scaling: RUNS BASE CAND CORPUS [extra scan args]
python3 reports/perf-round2/scripts/par_bench.py 7 base-bin cand-bin corpus/
# hardware ceiling with independent processes
python3 reports/perf-round2/scripts/multi_proc.py base-bin corpus/
# alternating peak RSS: N BIN_A BIN_B -- scan args
python3 reports/perf-round2/scripts/E-rss.py 5 base-bin cand-bin -- --root corpus/ --format json
```
