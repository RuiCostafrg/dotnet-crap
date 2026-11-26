# Share one native scoring engine

The Rust library owns source selection, scoring, evidence validation, and safe report writing so library users and the CLI get the same behavior. Distribute the native executable directly and use a thin .NET launcher to select it by platform, so NuGet users can install it with familiar tooling without introducing a second implementation or requiring the .NET runtime for native installs. The .NET tool is the primary package channel and replaces the original npm distribution.

See [Rust API](../RUST_API.md) and [distribution](../DISTRIBUTION.md).
