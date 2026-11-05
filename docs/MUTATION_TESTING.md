# Mutation testing

Install cargo-mutants 27.1.0 (`cargo install cargo-mutants --version 27.1.0 --locked`), then run `python3 scripts/mutation-gate.py`. The script runs the complete production inventory with two workers, a 120 second test timeout and a 600 second build timeout. It requires strictly more than 90% caught among caught + missed + timed out mutants. Unviable mutants are excluded; timeouts are not kills. Missing or incomplete output fails the gate. The mutation workflow runs this same command. Each invocation writes to a unique `target/mutation-runs/run-*` directory, so concurrent runs cannot mix evidence. The complete invocation output is retained and uploaded by CI. Generated output is ignored by Git.

## Qualified historical measurements

On quality head `318889605edb3e8258c38d0c29d862895bef688b`, the completed run reported 112 total: 109 caught, 1 missed, 0 timed out, and 2 unviable (99.09%). [Run 41932905187](https://github.com/RuiCostafrg/dotnet-crap/actions/runs/41932905187).

The combined run on `a08eae6184a8e4a50b5a8314a94c7cced58491db` reported 142 total: 139 caught, 1 missed, 0 timed out, and 2 unviable (99.29%). [Run 41934477734](https://github.com/RuiCostafrg/dotnet-crap/actions/runs/41934477734). These are historical results and do not establish the score for later commits.

## Initial measurement (incomplete)

On parent commit `08a1f55`, cargo-mutants 27.1.0 listed 112 mutants in `src/main.rs`. A baseline run using `cargo mutants --jobs 2 --jobserver-tasks 2 --timeout 120 --build-timeout 180 -- --locked` passed its unmutated baseline (169 seconds build, 3 seconds test). On the resource-constrained Android/aarch64 host, after approximately 308 seconds, five mutants had completed: 3 caught, 1 missed, 0 timeout, 1 unviable; 107 remained. The run was interrupted because of its projected runtime. This is not a full score and cannot establish whether the gate passes. The 180 second build timeout was close to the clean build time, so the gate uses 600 seconds instead.

The missed mutation removed the diagnostic describing the expected coverage map; `tests/mutation_contracts.rs` now checks this error. Its effect on the mutation score has not been measured. The one observed unviable mutation replaced `unique_map` with a map containing a default value, which cannot compile for the unconstrained generic value type. No other unviable mutants have been assessed. Run the full gate on the feature commit before drawing a score conclusion; later feature branches will require their own measurement.

## Full CI baseline and hardening

[The first full CI run](https://github.com/RuiCostafrg/dotnet-crap/actions/runs/41931454456)
on commit `c6c40e07c3f3ecb5230aab7859c59c56ef156473` completed all 112 mutants:
81 caught, 29 missed, 2 unviable, and no timeouts. The gate correctly failed.
This supersedes the interrupted local measurement above. CI logs identify each
survivor; the workflow now also preserves the complete `mutants.out` artifact on
success or failure, including per-mutant logs and inventory.

The follow-up behavioral corpus independently checks callable line/column spans,
coverage boundaries, nested callable ownership, positive hit counts, initialized
auto-property omission, switch-arm complexity, each malformed coverage path/hash/
line condition, nested source selection, existing relative report replacement,
and conditional text diagnostics. No production mutations are excluded.
A fresh full CI result is required before claiming the greater-than-90% target.

The dependency-security workflow separately runs cargo-audit 0.22.2 against the
committed lockfile and denies advisory warnings as well as vulnerabilities. A
passing mutation score is not a substitute for that audit or code review.

## Evidence paths and retention

Each invocation reserves `target/mutation-runs/run-<random>/`; cargo-mutants writes
its inventory, outcomes and logs in the `mutants.out/` child. CI uploads the
`target/mutation-runs/run-*/` artifact tree. Local runs retain the latest five
completed invocation directories by default; `--keep-runs N` adjusts this positive
limit. Current and active runs are never pruned. Interrupted runs without the
completion marker are retained for investigation and can be removed manually
once their processes are stopped. CI artifacts follow GitHub retention settings;
copy important evidence to a durable report before that retention period ends.

Only a complete, identity-validated inventory receives the `.complete` marker.
A complete score below the threshold is still valid evidence and is retained as
such. Baseline, launch and malformed/incomplete-output failures receive `.failed`
and `failure.txt` diagnostics instead. These failure directories have their own
retention pool, so a failed attempt cannot evict the latest validated run even
with `--keep-runs 1`. Concurrent pruning tolerates an already-removed directory
but surfaces other filesystem failures. Newer completed runs may retire older
completed evidence according to the configured retention limit; active runs are
never candidates for cleanup.
