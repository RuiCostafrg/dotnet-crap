# Parity implementation batches

All production changes belong to `RuiCostafrg/dotnet-crap`. The sibling
TypeScript repository is a read-only behavioral reference, inspected at commit
`4d86589163270ab81cd3b0603a27d558f17bfe04` on 2026-11-04. The detailed
[feature map](FEATURE_PARITY.md) remains the acceptance checklist. This document
turns that map into reviewable implementation batches; it does not claim parity.

## Architecture and repository boundaries

Keep the Rust application and .NET packaging in this repository. Preserve
`src/main.rs` as the eventual thin clap/process adapter, extracting cohesive
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

The initial batch is implemented as separate, unmerged PRs. The branch stack is
[#2 plan/performance](https://github.com/RuiCostafrg/dotnet-crap/pull/2), then
[#3 mutation gate](https://github.com/RuiCostafrg/dotnet-crap/pull/3), then
[#4 SARIF](https://github.com/RuiCostafrg/dotnet-crap/pull/4), then
[#5 C# callables](https://github.com/RuiCostafrg/dotnet-crap/pull/5).
The final branch includes the complete batch so its CI mutation job evaluates all
production sources together. Per-feature historical measurements remain evidence
for their recorded SHAs; they do not substitute for this combined run. No PR is
merged automatically. Subsequent batches below remain planned, not implemented.

## Subsequent feature PRs, in dependency order

Each numbered item is a separate proposed PR, not an implemented feature.

5. **Reusable API:** extract the existing scanner and typed report into a library;
   keep JSON/exit contracts unchanged. Public API examples and CLI equivalence
   checks must pass before downstream additions depend on it.
6. **Fresh Coverlet coverage:** snapshot source before a trusted test process,
   import explicitly selected Cobertura, verify source afterwards, reject stale or
   ambiguous mappings, and publish atomically only on success. Exercise actual
   pinned .NET collector fixtures, failures, multiple assemblies and uncovered code.
   OpenCover/Microsoft formats follow as separate PRs with their own fixtures.
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
`DOTNET_CRAP_E2E=1 cargo test --locked`. Run mutation tests on each final production
change and again on the integrated head; a result for the foundation does not
certify later feature branches. Mutation score and code coverage are different
metrics. A test returning early without `DOTNET_CRAP_E2E` is not live .NET evidence.

Record performance with `python3 scripts/benchmark.py --output target/bench.json`
and repeat with `--coverage`. Compare the same compiler/profile/corpus/runner;
shared-runner results are observational and do not establish a portable latency SLA.
The harness checks that the complete expected corpus was scanned and exits nonzero
for subprocess failures, invalid output, or an explicitly exceeded median budget.
