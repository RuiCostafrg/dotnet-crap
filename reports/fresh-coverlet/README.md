# Fresh Coverlet collection evidence (expanded validation in progress)

Last archived focused validation: `151356d` (strict completion-observation deadline),
recorded in `focused-151356d.json` / `focused-151356d.log`. A subsequent literal
source-prefix fix requires renewed final validation; current progress is not a
final-tree pass. Final source/test/Cargo hashes will be captured after those gates.

Last completed non-mutation validation revision: `dedf03e6d2aa30161475108fe036f3b3a270a916`.
The implementation starts at `7aaae575`; `dedf03e` fixes review findings and the
live expected-score arithmetic. `tested-files.json` records exact source/test and
Cargo input hashes, so later documentation commits can be checked for equality.

Formatting, warnings-denied clippy, locked debug/release tests, Python contracts,
actionlint 1.7.12 and the full explicitly enabled live SDK/collector suite passed.
Further PR review fixes and MSTest/NUnit fixtures are committed in `18fc47a`.
Fresh validation is running; the older logs do not certify those later changes. Supported-host packaging smoke
passed in Ubuntu CI at `027815c`; the local Android/bionic installed launcher
crashed with SIGABRT in the Mono runtime. That local smoke is not a pass.
Launcher contracts and archive/repack checks passed locally before that failure.

The independent Sol review found permissive results-directory permissions and an
inconsistent UNC policy. The fix creates Unix directories atomically with 0700
permissions, tested in a subprocess with umask 000. UNC is explicitly rejected on
all hosts, while native Windows drive paths remain supported by the parser.
CLI `collect` is reserved; `./collect` selects a directory with that name. The
review also identified stale documentation, now corrected. Parent integration
review corrected the hand calculation for partial coverage: CC 2 and 4/5 lines
covered means `2 + 4 * (1/5)^3 = 254/125 = 2.032`, not 2.016.

## Mutation provenance

`initial/` preserves the complete 282-entry inventory, outcome lists and gate log
for `7aaae575`: 250 caught, 23 missed, 0 timeouts, 9 unviable; 91.58%. That result
predates the review fixes and is not final-tree validation. No filters or blanket
exclusions were added. The subsequent local run at
`target/mutation-runs/run-2ntvch4c/mutants.out` was stopped after additional PR
review fixes made its source stale. Its partial outcomes are not a full score.
`inventory-audit.json` now refers to the new current-source invocation and
confirms all five production modules are included; final outcomes are pending.

## Benchmarks

`benchmark.json` and `benchmark-coverage.json` are fresh release measurements for
`dedf03e`, with seven samples, 500 source files and 1,000 callables in each mode.
`benchmark-provenance.json` records the source tree, compiler, commands and binary
SHA-256. Other local builds could contend for resources. These observations make
no speedup, regression, parser-throughput or portable latency claim. They do not
reuse the historical API branch measurements.

## Supported scope

The pinned fixture is .NET SDK 10.0.109/runtime 10.0.9, VSTest, Coverlet collector
6.0.4 and xUnit 2.9.2 (Test SDK 17.12.0, adapter 2.8.2), on this Linux-bionic ARM64
host. Native v1 supplied-line completeness is unchanged. Same-line nesting,
unmapped SourceLink/deterministic paths and UNC paths have explicit limits.
The live harness tests real failed tests and an unsupported MTP-mode invocation;
that rejection does not certify an MTP collector. Separate worker live runs
verified MSTest 4.5.2 and NUnit 4.3.2 / adapter 5.0.0 (the standalone NUnit
worker run was not archived; its README is a narrative observation). Combined current-source
validation is in progress. .NET 8 and actual MTP collection remain unverified; CI is configured to exercise .NET 10 on its
Linux/macOS/Windows matrix but results must be inspected independently.

Release publication remains disabled. The user subsequently authorized merging
PR #15 only after current-head CI is green and review findings are handled.
