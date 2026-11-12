# Verifying a published release

Candidate Distribution CI installs local artifacts and mocks the Action's
GitHub downloads. The workflows here exercise actual public services after a
release exists. No published release has been verified yet.

Both workflows run only from `main` in `RuiCostafrg/dotnet-crap`, with read-only
contents, Actions and attestation access. They require a canonical exact
version without `v` and a full lowercase 40-character expected source SHA.
There is no moving `latest` or candidate-artifact fallback.

Run `verify-release.yml` once the GitHub release exists:

```sh
gh workflow run verify-release.yml --repo RuiCostafrg/dotnet-crap --ref main \
  -f version=0.1.0 -f source-sha=<full-tagged-main-SHA>
```

It resolves the release tag to the expected commit, downloads actual release
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
executable copy. This does not imply publication on NuGet.org.

After npm approvals and the tap formula merge, run all channels:

```sh
gh workflow run verify-published.yml --repo RuiCostafrg/dotnet-crap --ref main \
  -f version=0.1.0 -f source-sha=<full-tagged-main-SHA> \
  -F verify-npm=true -F verify-homebrew=true
```

The combined workflow includes release-asset authentication and all five tool
RIDs, plus:

- npm, pnpm, Yarn Plug'n'Play, Yarn node_modules and Bun installing the exact
  public registry version without package lifecycle scripts on Linux/macOS,
  x64 and ARM64. Registry tarballs must match the exact attested `npm-packages`
  artifact of the matching release run, and executables must match the release
  archives. Tarballs are never reconstructed to substitute for original bytes.
- The actual authenticated shell installer, retaining its real GitHub download
  URLs, on Linux/macOS x64 and ARM64. Installation goes to a temporary home.
- The actual public Homebrew tap on Linux x64 and macOS x64/ARM64. The live
  formula must equal the authenticated release formula before Homebrew runs it;
  the installed executable must match the archive. Linux ARM64 Homebrew is
  explicitly outside this matrix.
- The composite Action checked out at the validated release source SHA, using
  real `gh` downloads and provenance verification. NUnit, MSTest and xUnit run
  on Ubuntu/macOS, collecting fresh Coverlet Cobertura and checking both passing
  and failing scans, exit codes and SARIF paths. These jobs remain read-only;
  SARIF files are retained as artifacts rather than uploaded to code scanning.
  Candidate CI separately exercises SARIF upload from an isolated main-only job.

Before all packages and the formula are public, `verify-npm=false` and/or
`verify-homebrew=false` allow staged verification. A skipped channel is not
evidence that it works; completion requires a run with both inputs true.

## Evidence and recovery limits

Record the run URL, exact version and full source SHA, platform results,
checksum/provenance results, registry integrity and original release-run ID.
Keep the README's pending notices until the respective live channels pass.

The original release and npm run artifacts are retained for 90 days. Complete
verification within that window. An expired or missing npm artifact fails
closed; rebuilding an equivalent archive cannot prove exact published-byte
identity. Reusing the existing immutable artifacts is required for retries.

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
checksum/registry tampering and installed-byte identity. They do not replace
live verification of GitHub, npm or the tap.
