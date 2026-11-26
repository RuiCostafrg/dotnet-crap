# Distribution design

The project ships native archives, a shell installer, a Homebrew formula, a
GitHub Action, and a .NET tool on NuGet.org. See the [README](../README.md)
for installation. For the reasons behind the shared native engine and isolated
publishing workflow, see the [architecture decisions](adr/).

## Release assets (`release-assets` artifact)

cargo-dist 0.34.0 (`dist-workspace.toml`, installed by the checksum-pinned
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
review remain prerequisites.

The tap's separate owner-approved sync verifies the attested formula and opens
a PR in [the Homebrew tap](https://github.com/RuiCostafrg/homebrew-tap).
Candidate formula checks install from a local tap using the candidate archives.
The public tap verifies the released formula before proposing the update and
runs its own macOS and Linux install checks.

## Published verification

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
installing it from NuGet.org only.
