# Verifying a published release

Candidate Distribution CI installs local artifacts and mocks the Action's
GitHub downloads. The workflows here exercise actual public services after a
release exists. Live verification results must be recorded separately from candidate CI.

Both workflows run only from `main` in `RuiCostafrg/dotnet-crap`, with read-only
contents, Actions and attestation access. They require a canonical exact
version without `v` and a full lowercase 40-character expected source SHA.
There is no moving `latest` or candidate-artifact fallback.

Once the GitHub release exists, set the version and full tagged commit SHA.
The values below are for v1.0.0. Replace both with the release you are verifying,
then run `verify-release.yml`.

```sh
release_version=1.0.0
release_source_sha=6587201921b9c6c048abbd13940cace841ec68a3
gh workflow run verify-release.yml --repo RuiCostafrg/dotnet-crap --ref main \
  -f version="$release_version" -f source-sha="$release_source_sha"
```

It requires an immutable published release, resolves its tag to the expected commit, downloads actual release
assets and verifies their SHA-256 checksums and GitHub build attestations. The
expected signing identity is exactly
`https://github.com/RuiCostafrg/dotnet-crap/.github/workflows/release.yml@refs/tags/v<version>`,
with the same source ref and source digest; self-hosted builders are rejected.
The authenticated bytes cross to downstream jobs only through an artifact of
this verification run.

The released `.nupkg` is installed from an isolated feed and cache on
`linux-x64`, `linux-arm64`, `osx-x64`, `osx-arm64` and `win-x64`. Each job compares
the installed native payload with the authenticated platform archive, then
runs the .NET launcher through version, passing scan and failing gate checks.
NuGet does not preserve Unix execute bits; the launcher makes its own private
executable copy.

The `nuget` channel verifies actual publication on nuget.org. It downloads
`https://api.nuget.org/v3-flatcontainer/dotnet-crap/<version>/dotnet-crap.<version>.nupkg`,
retrying for up to about ten minutes while nuget.org indexing catches up, and
compares its ZIP entry names and entry bytes with the authenticated release
`.nupkg`. Declared sizes are checked before decompression, so the registry copy
may exceed the release package by at most 1 MiB. NuGet.org adds a
repository signature, so the files differ in bytes: only `.signature.p7s` may be
added, and it must not exist in the release asset. Any other missing, extra or
changed entry fails. The published package is then installed with a
`NuGet.Config` that clears every source and adds only
`https://api.nuget.org/v3/index.json` (retried while the version propagates).
Because the registry could answer the install differently from the earlier
download, the package the install actually restored, and every file it extracted
under `tools/`, are compared with the release package again before anything from
it runs. It then receives the same payload, version and scan checks as the
isolated-feed jobs.

After the tap formula merge, verify all channels with the same version and
source SHA.

```sh
gh workflow run verify-published.yml --repo RuiCostafrg/dotnet-crap --ref main \
  -f version="$release_version" -f source-sha="$release_source_sha" \
  -F verify-homebrew=true -F verify-nuget=true
```

`v0.1.0` was never published to NuGet.org; verify it with `-F verify-nuget=false`.

The combined workflow includes release-asset authentication and all five tool
RIDs, plus:

- The actual authenticated shell installer, retaining its real GitHub download
  URLs, on Linux/macOS x64 and ARM64. Installation goes to a temporary home.
- The actual public Homebrew tap on Linux x64 and macOS x64/ARM64. The live
  formula must equal the authenticated release formula before Homebrew runs it;
  the installed executable must match the archive. Linux ARM64 Homebrew is
  explicitly outside this matrix.
- The package published on nuget.org for all five tool RIDs, compared entry by
  entry with the authenticated release `.nupkg` and installed from a config that
  clears all other sources.
- The composite Action checked out at the validated release source SHA, using
  real `gh` downloads and provenance verification. NUnit, MSTest and xUnit run
  on Ubuntu/macOS, collecting fresh Coverlet Cobertura and checking both passing
  and failing scans, exit codes and SARIF paths. These jobs remain read-only;
  SARIF files are retained as artifacts rather than uploaded to code scanning.
  Candidate CI separately exercises SARIF upload from an isolated main-only job.

Before the formula is public, `verify-homebrew=false` allows partial verification.
Before publication, `verify-nuget=false` skips the registry check; it is enabled
by default. A skipped channel is not evidence that it works; full verification
requires the tap check enabled and the package published.

## Evidence and recovery limits

Record the run URL, exact version and full source SHA, platform results,
checksum/provenance results and installed-byte comparisons.
The workflows retain `published-release-evidence` JSON with authenticated asset
hashes alongside the per-framework SARIF reports for 90 days. Reuse existing
immutable published assets for retries; never rebuild to replace published bytes.

The live Homebrew check deliberately tests the current public tap. Verifying
an older version after the formula has advanced fails instead of silently
installing the new version or rewriting a local formula. A new release source
SHA requires its own complete evidence.

## Offline validation

```sh
python3 -m unittest discover -s tests -p 'test_verify_published*.py'
actionlint
```

The main Python contract suite runs on PRs and includes these tests. Offline
tests exercise input validation, provenance command restrictions and rejection,
checksum tampering and installed-byte identity. They do not replace
live verification of GitHub or the tap.

## v1.0.0 verification

Version 1.0.0 was staged by `publish-nuget.yml` run
[45800157272](https://github.com/RuiCostafrg/dotnet-crap/actions/runs/45800157272)
from source `6587201921b9c6c048abbd13940cace841ec68a3` and promoted by the owner.
The Homebrew sync run
[45800165258](https://github.com/RuiCostafrg/homebrew-tap/actions/runs/45800165258)
opened the [formula update](https://github.com/RuiCostafrg/homebrew-tap/pull/31),
which merged after installation checks passed. Published verification run
[45888865138](https://github.com/RuiCostafrg/dotnet-crap/actions/runs/45888865138)
passed all 24 jobs, covering release provenance, the shell installer, Homebrew,
NuGet.org on all five tool RIDs, and the GitHub Action.

## Install the authenticated NuGet package locally

To install the exact bytes built by the release workflow instead, download
`dotnet-crap.1.0.0.nupkg` from the
[v1.0.0 GitHub release](https://github.com/RuiCostafrg/dotnet-crap/releases/tag/v1.0.0)
and save it as `./download/dotnet-crap.1.0.0.nupkg`. The commands below
verify its provenance and install it with an empty cache and only that local feed.

```sh
(
  set -e
  gh attestation verify ./download/dotnet-crap.1.0.0.nupkg --repo RuiCostafrg/dotnet-crap \
    --cert-identity https://github.com/RuiCostafrg/dotnet-crap/.github/workflows/release.yml@refs/tags/v1.0.0 \
    --source-ref refs/tags/v1.0.0 \
    --source-digest 6587201921b9c6c048abbd13940cace841ec68a3
  tmp_dir=$(mktemp -d)
  trap 'rm -rf "$tmp_dir"' EXIT
  cat > "$tmp_dir/NuGet.Config" <<'EOF'
<configuration>
  <packageSources>
    <clear />
  </packageSources>
</configuration>
EOF
  NUGET_PACKAGES="$tmp_dir/packages" dotnet tool install --global dotnet-crap \
    --version 1.0.0 --configfile "$tmp_dir/NuGet.Config" --add-source "$PWD/download"
)
```
