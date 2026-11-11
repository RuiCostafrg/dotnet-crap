# Parity implementation batches

The initial implementation batch is merged. See [PLAN.md](../PLAN.md) for the
current ordered backlog, acceptance criteria and integration gates. This document
retains the original batch design and broader milestone context.

All production changes belong to `RuiCostafrg/dotnet-crap`. The sibling
TypeScript repository is a read-only behavioral reference, inspected at commit
`4d86589163270ab81cd3b0603a27d558f17bfe04` on 2026-11-04. The detailed
[feature map](FEATURE_PARITY.md) remains the acceptance checklist. This document
turns that map into reviewable implementation batches; it does not claim parity.

## Architecture and repository boundaries

Keep the Rust application and .NET packaging in this repository. Preserve
`src/main.rs` as the thin clap/process adapter, extracting cohesive
`analysis`, `coverage`, `selection`, `config`, `baseline`, and `report` modules
behind `src/lib.rs`. Define scan options and typed reports at the library boundary;
keep filesystem/process policy outside pure scoring and rendering. Extract modules
with their first consumer rather than adding empty crates or a workspace hierarchy.
The .NET launcher remains a process adapter, never a second scoring engine.

Use separate feature branches and PRs against the existing Rust foundation.
Independent PRs must not depend on uncommitted work in another checkout. Rebase
and rerun combined checks before integration, especially changes to `main.rs`.
Do not merge feature PRs automatically.

## Initial parallel batch

1. **C# callable completeness:** operators, conversions, destructors, and indexers,
   including block/expression bodies and nested callable ownership. Add independent
   expected-complexity fixtures. Leave semantic compilation and primary-constructor
   policy explicit rather than guessing runtime behavior from syntax.
2. **SARIF diagnostics:** add `--format sarif`, stable score/completeness rule IDs,
   encoded relative source URIs, valid regions, deterministic fingerprints, and
   passing/empty reports. Preserve existing gate and safe-output behavior. Test
   Unicode/escaping and completeness-only failures at the public command boundary.
3. **Mutation quality gate:** run the full Rust production-source mutation inventory,
   strengthen behavioral assertions for surviving mutants, and publish commands,
   counts, and exclusions. Target strictly greater than 90% killed viable mutants;
   report timeouts separately and never count them as killed. No blanket source
   exclusions to inflate the result. Add a reproducible CI gate.
4. **Performance evidence:** deterministic generated C# corpus with and without
   source-bound coverage, release binary hash, warmup, individual samples, and
   whole-process timing. Make budgets explicit and opt-in until a controlled CI
   runner establishes a repeatable baseline.

## Initial batch review stack

The initial batch was implemented as separate PRs. The original branch stack was
[#2 plan/performance](https://github.com/RuiCostafrg/dotnet-crap/pull/2), then
[#3 mutation gate](https://github.com/RuiCostafrg/dotnet-crap/pull/3), then
[#4 SARIF](https://github.com/RuiCostafrg/dotnet-crap/pull/4), then
[#5 C# callables](https://github.com/RuiCostafrg/dotnet-crap/pull/5).
PRs #2–#9 are now merged into `main` at `c1c78cd`. The final reviewed tree's full
mutation run caught 171 of 174 viable mutants (98.28%), with 3 missed, no timeouts
and 2 unviable. Historical measurements remain qualified to their recorded SHAs.
Tag workflows build artifacts; automatic publication remains disabled. The reusable
API batch below is now merged in PR #11;
remaining batches stay planned.

## Subsequent feature PRs, in dependency order

Each numbered item is a separate bounded change. Status is explicit below;
unmarked items remain proposed.

5. **Reusable API — merged in PR #11:** commit `c80bbd0`
   extracts the scanner, coverage validation and typed reports behind `src/lib.rs`.
   The CLI owns destinations, staging and exits. The example and CLI/library
   equivalence tests preserve JSON order, SARIF identities and gate behavior;
   review-driven tests were added in `a285bf5`. Debug/release, live SDK, Python,
   formatting and clippy checks passed. Full mutation evidence for `c80bbd0`:
   178 caught, 3 missed, 0 timeouts, 5 unviable (98.34%). See the
   [evidence and survivor review](../reports/reusable-api/README.md).
   A targeted recheck catches the two library survivors; the remaining output
   parent-path survivor is qualified to this runner in the report.
   Merged at `e8153b8ec9b09f200da1d48cb065eae6f539a2de`; historical evidence above
   remains qualified to its named source revisions.
6. **Fresh Coverlet coverage — implemented in PR #15:** snapshot source before a trusted test process,
   import explicitly selected Cobertura, verify source afterwards, reject stale or
   ambiguous mappings, and publish atomically only on success. Exercise actual
   pinned .NET collector fixtures, failures, multiple assemblies and uncovered code.
   Validated source/test snapshot `1cad955` includes xUnit/MSTest/NUnit fixtures.
   Full live CI passed on Linux/macOS/Windows with .NET 10/VSTest; complete
   mutation evidence records 262 caught, 24 missed, 0 timeouts and 11 unviable
   out of 297, scoring 91.61% (262/286 viable mutants; the 11 unviable are
   excluded from the denominator) without exclusions. See the
   [source-qualified evidence and survivor review](../reports/fresh-coverlet/README.md).
   The [mapping contract](COVERLET.md) retains native v1 line evidence and
   rejects unsupported remaps/UNC paths. VSTest uses pinned Coverlet 6.0.4; MTP
   parity is not inferred. OpenCover/Microsoft formats follow as separate PRs
   with their own fixtures.
7. **Selection policy:** strict versioned config and include/exclude/ignore behavior,
   followed by Git merge-base selection and then GitHub PR resolution. Explicit CLI
   booleans override config. Test dirty trees, shallow history, renames, deletions,
   root containment and operation from different working directories. Never fetch
   or change checkout implicitly.
8. **Baselines:** versioned generation/comparison with qualified overload identities
   and token fingerprints. Fail on new/worsened violations; test moves and ambiguity.
   Coverage completeness cannot be suppressed by an existing score baseline.
9. **Remaining reports:** XML, standalone HTML, then display sorting/filter/top.
   Rendering options never change gate results. Add schemas and escaping fixtures.
10. **Modern syntax and resource bounds:** top-level code, patterns, records,
    primary constructors and a documented preprocessor policy; iterative traversal
    with depth/resource stress tests and measured profiling before optimization.
11. **Release/adoption:** schemas, dependency/workflow audits, provenance and installed
    tool checks on every supported RID; independent C# project adoption at pinned
    revisions. Do not advertise a distribution channel before validating publication.

## Evidence required before integration

Run formatting, clippy with warnings denied, locked debug/release tests and
`DOTNET_CRAP_E2E=1 DOTNET_CRAP_COVERLET_E2E=1 cargo test --locked`. Run mutation tests on each final production
change and again on the integrated head; a result for the foundation does not
certify later feature branches. Mutation score and code coverage are different
metrics. A test returning early without `DOTNET_CRAP_E2E` or the Coverlet-specific
`DOTNET_CRAP_COVERLET_E2E=1` is not live .NET/collector evidence.

Record performance with `python3 scripts/benchmark.py --output target/bench.json`
and repeat with `--coverage`. Compare the same compiler/profile/corpus/runner;
shared-runner results are observational and do not establish a portable latency SLA.
The harness checks that the complete expected corpus was scanned and exits nonzero
for subprocess failures, invalid output, or an explicitly exceeded median budget.
