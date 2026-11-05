# Distribution packaging

`dotnet-crap` is a .NET 10 global tool NuGet package. `scripts/pack-dotnet-tool.sh <native-stage> [version]` expects native executables at `<native-stage>/<rid>/dotnet-crap[.exe]`, then packs them into the tool package. The launcher selects the matching bundled executable and forwards arguments and exit status.

## Staging and concurrent packs

The pack script copies the launcher sources (not `bin/`, `obj/`, or `native/`) into a per-invocation work directory under `TMPDIR`. Native executables are staged there, and `dotnet pack` builds there too, so two packs from one checkout never share native, `bin`, or `obj` paths. The checkout's `packaging/dotnet-crap-launcher/native/` is no longer written. The finished `.nupkg` is published atomically to `<native-stage>/feed/`.

Each invocation requires a fresh output feed: `<native-stage>/feed` must not
already exist, even if it is empty or contains an identical-input build. This is
checked before building, and the directory is reserved exclusively at publication.
Use distinct stage directories for concurrent packs. To retry an installation or
publication, reuse the existing `.nupkg`; to rebuild, stage the native inputs in
a new directory. The script never removes or replaces an existing feed.
An interrupted pack may leave a reserved feed and likewise requires a new stage.

The current package pipeline does not promise byte-reproducible rebuilds;
archive timestamps and generated metadata can differ. Rebuilt archives are not
compared by normalized entries or silently substituted for retained packages.
The separate release-publication helper still requires exact byte identity on retries.

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
the release workflow does not enable the subset override.

## Launcher behavior

The launcher is `packaging/dotnet-crap-launcher/` with `NativeLauncher.cs` as the testable core. On Windows it starts `native/win-x64/dotnet-crap.exe` unchanged.

On Unix it never changes the installed file. If the file has an execute bit, the launcher tries to run it in place. NuGet extraction does not preserve Unix execute bits, and a shared install is often owned by another user, so when the kernel refuses the in-place launch with `EACCES` the launcher copies the native file into a fresh owner-only directory under `TMPDIR`, runs the copy, and removes the directory when the child exits. Arguments and the child's exit code pass through unchanged. Ctrl+C is left to the terminal's delivery to the child, so the private copy is still removed.

## Tests

- `dotnet run --project packaging/tests/DotnetCrapLauncher.Tests -c Release` starts the real launcher and a fake POSIX native executable. It checks argument and exit-code forwarding, direct execution, private-copy execution and cleanup, read-only installs, the `EACCES` fallback, and the in-process API. Run as a non-root user to enforce read-only permissions: root bypasses permission bits, and the runner prints a NOTE when it detects root.
- `bash scripts/test-install-action.sh` runs `scripts/install-action.sh` against a fake `curl`. It checks the connect and total timeouts on both requests, HTTPS-only transfer, checksum rejection, and exact-version validation.
- `bash scripts/test-release-publish.sh` runs the publish scripts with fake `gh` and `dotnet`, and checks the trusted-main ancestry check against a local git remote.
- `bash scripts/smoke-packaging.sh` runs all of the above, then builds the host native executable, packs two concurrent invocations, checks that the package has only the host native entry, installs it with `dotnet tool install`, and runs the installed launcher. As root on Linux it also runs the installed tool as `nobody` from an unwritable tree. It publishes, tags, and signs nothing.

Apply the local runtime settings your machine needs before running these. For example, when .NET uses an installation outside the default search path, set `DOTNET_ROOT`; on a host without ICU, set `DOTNET_SYSTEM_GLOBALIZATION_INVARIANT=1`. GitHub's `setup-dotnet` action configures the SDK in CI; hosted runners normally provide ICU without invariant mode.

## Release workflow

`.github/workflows/release.yml` runs on `v*` tags. Tags must match the Cargo version; that version is passed to `dotnet pack` to keep the NuGet package aligned. Build jobs use the pinned Rust 1.96.0 toolchain and the pinned .NET SDK 10.0.109. Every action is pinned to a full commit SHA.

The tag-triggered workflow checks out the event commit and uploads build and
package artifacts. It has read-only repository contents permission and does not
create a GitHub release or publish a NuGet package. Automatic publication is
unavailable until a future publication workflow is reviewed and host protections
can be enforced. The current repository plan cannot enforce required reviewers.
This does not prevent a maintainer who can change workflows from adding one.

The standalone publication scripts and their offline tests remain as preparatory
tooling. The GitHub release helper requires existing assets to match byte for byte
on retry; the NuGet helper uses `--skip-duplicate`. The tag verification helpers
check remote tag resolution and reachability from main. A remote tag check and
publication are not atomic, so a future workflow also needs enforceable tag
protection. These scripts are not invoked for publication by the release workflow.

Local packaging validation uses an explicit NuGet configuration with cleared
inherited sources plus an invocation-specific package cache, so another feed or
cached package with the same ID/version cannot substitute for the package under
test. The smoke test compares the native bytes in both concurrent packages and
the installed executable with the local build input.

Installer downloads allow three retries within a 60-second retry-start window,
with a 20-second connection timeout and a 240-second limit per attempt. This
budgets at most 300 seconds per asset request, including its final attempt.
See [curl's retry-time semantics](https://curl.se/docs/manpage.html#--retry-max-time).
Packages published to the local feed have mode `0644`, including under a restrictive
caller umask; existing feed contents are never replaced.
