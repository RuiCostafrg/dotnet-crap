# .NET 10/11 CI compatibility

Scope: launcher runtime roll-forward, executable fixture targets and CI runners.
Rust scoring/output/collector production sources and Cargo dependencies are unchanged.

## Release and runner verification (2026-11-12)

- [Microsoft .NET 11 download](https://dotnet.microsoft.com/en-us/download/dotnet/11.0): GA SDK 11.0.100 / runtime 11.0.0, released November 10, 2026.
- [Microsoft .NET 10 download](https://dotnet.microsoft.com/en-us/download/dotnet/10.0): latest SDK 10.0.404 / runtime 10.0.15. CI selects current stable patches with `10.0.x` and `11.0.x`; local evidence below names its exact installed versions.
- [Official runner image table](https://github.com/actions/runner-images#available-images): Ubuntu 26.04 and macOS 26 are available. Ubuntu's `-latest` alias still identifies 24.04 in that table, so CI uses explicit `ubuntu-26.04` / `macos-26` labels. Windows is retained.
- The pinned actionlint 1.7.12 predates Ubuntu 26.04; `.github/actionlint.yaml` permits that verified label without suppressing other validation.

## Local validation

Launcher and fixture implementation revision: `f8967f9`. The remaining commits
only change workflows, lint configuration and documentation.

Host: Ubuntu 26.04 arm64 under PRoot/Termux. Rust/Clippy 1.96.0.
.NET 10 live suite: SDK 10.0.109 / runtime 10.0.9 (`linux-bionic-arm64`).
.NET 10 packaging smoke: isolated Linux SDK 10.0.404 / runtime 10.0.15.
Separate Linux arm64 .NET 11 SDK
11.0.100 / runtime 11.0.0. The .NET 11 installation contains no runtime 10.

The .NET 11 installation originally aborted for missing ICU. Installing host
`libicu78` resolved that dependency. Its compiler then hit a PRoot GC virtual
address reservation failure; successful local runs use
`DOTNET_GCHeapHardLimit=0x40000000`. CI does not inherit this workaround.
An initial Clippy dependency link crashed in Termux's linker; the retry with
`CARGO_BUILD_JOBS=2` passed. An initial packaging build failed with a missing
`zmij` rlib; the subsequent smoke completed successfully. Failed attempts are
not counted as passing evidence. The first .NET 10 smoke reached installed-tool
execution but the Termux apphost could not find its runtime (`DOTNET_ROOT` unset).
The isolated Linux .NET 10 smoke uses an explicit runtime root, as setup-dotnet
does in CI.

- Formatting and warnings-denied Clippy with `--locked --all-targets`: pass.
- Locked release tests and warnings-denied Rust documentation: pass.
- Python contracts: 49 tests, 2 optional real-binary benchmark tests skipped, no failures.
- All workflow files: actionlint pass.
- .NET 10: full `DOTNET_CRAP_E2E=1 DOTNET_CRAP_COVERLET_E2E=1 DOTNET_CRAP_TEST_TFM=net10.0 cargo test --locked` passed, including all three Coverlet frameworks, native coverage/accuracy fixtures and negative collection/MTP cases.
- .NET 10 Linux SDK 10.0.404 packaging smoke: pass, including 11 launcher contracts, concurrent packs, installed-tool gates and read-only execution as `nobody`.
- .NET 11: full `DOTNET_CRAP_E2E=1 DOTNET_CRAP_COVERLET_E2E=1 DOTNET_CRAP_TEST_TFM=net11.0 cargo test --locked` passed. This includes physical native-coverage/accuracy fixtures, xUnit/MSTest/NUnit Coverlet, independent line/score assertions, deliberate failing-test cleanup and the negative MTP probe. Each test framework asserts actual `Environment.Version.Major` against the requested target.
- Without the roll-forward setting in a copied runtimeconfig, the ICU-repaired runtime-11-only host rejects the net10 contracts with exit 150 (missing runtime 10). This isolates the launcher requirement from the earlier ICU failure.
- .NET 11 launcher process contracts: 11 passed. Root bypasses some permission enforcement, as printed by the runner.
- .NET 11 packaging smoke: pass, including two concurrent packs, retained feed bytes on rejected rebuild, installed-tool passing/failing gates, and execution as `nobody` from an unwritable install.

[Selected execution receipts](validation.txt) retain suite summaries, framework/runtime summaries and packaging outcomes.

## Remaining hosted evidence and protections

Ubuntu x64, macOS arm64 and Windows hosted matrix evidence belongs to this PR's
checks. Local arm64 evidence does not establish macOS/Windows results.
Existing required .NET 10 check identities are preserved despite newer runners.
The owner can separately require the new Linux/macOS .NET 11 checks, as documented
in `docs/SECURITY_PARITY.md`; no protections were modified here.
Release artifact runner pins/publication and PR #16 are outside this change.

## Architecture evidence audit

The prior architecture inventory contains all 12 production modules: 323 total,
279 caught, 22 missed, 0 timeouts and 22 unviable (92.69% viable caught), qualified
to PR #18 head `5745f83`. The later targeted recheck caught 7/7. See
[the source-qualified report and survivor review](../architecture/README.md).
The seven extracted modules have no survivors in the recorded full run. This is
historical evidence, not a new mutation measurement for these workflow changes.
The broader PLAN backlog and the reviewed survivors remain open work.
