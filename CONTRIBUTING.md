# Contributing

The analyzer is Rust; the .NET launcher only selects and starts the packaged
native executable. Keep scoring, file selection, and coverage validation in Rust.
No Node.js or npm is required.

Use a stable Rust toolchain and a C compiler for Tree-sitter. The live C# tests
and launcher require the .NET 10 SDK.

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
DOTNET_CRAP_E2E=1 cargo test --locked
cargo test --locked --release
```

The environment variable enables execution of the real C# fixture; CI must set
it so a missing/broken .NET SDK cannot silently skip that contract. Ordinary Rust
tests still execute the compiled CLI against C# source files.

Keep fixtures small with hand-calculated complexity and CRAP scores. Tests should
exercise the public command and malformed input as well as passing scans.
Document supported C# constructs and coverage approximations when changing them.
The [parity roadmap](docs/FEATURE_PARITY.md) tracks future work; this first version
does not claim feature compatibility with the complete TypeScript tool.
