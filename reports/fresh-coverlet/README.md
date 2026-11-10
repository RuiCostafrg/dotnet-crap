# Fresh Coverlet collection evidence

Current production/test revision: `1cad955cbe5ec29590fb405174c495826de5f39f`.
The final evidence/plan follow-up changes no production source, executable tests,
fixture project/solution files or Cargo inputs. [`tested-files.json`](tested-files.json)
records the validated revision's exact SHA-256 hashes and separately identifies
a documentation-only correction to the MSTest fixture README.
All checks passed at that revision. Before merging an evidence-only follow-up,
verify its current-head checks and confirm executable input bytes remain unchanged and documentation amendments
match their separately recorded hashes.

## Validation

[Complete CI run](https://github.com/RuiCostafrg/dotnet-crap/actions/runs/42368570770)
passed Rust formatting, warnings-denied clippy, locked debug/release tests, Python
contracts, packaging smoke, and full explicitly enabled .NET tests on Linux x64,
macOS ARM64 and Windows x64. Both `DOTNET_CRAP_E2E=1` and
`DOTNET_CRAP_COVERLET_E2E=1` were set in the SDK jobs. The separate
[full production mutation run](https://github.com/RuiCostafrg/dotnet-crap/actions/runs/42368570749)
passed on the same head. Both runs checked out PR merge commit `76a45d2`
(`1cad955` merged into base `8620f71`). CI logs are retained in [`ci/`](ci/).
The documentation-only head `98ffe1b` reran every check successfully
([CI](https://github.com/RuiCostafrg/dotnet-crap/actions/runs/42371057237),
[mutation](https://github.com/RuiCostafrg/dotnet-crap/actions/runs/42371057282)).

Current-source local debug/release, Python and explicit collector runs also
passed; see [`validation.json`](validation.json), `debug.log`, `release.log`,
`python.log` and `live.log`. Actionlint 1.7.12 passed. The older complete local
SDK suite at `8a28326` is separately labeled in `full-live-8a28326.log`; it is not
substituted for the exact-head three-platform CI evidence.

Live logs intentionally contain a failed xUnit assertion and an MTP-mode SDK
rejection: these are passing negative tests verifying that no native evidence is
published. The Rust live test exits 0. The MTP diagnostic names the VSTest projects
under a correctly formed `--solution` invocation; it does not certify an MTP
collector. Local packaging smoke is **not** claimed as passing: the Android/bionic
Mono apphost crashed with SIGABRT. Launcher/offline/archive contracts passed
locally, and the supported Ubuntu CI packaging smoke passed.

## Actual collector and independent expectations

The four test projects use VSTest, .NET 10, Test SDK 17.12.0 and Coverlet collector
6.0.4. Framework pins are xUnit 2.9.2 / adapter 2.8.2, MSTest framework/adapter
4.5.2, and NUnit 4.3.2 / adapter 5.0.0. The local run used SDK 10.0.109/runtime
10.0.9; CI installs .NET 10.0.x on its recorded supported hosts.

Hand calculations, derived from the physical source and branch calls:

- `Choose`: one decision means CC 2; four of five supplied lines hit gives
  `2 + 4 * (1/5)^3 = 254/125 = 2.032`.
- Both `Full` methods: CC 2 and all five lines hit gives CRAP 2.
- `Uncovered`: CC 1, with its executable line supplied at zero hits, gives CRAP 2.

The harness requires one passed, non-skipped test per project and four fresh
reports. Each library has exactly one two-call xUnit signature and one distinct
three-call MSTest/NUnit signature. MSTest Full hits are 3/3/2/1/3; NUnit Full hits
are 3/3/1/2/3 because Beta tests `value == 0`. Both keep coverage 1 and CRAP 2.
It then exercises snapshot/verification/publication through the collector API,
checks the native scan scores, and runs actual failure cases. Partial classes,
multi-project union and zero-hit evidence are exercised directly.

The initial standalone NUnit worker run was not archived; its README is a
narrative observation. The committed current-source combined logs and CI runs,
not that observation, certify NUnit support. MSTest's retained standalone XML
records an earlier two-call fixture, explicitly distinguished in its README.
.NET 8 and actual MTP collection remain unverified. Native v1 supplied-line
completeness and same-line nesting limits are unchanged; unmapped deterministic,
SourceLink and UNC paths are rejected rather than guessed.

## Full production mutation gate

The complete downloaded CI artifact was independently checked using
`scripts/mutation-gate.py` `read_inventory` and `check_score`:

- 297 total: **262 caught, 24 missed, 0 timeouts, 11 unviable**.
- **91.61%** caught among caught + missed + timeouts, strictly above 90%.
- Inventory: `src/lib.rs` 161, `src/main.rs` 11, `src/cobertura.rs` 81,
  `src/collector.rs` 27, `src/sarif.rs` 17.

No file filters or blanket exclusions were added. [`inventory-audit.json`](inventory-audit.json)
compares independently enumerated production modules with the real inventory,
records its hash, and validates complete, disjoint outcome identities.
[`final/`](final/) retains `mutants.json`, `outcomes.json`, all outcome lists,
the gate log, and every unviable build diagnostic. The linked GitHub artifact
also contains every per-mutant test/build log and diff.

All 24 survivors and 11 unviable cases are reviewed in
[`survivor-review.md`](survivor-review.md). Survivors include non-equivalent test
gaps; they are not all declared equivalent or removed from the denominator.
The initial full run at `7aaae575` remains in `initial/`, qualified to that older
tree. Superseded partial local runs are not final score evidence. Once complete
current-head CI evidence was obtained, the redundant local run was stopped.
Historical measurements from `reports/reusable-api` were not reused.

## Benchmarks and review

`benchmark.json` and `benchmark-coverage.json` contain seven samples each over
500 files / 1,000 callables. [`benchmark-provenance.json`](benchmark-provenance.json)
records the compiler, exact source `40b5c45`, commands and measured binary digest.
Rust sources, Cargo inputs and the benchmark harness are byte-identical between
that revision and `1cad955`; only tests/fixtures and documentation changed.
Concurrent local builds could contend for resources. These are observations,
with no speedup, portable latency or collector-throughput claim. Dense nested
coverage profiling remains step 8 work.

Independent Sol review at `18fc47a` found no blocking production issue; its two
fixture/documentation suggestions and later valid PR findings were integrated.
The subsequent deadline boundary and literal-prefix fixes have explicit
regressions. [`review-response.md`](review-response.md) records decisions.
`focused-151356d.json` / `.log` are an explicitly historical parent check, not an
independent or final-tree test claim.

Release publication remains disabled. The user authorized merging PR #15 only
after current-head CI is green and review findings are handled.
