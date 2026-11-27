![dotnet-crap banner](assets/readme-banner.svg)

# dotnet-crap

**Find complex C# code that tests barely cover.** `dotnet-crap` scores each
method, accessor, or other callable using its cyclomatic complexity and line
coverage. Run it locally to see where tests would help, or use it in CI to stop
new violations.

[Install](#install) · [Quick start](#quick-start) · [GitHub Actions](#github-actions) · [Performance](#performance) · [Documentation](#documentation)

## Install

The [NuGet tool](https://www.nuget.org/packages/dotnet-crap) runs on .NET 10
and .NET 11. It supports macOS and Linux on x64 and ARM64, plus Windows x64.

```sh
dotnet tool install --global dotnet-crap --version 1.0.0
dotnet-crap --version
```

On macOS or Linux, install the native executable with Homebrew:

```sh
brew install RuiCostafrg/tap/dotnet-crap
```

[GitHub Releases](https://github.com/RuiCostafrg/dotnet-crap/releases) provides
a shell installer and native archives, including static musl builds for Linux.
Native scans require neither Rust nor .NET. See [asset verification and an authenticated NuGet install](docs/PUBLISHED_VERIFICATION.md#install-the-authenticated-nuget-package-locally)
and [build prerequisites](CONTRIBUTING.md).

## Quick start

Run this from the root of your C# repository. Omit `src` to scan the whole root.

```sh
dotnet-crap --root . --top 10 src
```

A scan without coverage scores every callable as 0% covered. The CRAP formula is
`complexity² × (1 − coverage)³ + complexity`, with coverage between 0 and 1.
For example, a callable with complexity 4 scores 20 at 0% coverage, 6 at 50%,
and 4 at 100%. The default threshold is 10. Scores strictly above it fail.

### Scan with coverage

Use `collect` to turn fresh Cobertura reports into
[source-bound coverage JSON](examples/COVERAGE.md), then pass that file to
the scan. This example uses VSTest with `coverlet.collector` installed in the
test project. Replace the project path and set `--expected-reports` to the
number of reports your command produces. Choose a new file for the output.

```sh
dotnet-crap collect --root . --path src --output coverage.json --expected-reports 1 \
  -- dotnet test tests/Tests.csproj --collect 'XPlat Code Coverage' \
     --results-directory '{results}' -- \
     DataCollectionRunSettings.DataCollectors.DataCollector.Configuration.Format=cobertura

dotnet-crap --root . --coverage coverage.json --require-coverage src
```

Only run `collect` with test commands and code you trust. See
[coverage collection](docs/COVERLET.md) for Microsoft Testing Platform,
`dotnet-coverage`, report discovery, and path mapping.

### Adopt it on an existing codebase

Save today's violations in a [baseline](docs/BASELINES.md) and commit the file.
Later scans fail on new or worse violations. Required coverage still applies
independently of the baseline.

```sh
dotnet-crap --root . --coverage coverage.json --write-baseline crap-baseline.json src
dotnet-crap --root . --coverage coverage.json --baseline crap-baseline.json src
```

Use `dotnet-crap --help` for all flags and
[`dotnet-crap.json`](docs/CONFIG.md) for shared policy. Exit codes are **0** for
pass, **1** for a gate failure, and **2** for an input or runtime error. The
default report is text. Use `--format json` or `--format sarif` for automation.

## GitHub Actions

Add this step after checkout and coverage generation. It verifies and installs
the pinned release on Ubuntu or macOS, runs the scan, and uploads
[SARIF](docs/SARIF.md) to GitHub code scanning. Set `contents: read` and
`security-events: write` on the workflow job for this example.

```yaml
- uses: RuiCostafrg/dotnet-crap@6587201921b9c6c048abbd13940cace841ec68a3 # v1.0.0
  with:
    version: '1.0.0'
    scan: true
    sarif: true
    upload-sarif: true
    args: |
      --root
      .
      --coverage
      coverage.json
      --require-coverage
```

For a pull-request scan limited to changed files, pass
`--changed-since origin/main` as two argument lines and fetch the merge-base
history with `fetch-depth: 0` on checkout. See [Action inputs](action.yml).

## Performance

These wall times come from one Ubuntu 26.04 GitHub-hosted run on pinned C#
projects on November 19, 2026, before v1.0.0. The scan column measures scoring
with precollected coverage.
Collection includes building and running the selected tests. CsvHelper skipped
11 unparseable files in this run.

| Project | Files scored | Callables | Scan | Collect including tests |
| --- | ---: | ---: | ---: | ---: |
| GuardClauses | 15 | 121 | 0.03 s | 27.6 s |
| CsvHelper | 95 | 1,204 | 0.3 s | 213.7 s |
| eShopOnWeb | 205 | 556 | 0.1 s | 38.2 s |
| CleanArchitecture | 72 | 151 | 0.03 s | 16.0 s |

See the [evaluation](docs/EVALUATION.md) for source revisions, memory, and
reproduction instructions, and the [benchmark guide](docs/BENCHMARKS.md) for
comparing two builds on the same workloads.

## Documentation

- [Configuration and selection](docs/CONFIG.md) and [regression baselines](docs/BASELINES.md)
- [C# scoring and callable support](docs/CSHARP_CALLABLES.md) and [conditional compilation](docs/PREPROCESSOR.md)
- [Coverage collection](docs/COVERLET.md), [native coverage](examples/COVERAGE.md), and [SARIF](docs/SARIF.md)
- [Rust library](docs/RUST_API.md) and [1.x compatibility and schemas](docs/COMPATIBILITY.md)
- [Changelog](CHANGELOG.md) and [GitHub Releases](https://github.com/RuiCostafrg/dotnet-crap/releases)

See [Contributing](CONTRIBUTING.md) to build and test the project and
[Security](SECURITY.md) to report a vulnerability. Licensed under [MIT](LICENSE).
