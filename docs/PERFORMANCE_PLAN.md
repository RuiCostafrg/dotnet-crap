# Performance plan, round 2

Continues [PLAN step 8](../PLAN.md). Evidence for every number below is in
[`reports/perf-round2/README.md`](../reports/perf-round2/README.md). Prototype
patches are in `reports/perf-round2/prototypes/`. They were cut against
`main@1fc580e`; `src/` has not changed on `main` since.

## What the investigation settled

| question | answer |
|---|---|
| Where does scan CPU go? | tree-sitter parse 55–60%, our traversal 14–20%, the rest small |
| Cheapest large CPU win for `collect`? | stop doing the work twice: −39% to −46% wall, −39% to −49% CPU, output identical |
| Cheapest CPU win for scan? | single-pass traversal: `collect` ×2.6–3.2, about 22% of scan CPU on dense files |
| Largest remaining win for multi-file repositories? | parallel per-file scan; output identical, real-platform scaling still to be measured |
| What bounds scan memory? | the syntax tree of the largest file, about 100× its size; our own data is under 10% |
| What bounds `collect` memory? | the Cobertura DOM, 7.6× the report size |
| Do build-profile changes help? | not measurably on the local runner; undecided |

## Decisions

These are settled so implementers do not reopen them.

1. **Accept C-R3**: `collect` checks the staged evidence against the snapshot
   instead of running a second full scan. The second scan re-proved facts that
   follow from identical bytes and a deterministic parser. The same reader,
   validator and binding check still run, and the pre-publish source verification
   stays. One behaviour changes: a source edited between the post-command and
   pre-publish verifications is always reported as `SourceChanged`, where the
   re-scan could report it as `Syntax` or stale coverage. Document that in
   `docs/COVERLET.md`.
2. **Keep the old traversal as the resource-limit fallback** for the single-pass
   traversal. It preserves which limit is reported first, and it is the oracle for
   differential tests.
3. **Parallelism uses `std::thread::scope`, no new dependency.** The worker count
   is a `--jobs <N>` option on `scan` and `collect` and a field in the library
   options. The prototype's `DOTNET_CRAP_THREADS` variable is removed: configuration
   precedence belongs to PLAN step 3.
4. **Parallel memory is bounded by bytes in flight.** A worker may start parsing
   only while the source bytes being parsed stay within
   `max(1 MiB, size of the file it wants to parse)`. Typical repositories run at
   full width. A repository of very large files degrades to sequential and keeps
   today's peak.
5. **Rejected:** streaming JSON/SARIF output (no peak-memory change), `panic=abort`
   (changes failure behaviour for no measured gain), a custom tree-sitter allocator
   (`unsafe_code` is forbidden), and metadata-only source verification (weakens the
   changed-source guarantee).
6. **Deferred:** an on-disk per-file result cache. It would skip parsing unchanged
   files, but it needs a cache location and a trust model, so it waits for PLAN
   step 3 (configuration) and gets its own design.
7. **No speedup is published from the local runner.** Each PR's claim comes from the
   hosted Benchmark workflow (ubuntu-26.04 and macos-26, same-job base and head,
   alternating order, `separated` rule). No timing gates yet.

## PR sequence

`src/lib.rs` is touched by PRs 2, 3 and 4, so those three land in order. PR 0 and
PR 1 are independent of everything and can run alongside. PR 5 needs only PR 0. PR 3 can be developed
in parallel with PR 1 and PR 2 and rebased, because its `lib.rs` change is two lines.

```text
PR 0 bench harness ─────────────┐
PR 1 collect quick wins ─ PR 2 collect single scan ─ PR 4 parallel scan ─ PR 6 Cobertura reader
PR 3 single-pass traversal ─────┘ (rebased before PR 4)
PR 5 release-profile experiment (after PR 0)
```

Every Rust PR passes the standard gate in `PLAN.md` (fmt, clippy with warnings
denied, debug and release tests, live Coverlet tests, Python contracts, mutation
gate strictly above 90%) and attaches the Benchmark workflow summary.

