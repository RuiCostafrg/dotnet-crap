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
At that historical snapshot, automatic release publication was disabled and tag
workflows only built artifacts. Current first-release status is recorded in
[release integration](docs/RELEASE_INTEGRATION.md): publication is enabled by
configuration, but no release or live published-path verification exists yet.

All implementation stays in this repository. The TypeScript project remains a
read-only comparison source. See the [feature map](docs/FEATURE_PARITY.md) for
the broader scope and documented exclusions.

## Architecture

Keep one Rust package. Extract cohesive modules when they acquire a consumer;
avoid empty modules or a multi-crate workspace without a concrete need.

- `src/main.rs`: clap arguments, process exits and CLI orchestration.
- `src/lib.rs`: typed scan options, `ScanResult`, `scan` orchestration and
  crate-root re-exports of the documented API.
- `analysis`: syntax traversal, callable ownership and complexity.
- `evidence` (native v1), `cobertura` and `collector`: coverage evidence
  validation, import and trusted-process collection.
- `selection` and `source`: source discovery and decoding.
- `report`, `sarif` and `output`: typed reports, text/JSON/SARIF rendering and
  safe report publication; XML and HTML follow.
- `config`, `baseline`: add with their respective feature PRs.

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

### 2. Fresh Coverlet/Cobertura collection — implemented in PR #15

Depends on step 1. Deliver the mapping contract and collector as reviewable slices.

- [x] Snapshot selected sources before running an explicitly supplied trusted
  test command; verify original bytes again before publishing evidence.
- [x] Collect pinned Coverlet Cobertura output from a fresh per-run directory.
  Reject old output, failed tests, missing reports and changed source.
- [x] Define sequence-point ownership, zero-hit points, multi-assembly merging,
  same-line nesting and missing evidence before changing completeness semantics.
- [x] Reject ambiguous paths; test Windows roots, SourceLink/deterministic paths,
  partial classes, malformed XML and unsafe XML external references.
- [x] Add real collector fixtures covering partial/full/uncovered code, a
  multi-project solution and collection failures. Verify supported SDK/test-runner
  combinations rather than assuming VSTest and Microsoft Testing Platform match.

