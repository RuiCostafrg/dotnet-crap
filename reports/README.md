# Initial performance evidence

`benchmark-scan.json` and `benchmark-coverage.json` record whole-process timings
for production commit `9065454` using the harness added in this PR. These are
observations on this session's Android/aarch64 runner, not Linux release numbers
or a cross-platform performance promise. They include process startup, filesystem
walk, parsing, scoring, serialization and pipe output; fixture generation and JSON
assertion time are excluded. The corpus is 500 small files with two callables each.
This corpus does not exercise many callables and coverage points in a single file.

Build: `cargo build --locked --release`. Commands:

```sh
python3 scripts/benchmark.py --output reports/benchmark-scan.json
python3 scripts/benchmark.py --coverage --output reports/benchmark-coverage.json
```

The exact executable digest and every measured sample are in each JSON file.
Do not compare unrelated runner timings to establish a regression. No production
optimization or latency improvement is claimed by this initial measurement.

Baseline validation: locked debug/release tests, rustfmt, clippy with warnings
denied, and debug tests with `DOTNET_CRAP_E2E=1` passed. The budget failure path was
also exercised with three files/runs and a deliberately unattainable positive
budget; it returned exit 1. Mutation evidence is owned by its separate quality PR.

## Combined feature batch

`benchmark-combined-scan.json` and `benchmark-combined-coverage.json` were produced
with the same commands and corpus after stacking the mutation gate, SARIF renderer,
and expanded C# callable recognition. Their `production_commit` identifies the
complete tested Rust source tree; subsequent changes in this PR are documentation,
evidence and additional tests. Every scan processed the full expected corpus.

These session measurements have substantial host variability and were not run
as an interleaved controlled A/B experiment. They do not establish a speedup or
regression budget. No performance optimization is claimed. Profile dense
single-file coverage workloads before replacing the current per-callable coverage
filter; this corpus primarily measures common scans over many small files.