### PR 0: benchmark harness for this round

*Sonnet 5.5. Files: `scripts/benchmark.py`, `scripts/bench_workloads.py`, their
tests, `.github/workflows/benchmark.yml`, `docs/BENCHMARKS.md`. No `src/` change.*

- Record child CPU time (user + system) per sample next to wall time and RSS.
- Add two workloads: `many-medium` (64 files of about 35 KB, CPU-bound) and `skew`
  (one large file plus many small ones). Both exist only as ad-hoc corpora today.
- Let the harness pass extra arguments to the candidate only, so PR 4 can sweep
  `--jobs 1,2,4,8` against one baseline. Record the runner's CPU count.
- Add a real-repository job: two or three open-source C# repositories at pinned
  commits, scan only, base against head, reporting file count, bytes, wall, CPU,
  RSS and any syntax failure. This is the first measurement on real code.
- Build base and head with `--profile dist`, the profile release archives ship
  (thin LTO), falling back to `--release` when the base has no such profile. The
  workflow measures `--release` today, which is not what users run.

Done when the workflow runs green on a no-op `src/` change and the new rows appear
in the summary.

### PR 1: `collect` quick wins (R4, R5, R2)

*Sonnet 5.5. Port the R4, R5 and R2 hunks from `C-all-R1-R5.patch`. Files:
`src/collector.rs`, `src/cobertura.rs`.*

`C-R1` to `C-R4` are a stack on top of `C-R0` (instrumentation, not shipped) and do
not apply on their own. Read them to see one reduction at a time; take the code
from `C-all-R1-R5.patch`, which applies to `main` directly.

- Buffer the staged evidence write and flush before `sync_all`.
- Skip canonicalizing a candidate path identical to one already tried.
- Replace the lower-cased report copy with one pass that sets the three
  forbidden-declaration flags, checked in the existing order.

Done when evidence bytes are identical on all workloads and the unit test compares
the flag scan with `to_ascii_lowercase().contains` on edge cases. No behaviour or
documentation change.

### PR 2: `collect` parses each file once (R1, R3)

*Opus 5.5, reviewed by Sonnet 5.5. Port the R1 and R3 hunks from
`C-all-R1-R5.patch` on top of PR 1. Files:
`src/lib.rs`, `src/collector.rs`, `src/cobertura.rs`, `docs/COVERLET.md`,
`docs/RUST_API.md` if any public item moves.*

- The snapshot scan returns one line count per source, computed from the bytes
  that were hashed and parsed. `collect` drops its unhashed re-read and the
  verification that immediately follows the scan's own.
- Factor the stale-hash and line-bound check out of `scan` into one function used
  by both `scan` and a new snapshot check of the staged evidence. Messages and
  error kinds stay identical.
- Keep the order: stage, fsync, check staged evidence, verify sources, validate
  destination, publish.
- The prototype's direct unit tests for the snapshot check (stale hash, line out of
  range, bad key, bad JSON, wrong version) are required: those branches are
  unreachable through `collect` and would otherwise be mutation survivors.
- Do not carry over the `collect-profile` instrumentation (`C-R0`).

Done when per-file work is 4 reads, 4 hashes, 1 decode, 1 parse (assert the counts
in a test, not by timing), live Coverlet tests pass on xUnit, MSTest and NUnit, and
the hosted benchmark shows `collect` rows separated.

Expected from the prototype: `collect` wall and CPU down by 40–50% on every workload.

### PR 3: single-pass traversal

*Opus 5.5, reviewed by Sonnet 5.5. Start from `A-single-pass-traversal.patch`.
Files: `src/analysis.rs`, two lines in `src/lib.rs`.*

- Ship the single-pass walk, the kind table built once per scan, and the reference
  traversal as the limit fallback.
- Remove the prototype-only variants (`ids_only`, `cursor_two_pass`) and the
  benchmark test. Keep the differential tests against the reference, including the
  budget-boundary grid.
