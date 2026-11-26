# 1.0.0 release notes

First stable release, on top of
[v0.4.0](https://github.com/RuiCostafrg/dotnet-crap/releases/tag/v0.4.0).
Behavior is the same as 0.4.0. What changes is the promise: from this release
`dotnet-crap` follows Semantic Versioning, as the
[compatibility policy](COMPATIBILITY.md) defines.

## Stable in 1.x

Breaking any of these needs a 2.0.0:

- **Command line:** every flag, its meaning and its default, for scans and for
  `collect`.
- **Exit codes:** `0` pass, `1` policy failure, `2` input or runtime error.
- **JSON report v1:** fields may be added in minor releases. None is removed,
  renamed or retyped, and `version` stays `1`.
- **Input formats:** native coverage, `dotnet-crap.json` and baseline files,
  all v1. Every 1.x release reads every v1 file an earlier 1.x release
  accepted.
- **SARIF:** rule IDs and `partialFingerprints`.
- **Scoring:** the CRAP formula and the documented complexity, ownership and
  coverage conventions.

The JSON report and the three input formats have JSON Schemas in
[`schemas/`](../schemas); SARIF output follows the standard SARIF 2.1.0
schema. Report consumers must ignore fields they do not know.

The text report's layout, error wording, run time and the Rust library API
are not covered. A flag or config key that is to be removed keeps working,
with a warning, for at least one minor release first.

## Upgrading

- **From 0.4.0:** nothing to change.
- **From 0.3.x or earlier:** read the [0.4.0 notes](RELEASE_0_4_0.md) and the
  [0.3.0 notes](RELEASE_0_3_0.md). In particular, since 0.4.0 executable lines
  missing from a coverage file count as uncovered. Since 0.3.0, `.gitignore`
  and `.ignore` files are honored.
- **From 0.2.1 or earlier:** also read the [0.2.2 notes](RELEASE_0_2_2.md),
  which made C# 14 extension blocks parse, so their callables are now scored.

## Since 0.4.0

- walkdir 2.5.0 → 2.5.1 (patch update).
- The release build's pinned `dtolnay/rust-toolchain` action moved forward
  one commit.
- The GitHub Action's `exit-code` output and failure annotation now describe
  `2` as an input or runtime error, as the CLI documents. The code is
  unchanged.

## Distribution

Unchanged: NuGet.org, Homebrew, the shell installer, the GitHub Action and
GitHub release assets, for .NET 10 and .NET 11.
