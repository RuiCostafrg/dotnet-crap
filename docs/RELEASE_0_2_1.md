# 0.2.1 release notes

Patch release on top of
[v0.2.0](https://github.com/RuiCostafrg/dotnet-crap/releases/tag/v0.2.0). No
scoring, CLI or Rust API changes: every score from 0.2.0 is unchanged.

## Fixes

- **Coverage outside the scan root no longer fails the import**
  ([#65](https://github.com/RuiCostafrg/dotnet-crap/pull/65),
  [#66](https://github.com/RuiCostafrg/dotnet-crap/pull/66)). Coverlet can
  instrument files outside the repository, such as the program file
  Microsoft.NET.Test.Sdk compiles from the NuGet package cache when one test
  project references another. `collect` and `--coverage` used to reject the whole
  report with "Cobertura path is outside the coverage root". An existing file
  outside the root, named by an absolute path or through a `<source>` prefix, is
  now ignored, because it can never be a scanned source. Directories, missing
  files and ambiguous in-root matches still fail, and an outside file never
  supplies evidence for a missing selected source. See [COVERLET.md](COVERLET.md).

## Performance

- UTF-8 sources are decoded in place instead of being copied, so each file is
  held in memory once ([#56](https://github.com/RuiCostafrg/dotnet-crap/pull/56)).

## Distribution

- **NuGet.org releases are staged**
  ([#62](https://github.com/RuiCostafrg/dotnet-crap/pull/62)). `publish-nuget.yml`
  uploads with `dotnet package stage push`, and the owner promotes the package on
  NuGet.org with 2FA. The trusted publishing policy allows staging only. See
  [NUGET_PUBLISHING.md](NUGET_PUBLISHING.md).
- **Post-release verification covers NuGet.org**
  ([#60](https://github.com/RuiCostafrg/dotnet-crap/pull/60)). The package served
  by NuGet.org must contain exactly the release `.nupkg` entries, apart from the
  NuGet repository signature, and install and run from NuGet.org alone.
- npm remains retired; 0.2.1 is not published there.

## Also in this release

A reproducible evaluation on four independent C# projects
([#64](https://github.com/RuiCostafrg/dotnet-crap/pull/64),
[EVALUATION.md](EVALUATION.md)).
