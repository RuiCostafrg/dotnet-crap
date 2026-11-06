![dotnet-crap banner](assets/readme-banner.svg)

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
./target/release/dotnet-crap --root . --format sarif --output report.sarif
```

Source paths are relative to `--root`; coverage and report paths are relative to
the calling directory. Reports are produced for passing and failing score gates.
Exit codes are **0** for pass, **1** for a score gate failure, and **2** for an
argument, source, coverage, or I/O error. `--fail-on-empty` rejects an empty scan.
Missing required coverage and empty-selection gate failures also exit 1.
[SARIF output](docs/SARIF.md) includes score, missing-required-coverage, and
empty-selection diagnostics for code-scanning integrations.

## Rust library

The package also exposes `dotnet_crap::{ScanOptions, scan, ReportFormat}` so Rust
code can scan and render reports without starting the CLI. Gate failures are
reported by `ScanResult::report().failed`; input errors are returned as `Err`.
See the [Rust API guide](docs/RUST_API.md) for consumption, verification, and
limits. Run the bundled example with `cargo run --example scan -- <root>`.

## Coverage and scope

The initial [native coverage format](examples/COVERAGE.md) uses root-relative
source paths and SHA-256 hashes of exact source bytes. It rejects stale coverage.
It is an input contract, not a built-in Coverlet collector; use reports produced
by a trusted test pipeline.
The initial denominator is the supplied coverage points inside each callable;
producers must include zero-hit points. `--require-coverage` requires some
evidence per callable, rather than proving complete statement coverage.

The scanner supports C# methods, constructors, operators, conversions, destructors,
explicit accessors, expression-bodied properties/indexers, and nested callables.
See [callable support and limits](docs/CSHARP_CALLABLES.md). F#, Visual Basic, semantic
compilation, conditional-compilation symbol resolution, Git/PR selection,
baselines, and additional report formats are future work. See the
[full feature-parity plan](docs/FEATURE_PARITY.md) for the TypeScript feature
mapping, .NET-specific requirements, milestones, and acceptance criteria.

## Installation and releases

The release workflow builds platform binaries and a .NET 10 tool package as
workflow artifacts; it does not publish a release. Automatic publication is
unavailable until a future publication workflow is reviewed and host protections
can be enforced. If a release is published in the future, download an archive and its checksum
from [GitHub Releases](https://github.com/RuiCostafrg/dotnet-crap/releases), or
install the published NuGet package:

```sh
dotnet tool install --global dotnet-crap --version 0.1.0
dotnet-crap --help
```

These commands describe a future release; no release is created by this workflow.
See [packaging](packaging/README.md) for local package creation
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
