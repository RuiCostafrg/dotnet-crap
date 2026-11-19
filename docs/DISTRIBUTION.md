# Distribution design

The primary installation channel is the .NET tool. GitHub binary archives,
the shell installer, Homebrew formula generation and the composite GitHub
Action remain supported distribution paths. The immutable GitHub release
[v0.1.0](https://github.com/RuiCostafrg/dotnet-crap/releases/tag/v0.1.0)
already exists and must never be replaced or rebuilt as the same version.
The .NET tool is published on NuGet.org from 0.2.0 onward by
`publish-nuget.yml` (see [NuGet publishing](NUGET_PUBLISHING.md)); `v0.1.0` is
not on NuGet.org. 0.2.0 was pushed directly by the workflow in its tag. Releases
whose tag contains the current workflow are instead staged privately on NuGet.org
with `dotnet package stage push` (GA .NET SDK 11.0.100); the owner promotes each
one manually on the website with 2FA before it becomes public or
restorable, and unpromoted staged content expires. After each release and only
after promotion, `verify-published.yml` installs the package from NuGet.org on
all five tool RIDs (see [published verification](PUBLISHED_VERIFICATION.md)).

npm distribution is retired. This repository no longer builds, installs,
stages, publishes or verifies npm packages. The five existing registry packages
(main and four platform packages, versions 0.0.0 and 0.1.0) were deprecated by the
owner on 2026-11-19, so installs warn "Package no longer supported"; nothing new
is published there. Historical release plans describe
the previous channel set and are not instructions to approve those stages.

## Release assets (`release-assets` artifact)

cargo-dist 0.33.1 (`dist-workspace.toml`, installed by the checksum-pinned
`scripts/install-dist.py`) builds and assembles one flat directory:

| File | Notes |
| --- | --- |
| `dotnet-crap-<target>.tar.xz` + `.sha256` | `aarch64-apple-darwin`, `x86_64-apple-darwin`, `{x86_64,aarch64}-unknown-linux-{gnu,musl}` |
| `dotnet-crap-x86_64-pc-windows-msvc.zip` + `.sha256` | Only for the .NET tool's `win-x64` RID |
| `dotnet-crap-installer.sh` | cargo-dist shell installer, installs to `~/.local/bin` |
| `dotnet-crap.rb` | Homebrew formula for the tap (class `DotnetCrap`) |
| `dotnet-crap.<version>.nupkg` + `.sha256` | .NET tool packed from the archives above |
| `sha256.sum`, `dist-manifest.json` | cargo-dist checksums and manifest |

Each tar archive contains `dotnet-crap-<target>/dotnet-crap` plus `LICENSE`,
`README.md` and `musl-COPYRIGHT`. `dotnet-crap --version` prints exactly
`dotnet-crap <version>`.

Linux channel choices: the GitHub Action and the .NET tool's `linux-x64`
and `linux-arm64` RIDs use the static **musl** build, so one binary runs on any
glibc or musl distribution. The shell installer and Homebrew use cargo-dist's
own selection.

## Workflows

| Workflow | Trigger | Purpose |
| --- | --- | --- |
| `build-release.yml` | `workflow_call` | Build all cargo-dist archives, shell installer and formula; pack and locally install the .NET tool; validate the exact asset set; upload `release-assets` with its digest. Read-only token. |
| `distribution.yml` | PR, main push, dispatch | Build candidate assets, then verify shell/Homebrew installation and the Action. An isolated main-only job uploads SARIF. |
| `installation.yml` | `workflow_call` | Run the generated shell installer and formula against candidate archives through a temporary local tap. |
| `github-action.yml` | `workflow_call` | Exercise the candidate Action with fresh NUnit/MSTest Coverlet coverage on Linux/macOS. |
| `release.yml` | `v*` push, dry-run dispatch | Require qualified main CI, build and verify candidate assets, then request owner approval for GitHub publication. |
| `verify-release.yml` | dispatch or `workflow_call` | Authenticate immutable GitHub assets and install the released .NET tool on all five RIDs. |
| `verify-published.yml` | dispatch | Call release verification, then exercise the real shell, public tap and Action. |

The .NET tool contains `linux-x64`, `linux-arm64`, `osx-x64`, `osx-arm64`
and `win-x64`. Candidate installed-tool CI covers `linux-x64`, `osx-arm64`
and `win-x64`; the published verifier covers all five. Building an archive
alone does not demonstrate installed-tool behavior. Candidate installation
uses local artifacts and a fake `gh`; live verification uses real immutable
release assets and provenance. See [published verification](PUBLISHED_VERIFICATION.md).

## Token isolation and approval gates

1. Planning, build and candidate verification use `contents: read`, no
   environment and no secrets. The candidate Action stays read-only. Only the
   isolated main-push SARIF uploader has `security-events: write`; it checks
   out and executes no repository code.
2. Build emits the SHA-256 of the sorted filename/digest listing of
   `release-assets`. The `publish` job recomputes it and refuses differing
   artifacts. It attests every `release-assets/*` file, including the formula
   and `.nupkg`, then attaches these exact files to the GitHub release.
3. `publish` alone has `contents: write`, `id-token: write` and
   `attestations: write`. It runs on a fresh runner, with no checkout,
   repository scripts, compiled binary or cargo-dist. It waits for owner
   approval in the `release` environment. The npm publication job and its
   second environment gate are removed.

Publication still requires an owner-pushed `v*` tag and
`DOTNET_CRAP_PUBLISH=true`. Dispatch is always a dry run. Preflight checks
Cargo/tag agreement, main ancestry, and successful exact-SHA main push runs of
`ci.yml`, `security.yml` and `distribution.yml`. The publisher checks tag binding
again, refuses any existing release including drafts, and confirms the result
is immutable. Owner-only immutable tags, protected main and required environment
review remain prerequisites. No repository settings are changed by this work.

The tap's separate owner-approved sync verifies the attested formula and opens
a PR in `RuiCostafrg/homebrew-tap`. This repository holds no tap token.

## Verification and integration followups

`action.yml` verifies release-workflow provenance at the exact version tag,
SHA-256 and the executable's reported version. Published checks require an
explicit version and full source SHA, with no moving-version or rebuild fallback.
Release artifacts and verification evidence are retained for 90 days. Never
substitute a rebuild for expired immutable bytes; inspect partial publication
manually or use a new version.

NuGet.org publication is a separate, owner-dispatched workflow on the release
tag, gated by the `release` environment. It only stages the package; the owner
then promotes it on NuGet.org with 2FA, and unpromoted staged content expires. A
GitHub `.nupkg` alone does not prove NuGet.org publication: run the NuGet job in
`verify-published.yml` only after promotion. That job provides the evidence by
comparing the registry package with the authenticated release package and
installing it from NuGet.org only. Existing release and QA schedules remain
disabled.
