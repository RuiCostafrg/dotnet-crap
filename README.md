<img src="assets/icon.svg" width="64" height="64" alt="">

# dotnet-crap

A Rust command-line quality gate for **C# CRAP scores**, inspired by
[typescript-crap](https://github.com/RuiCostafrg/typescript-crap).

This first version parses real `.cs` files with Tree-sitter and reports callable
complexity, coverage, and CRAP scores. It is distributed as a native executable
and a .NET tool NuGet package. No npm package or Node.js runtime is involved.

```text
CRAP = complexity² × (1 − coverage)³ + complexity
```

Coverage is a fraction from 0 to 1. Missing coverage scores as zero coverage.
Complexity 4 therefore scores 20 uncovered, 6 half covered, and 4 fully covered.
The default threshold is 10; only scores strictly above it fail.

## Build and run

Building requires stable Rust and a C compiler for the C# parser. Scanning with
a prebuilt native executable requires neither Rust nor the .NET SDK.

```sh
cargo build --locked --release
./target/release/dotnet-crap --root examples/Fixture --format json
./target/release/dotnet-crap --root . src --threshold 30
./target/release/dotnet-crap --root . --coverage coverage.json --require-coverage
./target/release/dotnet-crap --root . --format json --output report.json
```

Source paths are relative to `--root`; coverage and report paths are relative to
the calling directory. Reports are produced for passing and failing score gates.
Exit codes are **0** for pass, **1** for a score gate failure, and **2** for an
argument, source, coverage, or I/O error. `--fail-on-empty` rejects an empty scan.
Missing required coverage and empty-selection gate failures also exit 1.

## Coverage and scope

The initial [native coverage format](examples/COVERAGE.md) uses root-relative
source paths and SHA-256 hashes of exact source bytes. It rejects stale coverage.
It is an input contract, not a built-in Coverlet collector; use reports produced
by a trusted test pipeline.
The initial denominator is the supplied coverage points inside each callable;
producers must include zero-hit points. `--require-coverage` requires some
evidence per callable, rather than proving complete statement coverage.

The first implementation targets C# callables. F#, Visual Basic, semantic
compilation, conditional-compilation symbol resolution, Git/PR selection,
baselines, and additional report formats are future work. See the
[full feature-parity plan](docs/FEATURE_PARITY.md) for the TypeScript feature
mapping, .NET-specific requirements, milestones, and acceptance criteria.

## Installation and releases

The release workflow builds platform binaries and a .NET 10 tool package. Once a
maintainer tags and publishes a release, download an archive and its checksum
from [GitHub Releases](https://github.com/RuiCostafrg/dotnet-crap/releases), or
install the published NuGet package:

```sh
dotnet tool install --global dotnet-crap --version 0.1.0
dotnet-crap --help
```

These commands describe the release being prepared; this initial PR does not
publish a release. NuGet publication requires the repository's configured
`NUGET_API_KEY`. See [packaging](packaging/README.md) for local package creation
and verification. The launcher needs .NET 10 and starts the bundled native
executable, forwarding its arguments and exit code.

The repository also supplies a setup Action for released native binaries:

```yaml
- uses: RuiCostafrg/dotnet-crap@<full commit SHA>
  with:
    version: '0.1.0'
- run: dotnet-crap --root . --threshold 10
```

This action verifies SHA-256 before adding the pinned release to `PATH`. GNU
Linux binaries target Ubuntu 24.04-compatible glibc; Alpine/musl and Android are
not supported release targets.

## Development

See [CONTRIBUTING.md](CONTRIBUTING.md) for checks, including the live .NET fixture
suite, and [SECURITY.md](SECURITY.md) for coverage trust and release verification.
Licensed under [MIT](LICENSE).
