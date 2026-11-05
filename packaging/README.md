# Distribution packaging

`dotnet-crap` is a .NET 10 global tool NuGet package. `scripts/pack-dotnet-tool.sh <native-stage> [version]` expects native executables at `<native-stage>/<rid>/dotnet-crap[.exe]`, then packs them into the tool package. The launcher selects the matching bundled executable and forwards arguments and exit status.

## Staging and concurrent packs

The pack script copies the launcher sources (not `bin/`, `obj/`, or `native/`) into a per-invocation work directory under `TMPDIR`. Native executables are staged there, and `dotnet pack` builds there too, so two packs from one checkout never share native, `bin`, or `obj` paths. The checkout's `packaging/dotnet-crap-launcher/native/` is no longer written. The finished `.nupkg` is copied atomically to `<native-stage>/feed/`.

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

The `publish` job is the only job that writes to GitHub or NuGet, and it needs the `release` environment:

1. **Trusted main:** `scripts/verify-tag-on-main.sh` fails unless the tagged commit is reachable from `origin/main`, so a tag on a branch-only commit cannot publish. It runs from the tagged checkout, so it catches mistakes but cannot stop a tag that carries its own modified workflow. The environment's reviewer and tag policies are the enforcement point for that case.
2. **GitHub release, safe to retry:** `scripts/publish-github-release.sh` creates the release with `--verify-tag`, so gh never creates a tag. If creation fails and the release is readable, the script downloads the existing assets into an isolated temporary directory and requires byte-for-byte matches. It never overwrites published assets. A failed download or mismatch fails the retry. If the release is not readable, it reports the create error and exits non-zero.
3. **NuGet, safe to retry:** `scripts/publish-nuget.sh` pushes with `--skip-duplicate`, so a version that is already published counts as success. Other push errors still fail the job. A missing `NUGET_API_KEY` skips NuGet with an explicit notice.

A retry rebuilds the artifacts from the same tag. Rebuilt archives may not be byte-identical, while NuGet keeps the first pushed `.nupkg`. After a NuGet-only retry, compare the GitHub asset digest with the published package before you rely on it.

### Required host settings (not configured by this repository)

The workflow names an environment, but naming it protects nothing. A missing environment is created on first use with no protection, and a missing secret is simply absent. Before the first release, a repository administrator must configure:

- **`release` environment with required reviewers.** Approval must be required before the `publish` job starts.
- **Deployment policy for the `release` environment limited to `v*` tags.** Add a protected tag ruleset for `v*` that only maintainers can create or update, so an untrusted tag cannot reach the environment.
- **`NUGET_API_KEY` must be an environment secret of `release`, not a repository secret.** A repository secret is available to every workflow in the repository; environment scope limits it to approved `publish` runs.

This repository's current GitHub state, as checked with the read-only API, has no environments and no repository secrets. Rulesets were not readable on this private plan. Until the settings above are in place, do not add `NUGET_API_KEY` anywhere.

Publication is disabled by default. Set the repository variable
`RELEASE_PUBLISH_ENABLED=true` only after configuring supported required reviewers,
protected release tags, and environment-scoped credentials. This opt-in is a
rollout safeguard, not a substitute for host enforcement against a workflow
modified in an untrusted tag. If the GitHub billing plan cannot enforce required
reviewers for this private repository, leave publication disabled.

A retry verifies existing GitHub assets byte-for-byte and never overwrites them.
Rerun only the failed publish job with its original retained artifacts. If rebuilt
artifacts differ, publication fails closed; use a new version rather than mutating
the published release. Missing existing assets also fail verification.
