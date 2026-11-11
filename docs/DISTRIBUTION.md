# Distribution design

dotnet-crap is distributed through the same channels as
[typescript-crap](https://github.com/RuiCostafrg/typescript-crap): GitHub release
archives with a shell installer, a Homebrew formula in
[`RuiCostafrg/homebrew-tap`](https://github.com/RuiCostafrg/homebrew-tap), npm
packages, a composite GitHub Action, and the existing .NET tool package.
Linux and macOS are the supported targets for the new channels. A Windows
archive is still built because the .NET tool package bundles `win-x64`, but no
new channel installs it.

Publication happens only from an owner-pushed `v*` tag, after every channel is
validated against the exact candidate assets built in CI and the owner approves
the `release` environment (see
[Publication prerequisites](#publication-prerequisites)). No release has been
published yet.

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

Linux channel choices: npm, the GitHub Action and the .NET tool's `linux-x64`
and `linux-arm64` RIDs use the static **musl** build, so one binary runs on any
glibc or musl distribution. The shell installer and Homebrew use cargo-dist's
own selection.

## Workflows

| Workflow | Trigger | Purpose |
| --- | --- | --- |
| `build-release.yml` | `workflow_call` | Plan, build every target with cargo-dist, assemble, pack the .NET tool and npm packages from the archives, upload `release-assets` and `npm-packages`. Read-only token. |
| `distribution.yml` | PR, push to main, dispatch | Calls `build-release.yml` for the candidate version, then the three verification workflows below. |
| `npm-install.yml` | `workflow_call` | Installs the packed npm packages with npm, pnpm, Yarn and Bun on Linux and macOS and runs the binary. |
| `installation.yml` | `workflow_call` | Runs the generated shell installer and installs the generated formula from a local, temporary tap with URLs rewritten to the candidate archives. |
| `github-action.yml` | `workflow_call` | Runs the action from this commit against NUnit and MSTest fixtures with Coverlet Cobertura, on Ubuntu and macOS. |
| `release.yml` | `v*` tag, dispatch (dry run) | Calls the same build and verification, then the gated publication jobs. |

Reusable workflows exchange data only through artifacts of the calling run
(`release-assets`, `npm-packages`). Verification workflows never need write
permissions.

## Token isolation (release.yml)

1. `plan`, `build`, verification: `contents: read`, no environment, no
   secrets. All code from the repository and cargo-dist runs here. The Action
   test job additionally holds `security-events: write` so one cell can exercise
   `upload-sarif`; it cannot write contents, releases or packages.
   `build-release.yml` outputs a digest of the exact `release-assets` and
   `npm-packages` listing; `publish` refuses artifacts that differ.
2. `publish` (environment `release`): `contents: write`, `id-token: write`,
   `attestations: write`. No checkout, no compiled binary, no cargo-dist. It
   checks that the tag still names the built commit and that no release (or
   draft) exists, attests `release-assets/*` and `npm-packages/*`, then creates
   the release from the downloaded artifacts.
3. `publish-npm` (environment `release`, second approval): `contents: read`,
   `attestations: read`, `id-token: write` only, for npm trusted publishing.
   It checks every npm executable against the attested release archives first.

The Homebrew formula is not pushed by this repository. The tap's own
owner-approved sync workflow verifies the attested `dotnet-crap.rb` and opens a
PR in the tap, so no tap token exists here.

## GitHub Action

`action.yml` installs a pinned release: it downloads the archive and checksum
with `gh release download`, verifies build provenance with
`gh attestation verify` against `release.yml` at the version tag, verifies
SHA-256, and checks `dotnet-crap --version`. Optional inputs run a scan, write
SARIF, and upload it to code scanning. CI runs the real action against the
candidate archives through a fake `gh` that serves local files. Provenance
verification against an actual published release is pending the first release.

## Publication prerequisites

Publication jobs run only for a `v*` tag push **and** when the repository
variable `DOTNET_CRAP_PUBLISH` is `true`. Unsetting the variable switches
publication off again. It was set on 2026-11-11, after all of these were in place:

- [x] Rulesets protect `main` and `v*` tags (only the owner creates tags; release
  tags cannot move or be deleted).
- [x] The `release` environment has the owner as required reviewer (self-review
  allowed, no admin bypass) and deploys only from `v*` tags.
- [x] npm packages `dotnet-crap`, `dotnet-crap-linux-x64`, `dotnet-crap-linux-arm64`,
  `dotnet-crap-darwin-x64` and `dotnet-crap-darwin-arm64` exist as `0.0.0`
  placeholders, each with trusted publishing for `RuiCostafrg/dotnet-crap`,
  workflow `release.yml`, environment `release`, **stage publish only**: every
  release version stays private until the owner approves its stage with 2FA.
- [x] The tap syncs `dotnet-crap.rb`
  ([homebrew-tap#18](https://github.com/RuiCostafrg/homebrew-tap/pull/18)); before
  the first release its verify job is a successful no-op.

No secrets are required: the GitHub release uses `GITHUB_TOKEN`, npm uses OIDC.
NuGet.org publication of the .NET tool is not automated yet; the attested
`.nupkg` is attached to the GitHub release.