- Never call `TreeCursor::depth()` per step. Track depth by hand.
- Add a test-visible signal that the fast path completed without falling back, and
  assert it on ordinary inputs. Without it, a mutant that breaks the fast path
  falls back silently, produces identical output and survives.

Done when output is identical on all workloads, the differential tests pass, and
the hosted benchmark shows the dense-file scan rows separated.

Expected from the prototype: scan CPU −22% on dense-file (separated locally),
−12% to −18% on the other single-file workloads (not separated locally).

### PR 4: parallel per-file scan

*Opus 5.5, reviewed by Sonnet 5.5. Start from `B-parallel-scan.patch`, rebased on
PRs 2 and 3. Files: `src/lib.rs`, new `src/par.rs`, `src/main.rs`,
`docs/RUST_API.md`, `README.md`.*

- Ordered worker pool as prototyped: results by file index, lowest failing index
  wins, work past the first failure is abandoned, a single worker runs inline with
  no threads.
- One parser per worker, the kind table shared by reference.
- Source verification runs on the same pool.
- `--jobs <N>`; the default is the available parallelism. Apply decision 4's
  bytes-in-flight bound between reading a file and parsing it.
- Schedule large files first while keeping output order, so one big file does not
  end up as the tail.
- Tests: identical report and hash order for 1 to 8 workers, first-failure
  precedence for mixed error kinds, the memory bound holding with several large
  files, and `--jobs 1` matching the pre-change binary byte for byte.

Done when the hosted benchmark shows `many-medium`, `solution` and the
real-repository rows separated at the default worker count, single-file rows are
not slower at `--jobs 1`, and peak RSS on `skew` stays within 10% of sequential.

**Stop condition:** if the glibc runner shows less than ×1.5 at 4 workers on
`many-medium`, do not merge. Report the profile instead; allocator contention is
then real on release platforms too and the design needs a different answer.

Expected: unknown until measured on glibc. The local hardware ceiling is ×1.7 at 2
workers and ×2.7 at 4.

### PR 5: release-profile experiment

*Haiku 5.5, after PR 0. Files: `Cargo.toml` only.*

Release archives are built with `[profile.dist]`, which already sets thin LTO. Open
a PR that changes it to `lto = "fat"` with `codegen-units = 1` and read the
Benchmark workflow result, which after PR 0 compares the `dist` profile of base and
head. Merge only if scan rows are separated on ubuntu-26.04. Otherwise close it and
record the result in `reports/perf-round2/README.md`. `strip = "symbols"` is a packaging decision for
PLAN step 9, not part of this experiment.

### PR 6: streaming Cobertura reader (prototype first)

*Opus 5.5 for a research prototype, then a decision. Files: `src/cobertura.rs`.*

The DOM costs 7.6× the report in memory and about half of import CPU on
single-file workloads. A pull parser would hold roughly the report text alone.
This parser is security-sensitive (DOCTYPE, entity and stylesheet rejection, size
and node limits), so:

1. Prototype behind the existing `parse_reports` signature and run it
   differentially against the current importer on every fixture, the generated
   workloads and malformed inputs. Errors must match in kind and message.
2. Measure peak RSS and import time with the harness from PR 0.
3. Go only if import peak memory drops by at least half with identical results.
   A new parsing dependency needs the owner's sign-off with its audit status.

## Running this with subagents

- One worktree and one branch per PR. PRs 2, 3 and 4 never run in the same files at
  the same time.
- Start children one at a time. On the local runner, several children starting
  together time out before doing any work.
- Give each child the PR section above, the patch path, the gate from `PLAN.md`,
  and the runner notes in `reports/perf-round2/README.md`. Children have no other
  context.
- Local timings are for catching regressions only. Children must not quote them as
  results; the PR description quotes the hosted Benchmark summary.
- Each implementation PR gets an independent review from the other model before
  merge, with the review brief restricted to the diff and this plan.

## What this round will not fix

Scan memory for one very large file stays at about 100× its size. That is the
syntax tree, and nothing in this plan shrinks it. Parsing itself, the largest CPU
share, is only attacked by running files in parallel and, later, by not re-parsing
unchanged files (decision 6).
