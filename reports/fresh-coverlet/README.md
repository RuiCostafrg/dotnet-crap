# Fresh Coverlet collection evidence (validation in progress)

Production and test revision: `dedf03e6d2aa30161475108fe036f3b3a270a916`.
The implementation starts at `7aaae575`; `dedf03e` fixes review findings and the
live expected-score arithmetic. `tested-files.json` records exact source/test and
Cargo input hashes, so later documentation commits can be checked for equality.

Formatting, warnings-denied clippy, locked debug/release tests, Python contracts
and actionlint 1.7.12 passed. Explicit live SDK/collector tests, packaging smoke,
and the final full mutation rerun are in progress. This file must be finalized
with their actual results before declaring step 2 complete.

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
exclusions were added. The final run is
`target/mutation-runs/run-2ntvch4c/mutants.out`; its outcomes are pending.
`inventory-audit.json` confirms all five production modules are included.

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
that rejection does not certify an MTP collector. .NET 8, NUnit, MSTest and actual
MTP collection remain unverified; CI is configured to exercise .NET 10 on its
Linux/macOS/Windows matrix but results must be inspected independently.

No release publication or PR merge is authorized or performed by this feature.
