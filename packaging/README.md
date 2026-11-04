# Distribution packaging

`dotnet-crap` is a .NET 10 global tool NuGet package. `scripts/pack-dotnet-tool.sh <native-stage> [version]` expects native executables at `<native-stage>/<rid>/dotnet-crap[.exe]`, then packs them into the tool package. The launcher selects the matching bundled executable and forwards arguments and exit status.

Release builds require all five RIDs: `linux-x64`, `linux-arm64`, `osx-x64`,
`osx-arm64`, and `win-x64`. The feed is created under `<native-stage>/feed`.
For a local development package containing only your host binary:

```sh
cargo build --locked --release
mkdir -p target/stage/linux-x64
cp target/release/dotnet-crap target/stage/linux-x64/
DOTNET_CRAP_LOCAL_RID=linux-x64 bash scripts/pack-dotnet-tool.sh target/stage 0.1.0
dotnet tool install --tool-path target/tool --add-source target/stage/feed --version 0.1.0 dotnet-crap
target/tool/dotnet-crap --root examples/Fixture --threshold 100 --fail-on-empty
```

Use the appropriate host RID. A local subset package is only for development;
the release workflow does not enable the subset override. Tags must match the
Cargo version; that version is passed to `dotnet pack` to keep the NuGet package
aligned. Publication is triggered by `v*` tags after native build/smoke jobs and
an installed-tool smoke. GitHub assets are published automatically; NuGet push
runs when `NUGET_API_KEY` is configured, otherwise emits an explicit notice.
No release is created by a pull request.
