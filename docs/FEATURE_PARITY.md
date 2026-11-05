# Feature parity roadmap

Reference: [typescript-crap](https://github.com/RuiCostafrg/typescript-crap/tree/4d86589163270ab81cd3b0603a27d558f17bfe04)
2.3.2, inspected 2026-11-04. This is a plan for equivalent C# workflows, not a
claim that the initial release has full parity. Node, V8, Jest, Vitest, and npm
are replaced by .NET workflows rather than ported literally.

## Feature map

| TypeScript capability | .NET equivalent | Initial PR / next milestone |
| --- | --- | --- |
| Rust Tree-sitter scanner | C# Tree-sitter scanner | Implemented for the callable subset below |
| CC and CRAP, strict threshold, 0/1/2 exits | Same formula and gate conventions | Implemented |
| Root and multiple path selection | `.cs` files, deduplication, root containment | Implemented; bin/obj/.git and generated suffixes excluded |
| Ignore rules, config include/exclude | `.gitignore`, `.ignore`, `dotnet-crap.json` | Phase 3 |
| Source-hashed native coverage | Version 1 path/hash/line map | Implemented; reduced completeness contract below |
| Fresh V8/Istanbul/LCOV collectors | Fresh Coverlet and Microsoft coverage collection | Phase 2 |
| Every executable statement-start line | C# sequence points with callable ownership | Phase 2; current denominator is supplied points |
| Changed-file and PR selection | Git merge-base and authenticated `gh` discovery | Phase 3 |
| Project policy and boolean overrides | Versioned config and explicit CLI precedence | Phase 3 |
| Regression baselines and move matching | Qualified C# identities and token fingerprints | Phase 4 |
| Text and JSON reports | Same output roles | Implemented, initial report shape only |
| XML, HTML, SARIF, filters/sorting/top | C# reports and stable diagnostic IDs | Phase 5 |
| Public Rust API and streaming rendering | `dotnet_crap` library and thin CLI | Phase 5 |
| Versioned JSON Schemas and compatibility policy | Coverage/config/baseline/report schemas | Phase 6 |
| Native archives and checksums | Linux/macOS x64 + ARM64, Windows x64 | Release workflow included; release-time validation pending |
| npm distribution | .NET global/local tool NuGet package | Launcher and local install smoke included |
| Setup GitHub Action | Pinned native release download and checksum | Included; published-release adoption pending |
| Installers, Homebrew, Linux packages, provenance | Equivalent native distribution | Phase 6 |
| Audits, mutation, coverage, proofs, benchmarks | Rust and real .NET validation | Basic CI/E2E included; advanced checks Phase 6 |
| Independent project adoption | Public C# projects with measured evidence | Phase 7 |

## Initial release contract

The scanner scores implemented methods, constructors, local functions, explicit
accessors, expression-bodied properties, lambdas, and anonymous delegates.
Declaration-only methods and auto-accessors are omitted. Callables are separate
records; nested decisions are not charged to the enclosing callable. Locations
include a one-based line, UTF-8 byte column, and end line.

Complexity begins at 1 and adds one for `if`, loops, `catch`, ternaries,
non-default switch labels and non-discard switch-expression arms, `when` guards,
`&&`, `||`, `??`, and `??=`. Comments and strings do not contribute.
This is the tool's convention, not a claim of Roslyn or IDE metric equivalence.
Operators, destructors, expression-bodied indexers, top-level statements,
advanced patterns, and conditional-compilation symbols need broader support.

Native coverage is validated for version, unknown/duplicate keys, canonical
paths/line numbers, unsigned hit counts, source hashes, and selected-source line
bounds. Every supplied point within a callable's span contributes equally;
positive hits count as covered. Omitted points do not enter the denominator.
`--require-coverage` requires at least one supplied point per callable, not
complete statement evidence. Nested callable line spans are excluded from the
parent; same-line nesting is ambiguous. Producers must include zero-hit points.
Hashes detect stale bytes, not forged reports. The live fixture uses explicit
instrumentation; it is not Coverlet integration.

Score, missing-required-coverage, and empty-selection gate failures exit 1 and
still emit reports. Invalid source/input/I/O exits 2. Missing coverage is 0% for
scoring. Empty scans pass unless `--fail-on-empty` is set. Reports are staged
before replacement and reject source extensions, symlinks and selected-input
aliases, including generated/unselected source hardlinks.

## Phase 2: real coverage collection and complete evidence

1. Add a Rust `coverage` command that snapshots selected source bytes **before**
   executing a trusted test command, then verifies them after completion.
2. Collect Coverlet with an explicit Cobertura format. Add OpenCover and Microsoft
   `dotnet-coverage` import after that. Pin collector versions in fixtures; do not
   depend on unspecified reporter defaults.
3. Normalize report paths, SourceLink/deterministic-build paths and Windows drive
   roots without guessing ambiguous matches. Handle partial classes and multiple
   test assemblies conservatively. Never claim fresh evidence by hashing an old
   XML report after tests have run.
4. Define statement/sequence-point ownership, missing evidence, overlapping
   uncovered counters, and same-line nested callable handling. Upgrade
   `--require-coverage` only with a documented compatibility change.

Acceptance: real xUnit/NUnit/MSTest fixtures run in CI with pinned dependencies;
partial/full/no coverage, test failure, stale source, missing files, overlapping
assemblies, and malformed XML have end-to-end assertions. A failing collector
publishes no new coverage. SDK 8/10 and VSTest/Microsoft Testing Platform behavior
are documented from actual runs.

## Phase 3: C# completeness and selection policy

1. Add operators, conversions, destructors, indexers, top-level code, advanced
   switch/pattern forms, async/iterators, records and primary constructors.
2. Decide preprocessor policy explicitly: source-as-written versus configured
   compilation symbols. Test inactive and invalid branches; never silently claim
   compilation-aware results. Add generated-file markers and deterministic ignores.
3. Add versioned `dotnet-crap.json`, strict fields, include/exclude globs,
   threshold/completeness policy and explicit CLI-over-config precedence.
4. Add committed Git diff/PR modes matching the reference: merge-base selection,
   whole changed files, verified HEAD/worktree bytes, renames/deletions, shallow
   history diagnostics and no automatic checkout/fetch.

Acceptance: hand-calculated modern C# syntax corpus; local Git fixtures for
renames, dirty files and merge bases; process-boundary stub only for `gh`.
Ignore/config behavior is deterministic from different working directories.

## Phase 4: regression baselines

Add versioned baseline creation/comparison with qualified scope/overload identities,
comment/format-insensitive token fingerprints, explicit edited-move mappings and
conservative ambiguity handling. Completeness failures cannot be suppressed.

Acceptance: new/worse violations fail; unchanged/improved/removed ones pass;
renames, overloads, anonymous functions, moved files and threshold mismatches have
fixtures. Never silently rewrite a committed baseline.

## Phase 5: reports and reusable API

Split scanning/rendering into a reusable Rust library. Add XML, standalone HTML
with search and failing-only filters, then SARIF 2.1.0 with stable `DNCRAP001`
(score) and `DNCRAP002` (completeness) IDs, encoded relative locations and stable
fingerprints. Add deterministic text sort/top/filter flags without changing gates.

Acceptance: format/schema/escaping tests, Unicode and terminal controls, output
failures and failed-gate reports. Each format reports empty selections clearly.
API compatibility checks and examples run in CI.

## Phase 6: hardening, distribution and performance

Publish schemas and a compatibility/migration contract. Harden alias defenses
against ignored/unselected source hardlinks, improve diagnostics, and replace
recursive traversal with iterative bounded-resource processing. Add Rust coverage,
mutation tests, dependency auditing, workflow linting, benchmark budgets and
bounded verification where it adds evidence.

Release gates: test native and installed .NET tool on all five supported RIDs;
verify checksums and provenance; pin matching Rust/NuGet versions; add protected
publishing and trusted publisher identity where supported. Verify NuGet package-ID
ownership before first publication. Linux GNU builds initially require Ubuntu
24.04-compatible glibc; musl, Windows ARM64 and Android are not release targets.
Add installers, Homebrew and Linux packages after the core assets are stable.
The setup Action must be exercised against a real immutable release; add
optional gate/SARIF integration after the report contract is stable.

## Phase 7: adoption and parity sign-off

Measure three to five independent C# repositories at pinned commits. Record SDK,
collector and tool versions, commands, coverage mapping limitations, scores,
runtime, memory and integration friction. Include a multi-project solution,
a library, an ASP.NET application and a modern language-feature project.

Parity is complete only when each applicable matrix row has automated evidence
or an explicit documented exclusion. The initial PR deliberately ends at the
small runnable foundation; these milestones should be separate reviewable PRs.
