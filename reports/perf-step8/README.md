# PLAN step 8: performance evidence (Linux userland on aarch64)

All numbers here come from one runner, one toolchain and one generated corpus per
row. Each row is a single `scripts/benchmark.py --baseline` invocation that ran
both binaries in alternating ABBA order on identical inputs and required
byte-identical output on every run. Do not compare these numbers with other
runners, other toolchains or the historical `reports/benchmark-*.json` files.

- **Runner:** Android 12, Qualcomm KONA (8 × aarch64), Ubuntu 26.04 userland under
  PRoot. The machine was shared with other concurrently running build and test
  sessions, so absolute times are noisy. PRoot intercepts every syscall through
  ptrace, which makes syscall-heavy paths (path canonicalization, file reads)
  much slower than on a native kernel. The PRoot load average is synthetic.
- **Toolchain:** rustc 1.96.0 (aarch64-linux-android host), `cargo build --locked --release`.
- **Baseline:** `59136ea` (production source identical to `main` at `77003a6`),
  binary `e938e2cc…`. **Candidate:** `01d756c`, binary `14d26c4a…`. Full digests,
  `rustc -vV` and the exact commands are in
  [`linux-aarch64/provenance.json`](linux-aarch64/provenance.json). Every sample is
  in the per-row JSON files.
- **Claim rule:** a speedup is claimed only when every candidate sample beats
  every baseline sample in the same invocation (`separated = yes`). Every other
  row is "no measurable difference on this runner", whatever its median ratio.

## Profiling

`perf_event_open` fails with `EACCES` on this runner (Android SELinux under
PRoot), so `perf`, `samply` and `cargo flamegraph` cannot sample here. The
evidence instead comes from three sources:

1. **Deterministic work counts.** `span_point_visits` is the number of coverage
   points inside every callable's full span, which is exactly the per-point
   visits the old scorer made. The `deep-nesting` default (3000 lambdas around
   2000 statements) needs 15.0M visits for 8000 points. At 12000 lambdas it
   needs 168M.
2. **Targeted timing.** The Cobertura importer was instrumented temporarily
   (instrumentation removed) on a single file with 2000 `<class>` records.
   Per-class `resolve_filename` (canonicalization) took 2.8–5.6 s, and per-class
   `lines().count()` over the whole source took 1.3–1.5 s. The import took
   ~7.1 s before the fix and ~66 ms after.
3. **In-process phase split.** See
   [`phase-breakdown-head.txt`](linux-aarch64/phase-breakdown-head.txt). For
   `dense-file` at 8000 methods (1.1 MB, 355k named nodes), tree-sitter parsing
   is 55–65% of the in-process scan and one bare tree walk is ~70 ms. The
   remaining scan cost is dominated by the third-party parser, so no further
   scan optimization was attempted without a sampling profile.

The `Benchmark` workflow records a `perf` profile of a frame-pointer build on
GitHub's Linux runner (`target/profiles/*.txt` artifact), and a same-job base/head
comparison on Linux and macOS.

## Changes

- **Coverage counting (`src/lib.rs`).** Each file's points are indexed once as
  sorted lines plus a prefix sum of positive hits. A callable's (total, hit)
  count is its span count minus each merged, clipped nested-callable range, using
  two binary searches per range. Before, the scorer walked every point in the
  span, so cost grew as O(depth × points). The counts are the same integers, so
  every ratio and score is bit-identical.
- **Cobertura import (`src/cobertura.rs`).** Successful filename resolutions are
  memoized per report, and line counts per resolved source. Failures are never
  cached, so the same class reports the same error.

## Same-runner comparison (default scales, 12 runs each)

| workload | mode | base median ms (min–max) | head median ms (min–max) | head/base | separated | base / head median peak RSS KiB |
|---|---|---|---|---|---|---|
| small-files | scan | 1678 (969–2904) | 1672 (1119–1966) | 1.00 | no | 10326 / 10298 |
| small-files | scan-coverage | 1875 (963–2460) | 1606 (903–7213) | 0.86 | no | 10730 / 10744 |
| small-files | collect | 9320 (7193–11672) | 8790 (7564–10839) | 0.94 | no | 17528 / 17396 |
| dense-file | scan | 697 (659–1068) | 753 (605–875) | 1.08 | no | 66326 / 66312 |
| dense-file | scan-coverage | 800 (627–1029) | 771 (650–1319) | 0.96 | no | 68960 / 68636 |
| dense-file | collect | 5251 (4800–6367) | 1460 (1357–1553) | 0.28 | yes | 102834 / 102784 |
| many-callables | scan | 451 (423–652) | 530 (431–626) | 1.17 | no | 47344 / 47364 |
| many-callables | scan-coverage | 496 (398–746) | 485 (452–546) | 0.98 | no | 48576 / 48408 |
| many-callables | collect | 9702 (8114–12244) | 1063 (874–1349) | 0.11 | yes | 74962 / 74896 |
| deep-nesting | scan | 266 (233–437) | 259 (182–478) | 0.97 | no | 28468 / 28426 |
| deep-nesting | scan-coverage | 328 (255–448) | 273 (217–682) | 0.83 | no | 29628 / 29464 |
| deep-nesting | collect | 581 (486–1188) | 555 (498–988) | 0.96 | no | 40916 / 40832 |
| solution | scan | 2892 (1782–4404) | 2587 (1392–6117) | 0.89 | no | 11638 / 11594 |
| solution | scan-coverage | 2493 (1820–3425) | 2580 (1664–3359) | 1.03 | no | 12168 / 12182 |
| solution | collect | 15544 (12420–18838) | 15284 (13415–17061) | 0.98 | no | 12444 / 12426 |

