# 0.3.0 release notes

Minor release on top of
[v0.2.2](https://github.com/RuiCostafrg/dotnet-crap/releases/tag/v0.2.2). It adds
the features planned for 1.0: a config file, ignore rules, baselines, Microsoft
Testing Platform coverage, and versioned schemas for every file format. The
CRAP formula and complexity rules are unchanged.

## Behavior change: ignore files are honored

Scans and `collect` now skip files excluded by `.gitignore` and `.ignore`
files, including nested ones, even outside a Git repository
([#74](https://github.com/RuiCostafrg/dotnet-crap/pull/74)). A file named
explicitly on the command line is always scanned. Pass `--no-ignore`, or set
`"respect_ignore_files": false` in the config, to scan as 0.2.x did.

## New

- **`dotnet-crap.json` config file**
  ([#74](https://github.com/RuiCostafrg/dotnet-crap/pull/74),
  [guide](CONFIG.md)). Covers threshold, include/exclude globs, ignore files,
  coverage and empty-scan policy, defines and baseline. It is read from
  `<root>/dotnet-crap.json`, or from `--config PATH`; `--no-config` skips it.
  Unknown keys are rejected. Command-line values always win, and each boolean
  setting gets a `--no-…` flag to switch it off from the command line.
- **`--include` / `--exclude` globs** in gitignore syntax, on the command line
  or in the config.
- **Baselines** ([#76](https://github.com/RuiCostafrg/dotnet-crap/pull/76),
  [guide](BASELINES.md)). `--write-baseline PATH` records today's violations;
  `--baseline PATH` then fails only on violations that are `new` or `worse`.
  A callable keeps its entry when it is reformatted, commented, or moved to a
  renamed file. When the match is ambiguous it is treated as `new`;
  `--baseline-move OLD=NEW` states a move explicitly. A baseline is replaced
  only with `--overwrite-baseline`.
- **Microsoft Testing Platform and Microsoft coverage**
  ([#75](https://github.com/RuiCostafrg/dotnet-crap/pull/75),
  [guide](COVERLET.md#microsoft-testing-platform)). `collect` now finds the
  Cobertura reports from `Microsoft.Testing.Extensions.CodeCoverage`,
  `coverlet.MTP` and the `dotnet-coverage` tool. `{results}` may begin a path
  (`{results}/coverage.cobertura.xml`). Each tool is tested live in CI.
- **Versioned formats and a compatibility policy**
  ([#78](https://github.com/RuiCostafrg/dotnet-crap/pull/78)). JSON Schemas
  for the JSON report, native coverage, config and baseline files are in
  [`schemas/`](../schemas). [COMPATIBILITY.md](COMPATIBILITY.md) defines what
  stays stable from 1.0.0.

## Output changes (additive)

- The JSON report starts with `"version": 1`. Each callable gains `scope` and
  `signature`. When a baseline is used, each failing callable gets a `baseline`
  state and the report gets a `baseline` summary of counts.
- SARIF results carry `baselineState` when a baseline is used. Rule IDs and
  `partialFingerprints` are unchanged.

## Distribution

- Supported runtimes remain .NET 10 and .NET 11. Channels are unchanged:
  NuGet.org, Homebrew, the shell installer, the GitHub Action and GitHub
  release assets. npm is retired.
- New dependency: `ignore` 0.4. The dev-only `jsonschema` checks the schemas in
  tests.