Implementation and validated source/test snapshot: `1cad955`;
[PR #15](https://github.com/RuiCostafrg/dotnet-crap/pull/15).
Native v1 remains unchanged: supplied-line evidence, maximum-hit union across
assemblies, zero-hit retention and existing same-line nesting limits. Foreign
roots, unmapped SourceLink/deterministic paths and UNC paths are rejected.
See [the mapping/command contract](docs/COVERLET.md).

Validation: locked debug/release, formatting, warnings-denied clippy, Python
contracts, actionlint and actual .NET 10 VSTest collection for xUnit, MSTest and
NUnit passed. Linux/macOS/Windows CI and supported Ubuntu packaging smoke passed.
The full source-qualified mutation run tested 297 mutants: 262 caught, 24 missed,
0 timeouts, 11 unviable; **91.61%**, with all five production modules represented
and no blanket exclusions. [Complete evidence, inventory and survivor review](reports/fresh-coverlet/README.md).
SDK 8 and actual MTP collection remain unverified; no parity is inferred from
the negative MTP-mode SDK test. Release publication was disabled for that
validated snapshot; see step 9 for current status.

**Done when:** actual .NET collector runs produce independently checked scores;
failures leave no new coverage artifact. Document any versioned format migration.
Add OpenCover/Microsoft coverage adapters in later PRs with their own evidence.

### 2a. Behavior-preserving architecture boundaries — merged in PRs #17 and #18

Independent audits (Claude Sonnet 5.5 and OpenCode GLM-5.3 Flash) reviewed
module boundaries, errors, process isolation, duplicated path logic and
testability. Ranked by value per review risk:

1. **Split `src/lib.rs` (710 lines, six concerns) into cohesive modules.**
   `analysis` (callable recognition, naming, complexity and the traversal, moved
   verbatim), `evidence` (native v1 schema and validation), `selection` (source
   walk), `source` (decoding), `report` (typed report and text rendering).
   `lib.rs` keeps `scan` orchestration and crate-root re-exports, so the
   documented API paths are unchanged. Rationale: the collector already reaches
   into private lib internals; later config/selection/baseline work needs seams.
2. **Library-owned report writer (`output`).** Move the CLI's alias, symlink,
   `.cs` and staged-persist protections into a documented
   `output::write_report`, sharing the destination-parent and C#-path helpers
   with the collector. `main.rs` becomes a thin clap/exit adapter, and library
   callers no longer need to "reproduce each check".
3. **Separate pure collection planning from process execution.** Extract
   command validation, `{results}` substitution, report discovery and evidence
   assembly from `collector::collect`; `run_command` stays the only process code.
4. **Typed library errors.** Replace `anyhow` in the library with
   `dotnet_crap::Error` (a matchable `ErrorKind` enum plus the existing message
   and `source()` chain). Messages and `{:#}` chains stay byte-identical, so CLI
   stderr is unchanged; the CLI maps every library error to exit 2 as today.
   (`Debug` for `ScanResult`, `Report` and `Item` landed with item 1.)

Deliberately not done: unifying the scanner's and collector's native-coverage
key derivation (`lib.rs` rejects `\` while the collector first rewrites it to
`/`, with different messages; a follow-up should define one contract),
splitting `cobertura.rs` (424 lines but one cohesive
contract), merging the native-evidence and Cobertura path validators (different
contracts and messages), restructuring `scan` error order, or a multi-crate
workspace. Follow-up: `input_paths()` keeps a relative coverage path as
supplied, so `output::write_report` alias checks depend on an unchanged working
directory (documented in the Rust API; resolving it at scan time is an API change).
Step 8's prefix-sum `CoverageIndex` traversal (PR #19) merged first. The merge
of `main` into PR #17 moved it, with its tests, verbatim into `analysis.rs`.
The coverage traversal hot path belongs to step 8; it moves verbatim,
with only its three error constructors adapted in item 4.

Delivery: [PR #17](https://github.com/RuiCostafrg/dotnet-crap/pull/17) (items
1–3, no output change) and stacked
[PR #18](https://github.com/RuiCostafrg/dotnet-crap/pull/18) (item 4, API change
documented in [Rust API](docs/RUST_API.md)). PR #17 merged at `e12c6d5`, after
merging `main` with PR #19 and PR #20. PR #18 merged at `8319728`.
PR #17 passed formatting, warnings-denied clippy and locked tests, and live
.NET 10 Coverlet collection for xUnit/MSTest/NUnit on Linux. `collect` moved
byte-for-byte.
PR #18 head `5745f83` passed the complete standard gate: formatting, clippy,
debug/release/live tests, Python contracts and docs. Its full mutation run, with no
exclusions, tested 323 mutants: 279 caught, 22 missed, 0 timeouts, 22 unviable,
**92.69%**. All 12 production files are represented, and the seven new modules have
no survivors. 21 survivors repeat the reviewed PR #15 cases. The one new survivor
is killed by a test added afterwards (targeted recheck: 7/7 caught).
[Evidence, inventory and survivor review](reports/architecture/README.md). Each PR must pass the
standard gate including live xUnit/MSTest/NUnit Coverlet tests, and the full
mutation inventory must name every new production module.

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

- [ ] Logical-pattern scoring: count each explicit `and`/`or` combinator once,
  preserve comparison/negation and callable ownership rules, and validate
  physical SDK fixtures with independent complexity and partial/full coverage
  expectations. Implementation is in [PR #41](https://github.com/RuiCostafrg/dotnet-crap/pull/41);
  integration and complete validation remain pending. See `docs/CSHARP_PATTERNS.md`.
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
Progress: [step 8 performance evidence](reports/perf-step8/README.md) in [PR #19](https://github.com/RuiCostafrg/dotnet-crap/pull/19).
Next: [round 2 plan](docs/PERFORMANCE_PLAN.md), based on the unmerged
[prototype evidence](reports/perf-round2/README.md).

- [x] Add dense single-file coverage, many-callable, deep-nesting and larger
  solution workloads alongside the existing many-small-files benchmark. The
  harness also replays Coverlet-shaped Cobertura through `collect`.
- [x] Profile aggregate coverage-point visits across deeply nested callables.
  The old scorer's work is recorded deterministically as `span_point_visits`.
  Prefix counts over sorted points replaced per-point span walks, and per-class
  Cobertura resolution and line counting were memoized. Both changes keep outputs
  byte-identical. Sampling profilers were unavailable on the local runner; the
  Benchmark workflow records a Linux `perf` profile.
- [x] Record runtime and peak memory with compiler, binary digest, source commit,
  corpus and individual samples. Keep correctness assertions in the harness.
- [ ] Continue survivor investigation at named source revisions: the initial 3
  cases and the 24 reviewed collector-tree survivors are qualified in their
  evidence. Strengthen independent assertions where behavior matters; do not
  declare uncovered boundaries or platform-specific cases globally equivalent.
- [ ] Introduce performance budgets only after controlled, repeated measurements.
  Optimize measured bottlenecks without changing scoring semantics. No budgets
  yet: the local evidence comes from a shared PRoot runner, and CI timings are
  same-job observations.

**Done when:** comparisons use the same runner/toolchain/corpus, and every
performance claim has reproducible measurements. Do not infer portable limits
or speedups from unrelated shared-runner timings.

### 9. Protected distribution and independent adoption

- [x] Enforce host-side protections matching typescript-crap: rulesets for owner
  review and strict CI on `main`, owner-only and immutable `v*` tags, SHA-pinned
  Actions allowlist, secret scanning with push protection, Dependabot alerts,
  private vulnerability reporting and an owner-approved `release` environment.
  Windows CI runs but is not required. See `docs/SECURITY_PARITY.md`.
- [ ] Integrate the reviewed publishing jobs from PR #16. Trusted publication
  prerequisites and the owner-approved `release` environment are configured, and
  `DOTNET_CRAP_PUBLISH=true`; this enables publication after the tag and approvals,
  but does not prove that publication has happened.
- [ ] Verify NuGet package-ID ownership, immutable assets, checksums/provenance,
  and installed-tool behavior on all five supported release RIDs.
- [ ] Retain fresh-feed packing: reuse an existing package for retries, use a
  new stage for rebuilds, and never substitute normalized archive contents for
  exact published-byte identity.
- [ ] Exercise the setup Action against an actual immutable release; add other
  distribution channels only after the core release path is validated.
- [ ] Distribution channels mirroring typescript-crap (see
  [docs/DISTRIBUTION.md](docs/DISTRIBUTION.md)): cargo-dist 0.33.1 archives for
  macOS and GNU/musl Linux, shell installer, Homebrew formula for
  `RuiCostafrg/homebrew-tap`, npm main + four platform packages, and the
  provenance-verifying GitHub Action. The upstream candidate workflow builds and
  installs it through npm/pnpm/Yarn/Bun, the shell installer, a local Homebrew
  tap and the Action against NUnit and MSTest projects with fresh Coverlet
  Cobertura coverage on Ubuntu and macOS. Separate .NET installed-tool CI
  exercises only `linux-x64`, `osx-arm64` and `win-x64`, although the package
  supports five RIDs; `linux-arm64` and `osx-x64` still need live verification.
  The publication prerequisites are in place (rulesets, `release` environment, npm placeholders
  with stage-only trusted publishing, tap sync in homebrew-tap#18) and
  `DOTNET_CRAP_PUBLISH` is set. Nothing has been published; the first tagged
  release and its published-path verification remain. Candidate CI is green
  at `018d2c006cc1527658715e7c450a5d56d37a3d39`; that is not live proof.
- [ ] Automate NuGet.org publication of the .NET tool (trusted publishing) once
  package-ID ownership is verified.
- [ ] Evaluate 3–5 independent C# projects at pinned revisions, including a library,
  web application and multi-project solution. Record coverage mapping limitations,
  runtime/memory, scores and integration friction.

**Done when:** supported parity rows have automated evidence or explicit exclusions,
and installation/publication claims reflect actual validated releases.

First-release execution order (separate from the feature backlog numbering):

1. Add reviewed, read-only published-verification workflows requiring an explicit
   version and full source SHA, for real GitHub assets/provenance, npm, shell,
   the public tap, the Action and all five .NET tool RIDs. Prepare these before
   integration; successful live runs require the actual publication later.
2. Resolve PR review findings and integrate reviewed distribution, verification
   and documentation changes into `main`. Require successful **push** runs of
   `ci.yml`, `security.yml` and `distribution.yml` at the exact final main SHA,
   as well as protected-branch checks. Candidate PR checks cannot replace these.
3. Only then create `v0.1.0` at that qualified main SHA. Approve `release` for
   GitHub publication, approve it again for npm staging, then approve the four
   platform stages with owner 2FA before the main npm package. Complete tap sync
   and the real published-path checks before changing pending install claims.

The integration owner performs merges and tagging after all workers finish.
[Release integration](docs/RELEASE_INTEGRATION.md) records the audited blockers,
required checks, approval boundaries and evidence still needed.

## Parallel work and integration

The library boundary is merged in PR #11. Then collector work and config/selection work can
run in separate prepared worktrees with agreed API ownership. Syntax fixtures and
benchmark research can proceed independently; coordinate changes to analysis code.
Baselines depend on stable identities; reporting can use the established report API.

Use one feature branch and PR per bounded change. For step 2, Sol owns API
integration and independent review; Luna owns live fixtures, Haiku owns mapping
research, and DeepSeek Flash owns the parser. Use the explicitly
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