## Scaling series (8 runs each)

| series | scale | span-point visits | base median ms (min–max) | head median ms (min–max) | head/base | separated | base / head median peak RSS KiB |
|---|---|---|---|---|---|---|---|
| deep-nesting-scan-coverage | 3000 | 15011002 | 340 (264–417) | 288 (236–455) | 0.85 | no | 29658 / 29460 |
| deep-nesting-scan-coverage | 6000 | 48020002 | 603 (511–679) | 383 (367–514) | 0.64 | no | 46814 / 46584 |
| deep-nesting-scan-coverage | 12000 | 168038002 | 1207 (1121–1552) | 648 (576–866) | 0.54 | yes | 71264 / 78776 |
| dense-file-collect | 2000 | 8000 | 2375 (1666–4091) | 823 (692–1056) | 0.35 | yes | 56544 / 56550 |
| dense-file-collect | 4000 | 16000 | 5233 (4774–7792) | 1423 (1343–2450) | 0.27 | yes | 102812 / 102756 |
| dense-file-collect | 8000 | 32000 | 12371 (11890–13847) | 2848 (2563–3526) | 0.23 | yes | 130200 / 129476 |


## Interpretation

- **Cobertura import:** collect on a dense file with many Coverlet `<class>`
  records is about 3.6× faster at 4000 methods (500 classes) and 9× faster for
  `many-callables` (1000 classes). The baseline grows superlinearly (2.4 → 5.2 →
  12.4 s per doubling) and the candidate roughly linearly (0.8 → 1.4 → 2.8 s).
- **Nested callables:** with coverage, the gap widens with depth (0.85 → 0.64 →
  0.54) and is fully separated at 12000 lambdas. At the default 3000 lambdas the
  quadratic term is too small to separate from runner noise.
- **No measurable difference** in any other row, including all scan modes for
  small-files, dense-file, many-callables and solution. These changes do not
  touch parsing or the directory walk.
- **Peak RSS.** Every row is within 1% except `deep-nesting` with coverage at
  ≥12000 lambdas, where the candidate peaks 3–10 MB higher (71.3 vs 78.8 MB at
  12000 in the series above). Follow-up runs at 15000 lambdas
  (not committed) explain it. Without coverage both binaries peak at 90.7 MB. With
  coverage the candidate adds ~4.5 MB (parsed coverage plus the index), while the
  baseline drops to 85.5 MB, below its own no-coverage run. So the gap reflects
  the baseline's allocator layout on this Android/bionic runner, not an extra
  growth term. Confirm on glibc/macOS before generalizing either way.
- **Observation, not acted on.** `collect` for many small files (small-files,
  solution) is slow in absolute terms here (9–16 s). It is linear and dominated by
  per-file syscalls (canonicalization, repeated source verification), which PRoot
  inflates. Measure it on a native runner before deciding whether it needs work.

## Budgets

None are introduced. These measurements come from a shared, noisy, non-native
runner. Budgets need repeated measurements on a controlled runner first (PLAN
step 8). `--max-median-ms` remains an opt-in local tool.

## Comparison with typescript-crap

typescript-crap (`benches/scan.rs`, `benches/review.rs`) runs in-process
`cargo bench` binaries (`harness = false`) with one warmup and 5 samples, printing
median/min/max. Its RSS is the process-wide `VmHWM` (Linux only), its stages are
separated in `review.rs`, and it has no wall-clock assertions. This harness
instead measures the whole released CLI process. It records every sample and the
CLI's own `wait4` peak RSS on Linux and macOS, plus compiler, binary digest,
source commit and corpus digest. It validates every report against
independently derived scores, and alternates base and head in one invocation with
byte-identity checks. We adopted typescript-crap's two habits: warm up, then
report all samples with no timing gates; and use stage-separated timing for
diagnosis (the phase split above). We did not adopt in-process timing, because it
hides process start-up, I/O and serialization, which users pay.
