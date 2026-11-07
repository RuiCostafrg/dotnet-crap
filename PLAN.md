# Next implementation steps

## Starting point

The first implementation batch, PRs #2–#9, is merged into `main` at
`c1c78cd9c0b42df51de96c49ee967fd9e8806078`.

Implemented: expanded C# callable recognition, text/JSON/SARIF reports,
source-hashed native coverage, iterative traversal with tested resource budgets,
terminal-safe diagnostics, dependency auditing, benchmark tooling, and hardened
native/.NET-tool packaging. Three physical C# accuracy fixtures run through the
real SDK with partial/full instrumentation and independently calculated scores.

The final reviewed tree's full Rust mutation run caught 171 of 174 viable
mutants: **98.28%**, with 3 missed, 0 timeouts and 2 unviable.
[Recorded run and evidence](https://github.com/RuiCostafrg/dotnet-crap/actions/runs/42109713977).
This is evidence for that tree, not a guarantee for future changes.

Coverage currently counts supplied points; `--require-coverage` requires evidence
per callable, not complete statement coverage. Same-line nested ownership remains
ambiguous. The real fixtures use controlled instrumentation, not Coverlet.
Automatic release publication remains disabled; tag workflows build artifacts.

All implementation stays in this repository. The TypeScript project remains a
read-only comparison source. See the [feature map](docs/FEATURE_PARITY.md) for
the broader scope and documented exclusions.

## Architecture

Keep one Rust package. Extract cohesive modules when they acquire a consumer;
avoid empty modules or a multi-crate workspace without a concrete need.

- `src/main.rs`: clap arguments, process exits and CLI orchestration.
- `src/lib.rs`: typed scan options, reports and supported public operations.
- `analysis`: syntax traversal, callable ownership and complexity.
- `coverage`: evidence validation and collector adapters.
- `report`: text/JSON/SARIF rendering, then XML and HTML.
- `selection`, `config`, `baseline`: add with their respective feature PRs.

Keep trusted-process execution separate from scanning and pure scoring. Keep
safe output staging and source-alias checks explicit at filesystem boundaries.
The .NET launcher remains an adapter to the Rust binary.

## Ordered PR backlog

Each row is a bounded feature change. Checked items are implemented on the
branch identified below; unchecked items remain proposed. Split a row further
if it cannot be reviewed and validated independently.

### 1. Reusable Rust API and thin CLI — merged in PR #11

- [x] Extract existing analysis, native coverage validation and typed reports
  behind `src/lib.rs`; expose scan and rendering operations without clap types.
- [x] Keep output destinations and exit-code translation in the CLI adapter.
  Preserve source identities needed to reject unsafe report destinations.
- [x] Preserve JSON fields/order, deterministic report ordering, SARIF identities,
  strict threshold behavior and exits 0/1/2.
- [x] Add a library usage example, public API documentation and meaningful
  CLI/library equivalence checks for passing, failing and invalid-input cases.

Implementation: `c80bbd0c4cc8fc4c9239130d07f75ddfd218079d`; additional
review-driven regression tests: `a285bf5`. This work is committed on
`feat/reusable-rust-api`, merged in [PR #11](https://github.com/RuiCostafrg/dotnet-crap/pull/11)
at `e8153b8ec9b09f200da1d48cb065eae6f539a2de`. The API, example and CLI
compatibility tests are documented in [Rust API](docs/RUST_API.md).

Validation: formatting, warnings-denied clippy, locked debug/release tests,
explicit live .NET SDK tests and Python contracts passed. The full production
mutation inventory includes `src/lib.rs`, `src/main.rs` and `src/sarif.rs`, without
exclusions: 186 total, 178 caught, 3 missed, 0 timeouts, 5 unviable; **98.34%** on
`c80bbd0`. [Complete inventory, outcomes and survivor review](reports/reusable-api/README.md).
This measurement does not claim that the later additional tests ran in that full
mutation invocation. A targeted recheck on `a285bf5` catches both library
survivors; the relative-parent mutant remains missed on this host and is
reviewed in the evidence. Benchmarks in both modes are observational on this runner.

**Done when:** existing tests pass, callers can scan without spawning the CLI,
and the complete production mutation inventory covers the extracted modules.
Audit mutation configuration after moving files so no library source is omitted.

### 2. Fresh Coverlet/Cobertura collection — in progress

Depends on step 1. Deliver the mapping contract and collector as reviewable slices.

- [x] Snapshot selected sources before running an explicitly supplied trusted
  test command; verify original bytes again before publishing evidence.
- [x] Collect pinned Coverlet Cobertura output from a fresh per-run directory.
  Reject old output, failed tests, missing reports and changed source.
- [x] Define sequence-point ownership, zero-hit points, multi-assembly merging,
  same-line nesting and missing evidence before changing completeness semantics.
- [x] Reject ambiguous paths; test Windows roots, SourceLink/deterministic paths,
  partial classes, malformed XML and unsafe XML external references.
- [ ] Add real collector fixtures covering partial/full/uncovered code, a
  multi-project solution and collection failures. Verify supported SDK/test-runner
  combinations rather than assuming VSTest and Microsoft Testing Platform match.

Implementation: `dedf03e` (collector/parser plus review fixes). Native v1 remains
unchanged: supplied-line evidence, maximum-hit union across assemblies, zero-hit
retention, and existing same-line nesting limits. Foreign roots, unmapped
SourceLink/deterministic paths, and UNC paths are rejected rather than guessed.
See [the mapping and command contract](docs/COVERLET.md). Live final-tree tests
and the full mutation rerun are in progress; do not interpret the initial
`7aaae57` mutation result as validation of the fixes.

**Done when:** actual .NET collector runs produce independently checked scores;
failures leave no new coverage artifact. Document any versioned format migration.
Add OpenCover/Microsoft coverage adapters in later PRs with their own evidence.

### 3. Versioned configuration and deterministic selection

- [ ] Add strict `dotnet-crap.json` parsing with unknown-field rejection and
  an explicit compatibility policy.
- [ ] Resolve CLI overrides, including explicit false booleans, before scanning.
- [ ] Add include/exclude globs, `.gitignore`/`.ignore` behavior and generated-file
  markers with a documented precedence and root-containment contract.
- [ ] Test nested working directories, overlapping inputs, links, empty results
  and configuration errors.

**Done when:** equivalent invocations from different working directories select
the same intended source set, and configuration cannot bypass coverage validation.

### 4. Git changes, then GitHub PR selection

Depends on step 3; use separate PRs for local Git and remote PR resolution.

- [ ] Select whole changed files against an explicit merge base.
- [ ] Define dirty-worktree behavior and verify bytes against the selected revision.
- [ ] Exercise renames, deletions, shallow history, absent bases and root escapes
  using local Git repositories.
- [ ] Resolve GitHub PR metadata through an authenticated adapter, with clear
  errors and no implicit fetch, checkout or repository mutation.

**Done when:** selection is reproducible and stale revision evidence is rejected.

### 5. Qualified identities and regression baselines

- [ ] Define scope/signature identities for overloads, explicit implementations,
  indexers and anonymous callables before exposing a baseline format.
- [ ] Add versioned baseline generation and comparison with token fingerprints.
- [ ] Fail new/worsened violations; test unchanged, improved, removed, moved and
  ambiguous records. Never silently rewrite a committed baseline.
- [ ] Keep missing required coverage independently enforceable.

**Done when:** moves and overloads cannot silently suppress unrelated violations.

### 6. Modern C# syntax and explicit preprocessor policy

- [ ] Define scoring for top-level statements, advanced patterns, records and
  primary constructors; extend async/iterator fixtures where coverage is missing.
- [ ] Document source-as-written versus compilation-symbol behavior before
  claiming compilation-aware analysis.
- [ ] Add physical SDK-compilable fixtures with hand-derived complexity, nested
  ownership and partial/full coverage expectations for each supported construct.

**Done when:** every newly supported form has independent accuracy evidence and
unsupported/ambiguous cases have explicit diagnostics or documented exclusions.

### 7. Remaining reports and versioned schemas

- [ ] Add XML, then standalone HTML, in separate PRs with escaping/security tests.
- [ ] Add sort/filter/top presentation options without changing gate outcomes.
- [ ] Publish native coverage and report schemas; ship config/baseline schemas
  with those features, including invalid examples and compatibility guidance.
- [ ] Keep SARIF fingerprint limits explicit; coordinate identity improvements
  with step 5 rather than silently changing existing fingerprints.

**Done when:** every format handles empty, passing, failing and invalid-input
cases consistently and generated artifacts validate against their contracts.

### 8. Performance and remaining mutation survivors

Start profiling after step 1; repeat on each relevant production change.

- [ ] Add dense single-file coverage, many-callable, deep-nesting and larger
  solution workloads alongside the existing many-small-files benchmark.
- [ ] Profile aggregate coverage-point visits across deeply nested callables.
  The current traversal budgets bound syntax nodes, not repeated coverage work;
  evaluate prefix counts or excluded-range skipping after measuring dense workloads.
- [ ] Record runtime and peak memory with compiler, binary digest, source commit,
  corpus and individual samples. Keep correctness assertions in the harness.
- [ ] Investigate the 3 recorded surviving mutants; strengthen independent
  assertions where behavior matters and document genuinely equivalent cases.
- [ ] Introduce performance budgets only after controlled, repeated measurements.
  Optimize measured bottlenecks without changing scoring semantics.

**Done when:** comparisons use the same runner/toolchain/corpus, and every
performance claim has reproducible measurements. Do not infer portable limits
or speedups from unrelated shared-runner timings.

### 9. Protected distribution and independent adoption

- [ ] Establish enforceable host-side release protections and trusted publication
  identity before adding a reviewed publishing workflow. Keep publication off
  while the repository plan cannot enforce the necessary protections.
- [ ] Verify NuGet package-ID ownership, immutable assets, checksums/provenance,
  and installed-tool behavior on all five supported release RIDs.
- [ ] Retain fresh-feed packing: reuse an existing package for retries, use a
  new stage for rebuilds, and never substitute normalized archive contents for
  exact published-byte identity.
- [ ] Exercise the setup Action against an actual immutable release; add other
  distribution channels only after the core release path is validated.
- [ ] Evaluate 3–5 independent C# projects at pinned revisions, including a library,
  web application and multi-project solution. Record coverage mapping limitations,
  runtime/memory, scores and integration friction.

**Done when:** supported parity rows have automated evidence or explicit exclusions,
and installation/publication claims reflect actual validated releases.

## Parallel work and integration

The library boundary is merged in PR #11. Then collector work and config/selection work can
run in separate prepared worktrees with agreed API ownership. Syntax fixtures and
benchmark research can proceed independently; coordinate changes to analysis code.
Baselines depend on stable identities; reporting can use the established report API.

Use one feature branch and PR per bounded change. For step 2, Sol owns API integration and independent review; Luna owns live
fixtures, Haiku mapping research, and DeepSeek Flash the parser. Use the explicitly
requested providers/models; report availability limitations without substitution.
Do not let agents edit the same shared files concurrently. Integrate parent changes
before validating a dependent PR; prefer a short stack over a long review chain.

## Required gates for each implementation PR

Run checks appropriate to the changed behavior; documentation-only edits do not
need new runtime tests. For Rust production changes, the standard gate is:

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo test --locked --release
DOTNET_CRAP_E2E=1 DOTNET_CRAP_COVERLET_E2E=1 cargo test --locked
python3 -m unittest discover -s tests -p 'test_*.py'
python3 scripts/mutation-gate.py
```

Require **strictly greater than 90%** caught among caught + missed + timed-out
mutants. Unviable mutants are separate; timeouts are never kills. Retain complete
inventory/outcome evidence for the tested SHA and review survivors. Do not add
blanket exclusions to inflate the score. Run live SDK/collector tests explicitly;
a skipped E2E body is not evidence of a .NET run.

For packaging/workflow changes, also run actionlint, the offline script contracts,
launcher tests and `bash scripts/smoke-packaging.sh` on supported hosts. For
performance-sensitive changes, build release and run both benchmark modes with
recorded source/binary provenance. Add larger workloads where the change demands it.

Before merging: inspect current-head checks and unresolved review findings,
resolve valid issues, verify the combined tree, then merge in dependency order
under the user's existing authorization. Keep release publication a separate
explicitly configured operation. Refresh the feature map and this plan as PRs land.
