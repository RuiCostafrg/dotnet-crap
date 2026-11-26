# Contributing

The analyzer is Rust; the .NET launcher only selects and starts the packaged
native executable. Keep scoring, file selection, and coverage validation in Rust.
No Node.js or npm is required.

See [architecture decisions](docs/adr/) for the design rationale and
[local CI](docs/local-ci.md) for the Linux check runner. Generate a reproducible
whole-process performance sample after a release build.

```sh
python3 scripts/benchmark.py --output target/benchmark-scan.json
python3 scripts/benchmark.py --coverage --output target/benchmark-coverage.json
```

The harness requires Python 3, discards one warmup, checks the full scan output,
and accepts an optional `--max-median-ms` budget for a controlled runner.

Use a stable Rust toolchain and a C compiler for Tree-sitter. The live C# tests
and launcher require the .NET 10 SDK.

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
DOTNET_CRAP_E2E=1 DOTNET_CRAP_COVERLET_E2E=1 cargo test --locked
cargo test --locked --release
```

The two environment variables enable the live C# and coverage collector
fixtures. Ordinary Rust tests also execute the compiled CLI against C# source
files. See [mutation testing](docs/MUTATION_TESTING.md) and the
[benchmark guide](docs/BENCHMARKS.md) for the other CI checks.

Keep fixtures small with hand-calculated complexity and CRAP scores. Tests should
exercise the public command and malformed input as well as passing scans.
Document supported C# constructs and coverage approximations when changing them.
Maintainers can find release preflight, packaging, and registry procedures in
[distribution](docs/DISTRIBUTION.md) and [NuGet publishing](docs/NUGET_PUBLISHING.md).
