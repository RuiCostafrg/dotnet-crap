# Reusable API validation evidence

Tested revision: `c80bbd0c4cc8fc4c9239130d07f75ddfd218079d` (reusable API). The source snapshot was copied before the commit, with `src/` and `tests/` matching this commit.

Validation commands passed for the implementation and were repeated after the
review-driven test additions:

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo test --locked --release
DOTNET_CRAP_E2E=1 cargo test --locked
python3 -m unittest discover -s tests -p 'test_*.py'
```

The E2E run used .NET SDK 10.0.109 and executed the real physical C# accuracy
and controlled-instrumentation fixtures. It is not evidence of Coverlet support.

## Mutation gate

Collected from the already-running `python3 scripts/mutation-gate.py` process (PID 24924), using `target/mutation-runs/run-jljq0ifv/mutants.out`; no replacement mutation run was launched. The completed gate log reports 186 mutants tested in 40 minutes, with no exclusions: 178 caught, 3 missed, 0 timed out, and 5 unviable. `scripts/mutation-gate.py` `read_inventory` and `check_score` imports validated that the disjoint outcome lists account for all 186 inventory entries. Score: **98.34%** (`178 / (178 + 3 + 0)`), above the gate's strict 90% threshold. The gate completed successfully.

Inventory composition: 161 `src/lib.rs`, 17 `src/sarif.rs`, and 8 `src/main.rs` mutants. The complete inventory and outcome identity lists are preserved in `mutants.json`, `caught.txt`, `missed.txt`, `timeout.txt`, and `unviable.txt` in this directory.

### Missed mutant review

- `src/lib.rs:411:26`, `replace <= with > in collect`: reverses the condition that merges overlapping nested-callable line intervals. It can also merge disjoint sibling spans across parent-owned lines, incorrectly dropping the parent coverage in the gap. With overlapping spans, ranges can remain overlapping rather than coalescing. The existing tests did not expose that case.
- `src/lib.rs:429:20`, `replace > with >= in collect`: changes the `total > 0` guard to `total >= 0`, so an empty set of eligible coverage lines is treated as a present coverage ratio and computes `0.0 / 0.0` (NaN). The intended behavior is `None`; downstream output or calculations can therefore be invalid for that input. The existing tests did not expose it.
- `src/main.rs:83:25`, `delete ! in run`: changes the parent-path filter so an output filename with no directory component selects an empty parent path instead of falling back to `.`. For a relative output such as `report.json`, temporary-file creation can fail rather than writing in the current directory. The existing relative-output regression already exercises this case. The mutation survived on this runner, so empty-parent staging succeeds here; this is a portability-sensitive behavior, not a demonstrated failure on the tested host.

Production code was unchanged through `a285bf5`. Review-driven tests in
`a285bf5` add sparse nested ownership and empty coverage evidence assertions,
coverage-input/generated-source alias and symlink protections, exact JSON field
order and SARIF fingerprint expectations, immutable captured policy, library
selection, and infinite-threshold validation. The full score above remains tied
to the earlier test tree; a targeted survivor recheck is recorded separately below.

### Targeted recheck after regression tests

On `a285bf5`, the following deliberately filtered run tested only the three
previous survivors (not a new full mutation score):

```sh
cargo mutants --no-config \
  -F 'src/lib.rs:411:26: replace <= with >|src/lib.rs:429:20: replace > with >=|src/main.rs:83:25: delete !' \
  --output target/api-survivor-recheck --jobs 2 --jobserver-tasks 2 \
  --timeout 120 --build-timeout 600 -C --locked
```

The unmodified baseline passed. Both library mutants were caught by the new
coverage tests. The relative-parent-path mutant remained missed on this host;
the existing test already successfully writes a bare `report.json` with that
mutation. This runner therefore does not distinguish the empty parent from `.`.
Do not assume the same behavior on other release platforms or exclude the
mutant globally. Exit 2 was the expected cargo-mutants result for this targeted
run containing a survivor. There were no timeouts or unviable mutants in this
subset. Complete subset inventory, outcomes and run output are in
[`survivor-recheck/`](survivor-recheck/). The historical full-run score is not
recalculated from these filtered results.

## Benchmark artifacts

`api-benchmark.json` and `api-benchmark-coverage.json` record fresh runs after
`cargo build --locked --release` on `a285bf5` (production sources unchanged from
`c80bbd0`). Both record Android-12 aarch64, 500 files, 1,000 callables, and 7
individual samples. The fixed binary SHA-256 is `de0a8668cf4da005909e870cff26eee7ae0713c8a72958ba538f37b1baa6c0d4`.
The initial samples overlapped a release rebuild and were replaced rather than
attributed to an uncertain source tree. No release rebuild ran during these new
measurements. Other isolated mutation builds and debug/live SDK tests could still
contend for resources: these measurements are observations only, with no
performance comparison or speedup claim. Commands, compiler and source/binary
provenance are preserved in `benchmark-provenance.json`.

Toolchain at evidence collection: `rustc 1.96.0 (ac68faa20 2026-05-25)`, host `aarch64-linux-android`, LLVM 21.1.8; `dotnet --version` = `10.0.109`.


## Inventory audit and remaining limits

No `.cargo/mutants.toml`, source-file filters, or blanket exclusions were used.
The complete inventory explicitly includes all three production files. This is
an audit of this tree; the existing gate does not automatically reject a future
configuration that silently shrinks the source set. Repeat the audit when moving
or adding production modules. Enforcing inventory coverage in the gate itself is
follow-up tooling work.

The five unviable mutants replace public return values with defaults that do not
exist for the returned types, or construct a generic coverage value without a
`Default` bound. They are not counted as caught. No timeout is counted as a kill.
Full per-mutant logs remain in the original ignored local evidence directory;
this report preserves the complete inventory and all outcome identities.


## PR review follow-up

The later review fix removes one redundant CLI source verification immediately
before stdout (the common pre-output verification and post-staging verification
remain). The runnable example now verifies after rendering, escapes diagnostic
controls and reports I/O errors. Library analysis and coverage semantics did not
change. The local full run and targeted subset above remain measurements of
their named revisions; the final PR head requires its own green CI mutation run.

The dense nested-coverage visit cost is inherited from the scanner before this
extraction: syntax budgets do not bound aggregate coverage visits. It is recorded
in PLAN.md step 8 for measured profiling and range/prefix-count optimization,
rather than introducing an unmeasured algorithm change in the API extraction.
