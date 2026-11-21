# 0.2.2 release notes

Patch release on top of
[v0.2.1](https://github.com/RuiCostafrg/dotnet-crap/releases/tag/v0.2.1). No CLI
or Rust API changes, and no change to how a callable is scored.

## Parsing

- **C# grammar 0.23.5 → 0.24**
  ([#69](https://github.com/RuiCostafrg/dotnet-crap/pull/69)). C# 14
  `extension(...)` blocks now parse, so their members are scored instead of the
  file being skipped. On the [evaluation corpus](EVALUATION.md), CsvHelper scores
  1,222 callables instead of 1,204 and has 10 unparsed files instead of 11; the
  other three projects are unchanged, and no existing complexity value changed.
- **Conditional compilation keeps its contract.** Grammar 0.24 accepts
  `#if` directives in positions 0.23.5 rejected, such as one that splits a
  method, and would score such files without an error. A placement check now
  sends these files to the [preprocessor fallback](PREPROCESSOR.md), as before,
  so `--define` still decides which branch is scored.

## Distribution

- **The NuGet package declares its license**
  ([#70](https://github.com/RuiCostafrg/dotnet-crap/pull/70)). The package now
  carries `MIT` as its license expression plus project and repository links;
  nuget.org reported "License missing" for 0.2.1.
- **Supported runtimes are .NET 10 and .NET 11** (unchanged in practice: CI
  already tested only these two).
- Built with Rust 1.100.0, cargo-dist 0.34.0 and the .NET 10.0.404 SDK
  ([#68](https://github.com/RuiCostafrg/dotnet-crap/pull/68)).
- npm is retired and its packages were unpublished on 2026-11-21; 0.2.2 is not
  published there.
