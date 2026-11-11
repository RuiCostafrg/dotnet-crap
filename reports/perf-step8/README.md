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
GitHub's Linux runner and a same-job base/head comparison on Linux and macOS (see
below).

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


## GitHub-hosted runners (Benchmark workflow)

[Benchmark run 42991034793](https://github.com/RuiCostafrg/dotnet-crap/actions/runs/42991034793)
on this PR measured the same base and head in one job per runner. The head is
merge commit `d7c0a6f` (PR head on `main`) and the base is its first parent,
`77003a6`. Both use rustc 1.96.0, 8 alternating runs per row, and byte-identical
outputs. Compare rows only within one runner's table.

### ubuntu-24.04 (AMD EPYC 7763 64-Core Processor, RSS floor 6368 KiB)

| workload | mode | base median ms (min–max) | head median ms (min–max) | head/base | separated | base / head median peak RSS KiB |
|---|---|---|---|---|---|---|
| small-files | scan | 37.1 (36.8–44.6) | 37.5 (37.0–37.9) | 1.01 | no | 7284 / 7386 |
| small-files | scan-coverage | 37.9 (37.7–38.3) | 38.0 (37.7–38.8) | 1.00 | no | 7578 / 7746 |
| small-files | collect | 134.8 (133.6–135.9) | 134.6 (134.2–135.8) | 1.00 | no | 12644 / 12568 |
| dense-file | scan | 279.7 (278.5–283.8) | 279.7 (276.6–296.0) | 1.00 | no | 52208 / 52216 |
| dense-file | scan-coverage | 291.3 (288.7–294.8) | 292.3 (286.0–294.7) | 1.00 | no | 54112 / 54046 |
| dense-file | collect | 963.9 (960.5–969.6) | 730.3 (719.6–749.6) | 0.76 | yes | 56826 / 56776 |
| many-callables | scan | 172.8 (168.3–174.8) | 172.0 (167.5–175.3) | 1.00 | no | 33410 / 33416 |
| many-callables | scan-coverage | 176.4 (174.8–186.1) | 176.8 (173.0–179.1) | 1.00 | no | 34432 / 34272 |
| many-callables | collect | 720.2 (712.3–731.0) | 445.4 (436.0–453.3) | 0.62 | yes | 41818 / 41602 |
| deep-nesting | scan | 70.0 (66.9–72.5) | 70.3 (68.6–70.7) | 1.00 | no | 17056 / 17096 |
| deep-nesting | scan-coverage | 117.4 (115.0–118.0) | 71.5 (70.3–76.5) | 0.61 | yes | 17876 / 18104 |
| deep-nesting | collect | 282.4 (274.8–288.9) | 237.1 (231.0–241.1) | 0.84 | yes | 26420 / 26272 |
| solution | scan | 61.8 (61.1–62.4) | 62.1 (61.6–62.9) | 1.01 | no | 8164 / 8232 |
| solution | scan-coverage | 63.5 (62.9–64.1) | 63.6 (62.7–64.2) | 1.00 | no | 8686 / 8660 |
| solution | collect | 215.6 (203.4–216.9) | 215.2 (205.1–216.2) | 1.00 | no | 9454 / 9562 |

### macos-15 (Apple M1 (Virtual), RSS floor 1120 KiB)

| workload | mode | base median ms (min–max) | head median ms (min–max) | head/base | separated | base / head median peak RSS KiB |
|---|---|---|---|---|---|---|
| small-files | scan | 41.9 (40.6–96.6) | 44.2 (40.4–57.8) | 1.06 | no | 4544 / 4448 |
| small-files | scan-coverage | 43.0 (42.2–45.6) | 42.9 (41.6–45.7) | 1.00 | no | 4880 / 4848 |
| small-files | collect | 220.2 (203.5–269.0) | 215.5 (182.0–268.7) | 0.98 | no | 12280 / 12304 |
| dense-file | scan | 250.3 (233.4–296.5) | 271.9 (234.9–351.8) | 1.09 | no | 51544 / 51480 |
| dense-file | scan-coverage | 251.1 (218.6–339.7) | 257.8 (236.5–291.9) | 1.03 | no | 53608 / 53728 |
| dense-file | collect | 1101.4 (986.5–1237.6) | 835.4 (762.5–922.0) | 0.76 | yes | 92600 / 90176 |
| many-callables | scan | 162.6 (140.6–192.9) | 181.4 (143.6–213.3) | 1.12 | no | 34472 / 34272 |
| many-callables | scan-coverage | 163.9 (144.3–221.0) | 147.7 (140.6–180.5) | 0.90 | no | 35440 / 35616 |
| many-callables | collect | 760.4 (720.8–839.8) | 487.1 (442.3–526.0) | 0.64 | yes | 65264 / 65000 |
| deep-nesting | scan | 59.5 (50.8–65.5) | 55.6 (50.9–90.0) | 0.93 | no | 18960 / 18848 |
| deep-nesting | scan-coverage | 88.1 (80.0–99.1) | 61.9 (56.1–70.5) | 0.70 | yes | 19800 / 19704 |
| deep-nesting | collect | 332.7 (283.7–387.4) | 328.9 (273.0–375.2) | 0.99 | no | 39072 / 34080 |
| solution | scan | 71.7 (65.2–81.9) | 72.1 (67.8–83.4) | 1.01 | no | 5184 / 5136 |
| solution | scan-coverage | 79.9 (69.8–86.7) | 76.9 (67.9–86.1) | 0.96 | no | 5824 / 5824 |
| solution | collect | 332.3 (300.5–421.4) | 325.9 (317.3–449.5) | 0.98 | no | 7144 / 7128 |

On native kernels, path canonicalization is cheap, so the Cobertura speedups are
smaller than under PRoot. They still fully separate for dense-file and
many-callables collect on both runners. Deep nesting with coverage separates at
the default 3000 lambdas on both runners (0.61 on Linux, 0.70 on macOS), and
deep-nesting collect also separates on Linux. No row is separated in the slower
direction. The many-small-files collect paths take 135–330 ms here, against
9–16 s under PRoot, which confirms that the local absolute numbers are syscall-inflated.
No regression in peak RSS separates from noise; the deep-nesting-collect RSS
difference on macOS favors the head.

The [Linux `perf` profile](ci-ubuntu-24.04/profiles/) of the head's frame-pointer build
(`task-clock`, 903 and ~200 samples) attributes the remaining scan time mostly to
tree-sitter: parse, lexing, parse-stack management and cursor traversal.
`dotnet_crap::scan` self time is 2–4%. No further first-party hot path is
indicated at these scales.

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

## Mutation evidence

The PR's Mutation workflow ([run 42991034520](https://github.com/RuiCostafrg/dotnet-crap/actions/runs/42991034520))
ran the full production inventory on `18a7893`. Production source there is
identical to later commits, which change only reports and docs. Results: 312
mutants, 277 caught, 24 missed, 0 timeouts, 11 unviable, so 277/301 = 92.03%
with no exclusions. The 24 survivors are the same set, modulo line numbers, as
the ones reviewed in [fresh-coverlet](../fresh-coverlet/survivor-review.md);
none is in code changed here. All 36 mutants in the new helpers
(`CoverageIndex::count`, `merge_ranges`, `span_counts`, `span_coverage`) are
caught. See [`mutation-ci/audit.json`](mutation-ci/audit.json) and the
retained outcome lists.

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
