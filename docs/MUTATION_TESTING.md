# Mutation testing

Install cargo-mutants 27.1.0 (`cargo install cargo-mutants --version 27.1.0 --locked`), then run `python3 scripts/mutation-gate.py`. The script runs the complete production inventory with two workers, a 120 second test timeout and a 600 second build timeout. It requires strictly more than 90% caught among caught + missed + timed out mutants. Unviable mutants are excluded; timeouts are not kills. Missing or incomplete output fails the gate. The mutation workflow runs this same command. Generated output is ignored by Git.

## Initial measurement (incomplete)

On parent commit `08a1f55`, cargo-mutants 27.1.0 listed 112 mutants in `src/main.rs`. A baseline run using `cargo mutants --jobs 2 --jobserver-tasks 2 --timeout 120 --build-timeout 180 -- --locked` passed its unmutated baseline (169 seconds build, 3 seconds test). On the resource-constrained Android/aarch64 host, after approximately 308 seconds, five mutants had completed: 3 caught, 1 missed, 0 timeout, 1 unviable; 107 remained. The run was interrupted because of its projected runtime. This is not a full score and cannot establish whether the gate passes. The 180 second build timeout was close to the clean build time, so the gate uses 600 seconds instead.

The missed mutation removed the diagnostic describing the expected coverage map; `tests/mutation_contracts.rs` now checks this error. Its effect on the mutation score has not been measured. The one observed unviable mutation replaced `unique_map` with a map containing a default value, which cannot compile for the unconstrained generic value type. No other unviable mutants have been assessed. Run the full gate on the feature commit before drawing a score conclusion; later feature branches will require their own measurement.
