# Changelog

## [1.0.0](https://github.com/RuiCostafrg/dotnet-crap/releases/tag/v1.0.0)

- Declares the [1.x compatibility policy](docs/COMPATIBILITY.md) for the CLI,
  exit codes, scoring, SARIF identities, and versioned inputs and reports.
  Upgrading from 0.4.0 requires no changes.

## [0.4.0](https://github.com/RuiCostafrg/dotnet-crap/releases/tag/v0.4.0)

- Derived executable lines missing from a coverage report count as uncovered
  and appear as `unevidenced_lines`. Producers should include zero-hit points.
  See the [native coverage contract](examples/COVERAGE.md) for exceptions.
- Partial scans resolve unmatched baseline entries only for files that were
  scanned or no longer exist. [Baseline matching](docs/BASELINES.md).
- Added `--changed-since`, `--partial-unparsed`, and text report views
  `--sort`, `--top`, and `--failing-only`. Partially scored files are
  reported in text, JSON, and SARIF.

## [0.3.0](https://github.com/RuiCostafrg/dotnet-crap/releases/tag/v0.3.0)

- Scans and collection honor `.gitignore` and `.ignore`. Use `--no-ignore`
  for the old selection behavior. Added [configuration and globs](docs/CONFIG.md).
- Added [regression baselines](docs/BASELINES.md) with conservative move matching
  and a way to declare moves explicitly.
- Added live Cobertura collection for [Microsoft Testing Platform and Microsoft
  coverage tools](docs/COVERLET.md#microsoft-testing-platform).
- Published [JSON Schemas](schemas/) for reports and inputs. JSON gained
  `version`, `scope`, and `signature`. Baseline runs include state information
  in JSON and SARIF.

## [0.2.2](https://github.com/RuiCostafrg/dotnet-crap/releases/tag/v0.2.2)

- Updated the Tree-sitter C# grammar to 0.24 so C# 14 extension blocks parse.
  The placement check preserves the [conditional compilation fallback](docs/PREPROCESSOR.md)
  even when the new grammar accepts split declarations.
- Declared the MIT license in the NuGet package. Supported runtimes are
  .NET 10 and .NET 11. npm distribution was retired.

## [0.2.1](https://github.com/RuiCostafrg/dotnet-crap/releases/tag/v0.2.1)

- Cobertura import ignores existing files outside the scan root. Missing files
  and ambiguous in-root mappings still fail.
- Decode UTF-8 source in place to avoid an extra copy.
- Stage NuGet releases for owner promotion and verify the install from
  NuGet.org against the authenticated release package.

## [0.2.0](https://github.com/RuiCostafrg/dotnet-crap/releases/tag/v0.2.0)

- Added fresh Cobertura collection and expanded C# callable and logical-pattern
  scoring. New pattern decisions can change scores from 0.1.0.
- The Rust option structs gained fields for parallelism, symbols, and skipping
  unparseable files. See the [Rust API guide](docs/RUST_API.md) for current usage.
- Added conditional compilation fallback, opt-in skipping of unparseable files,
  and parallel file scanning. Improved traversal and Cobertura import.
- First release on NuGet.org.

## [0.1.0](https://github.com/RuiCostafrg/dotnet-crap/releases/tag/v0.1.0)

- First release of the C# scanner.
