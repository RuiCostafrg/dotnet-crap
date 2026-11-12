# First release follow-up

Status checked on 2026-11-12. Follow these steps in order. The owner subsequently
authorized steps 1–3, including integration and the first release after reviewed
checks pass. Required host review and account/2FA approvals remain in force;
this authorization does not permit repository-protection changes or bypasses.

## Confirmed starting point

- [x] [Distribution PR #16](https://github.com/RuiCostafrg/dotnet-crap/pull/16)
  is open and mergeable at `018d2c006cc1527658715e7c450a5d56d37a3d39`, with
  38 successful checks, one neutral result and two skipped results.
- [x] No GitHub releases exist yet. Candidate installation CI is green; real
  published-channel installation and provenance verification remain unproven.
- [x] `DOTNET_CRAP_PUBLISH=true`; the `release` environment requires owner review
  and disallows admin bypass. Tag publication is enabled subject to its gates.
- [x] [Tap sync PR #18](https://github.com/RuiCostafrg/homebrew-tap/pull/18)
  is merged. Its pre-release no-op does not prove a real formula update works.
- [ ] Reconfirm npm's five stage-only trusted publishers before release. Their
  setup and `0.0.0` placeholders are recorded in [DISTRIBUTION.md](DISTRIBUTION.md),
  but their account settings were not independently rechecked for this plan.
- [ ] Enable GitHub immutable releases with owner approval. The API returned
  `enabled=false` during integration; tag protections alone do not lock assets.
  Publication preflight and the live verifier now require immutable releases.
- [ ] Owner confirms revocation of the old npm token labelled **Android**.
  Deleting local copies did not revoke it on npmjs.com. Do not copy it into chat.

## 1. Prepare published-release verification

Owner: implementation agent; reviewed through a PR before tagging.

- [ ] Add read-only, manually dispatched verification pinned to an explicit
  release version and full source commit SHA. The repository currently has no
  `verify-release.yml` or `verify-published.yml` equivalent. Keep candidate tests.
- [ ] Fetch actual GitHub assets and verify SHA-256 and build provenance,
  including the expected release workflow, tag and source commit. Exercise
  tampered-payload and incorrect-identity rejection in offline contract tests.
- [ ] Test npm registry installation with npm, pnpm, Yarn and Bun on Linux and
  macOS, both architectures. Compare registry tarball integrity with the exact
  attested tarballs from the release run; compare bundled executables with the
  release archives. Version output alone is insufficient.
- [ ] Install the real shell installer on Linux/macOS x64 and ARM64. Install
  Homebrew from the actual tap on its validated platforms (currently Linux x64
  and both macOS architectures). Keep any Linux ARM64 Homebrew exclusion explicit.
- [ ] Run the Action pinned to the release source SHA, using real `gh` downloads
  and attestation verification instead of CI's fake `gh`. Collect fresh Coverlet
  Cobertura and test passing/failing scans and SARIF on Ubuntu/macOS. Include
  NUnit, MSTest and xUnit in the published-path smoke suite.
- [ ] Install the exact released `.nupkg` from an isolated local feed and run it
  on all five supported RIDs: linux-x64, linux-arm64, osx-x64, osx-arm64, win-x64.
  Existing tool-install CI covers only three host RIDs; native archive smoke
  tests do not prove .NET launcher installation on the other two.

Completion evidence: reviewed workflow changes and green offline/candidate
checks. Live checks are explicitly pending until steps 3–5 make assets public.

## 2. Integrate and qualify the release commit

Owner: maintainer approves merge; implementation agent prepares/reviews changes.

- [ ] Recheck PR #16 against current `main`, resolve any drift and address
  actionable reviews. Rerun required checks if the tree changes.
- [ ] Correct stale status text in `PLAN.md` and `docs/IMPLEMENTATION_PLAN.md`:
  publication is configured, but the first release is not yet validated.
- [ ] Merge only with explicit owner approval. Wait for `ci.yml`, `security.yml`
  and `distribution.yml` push runs to succeed on the exact `main` commit to tag;
  PR-head checks alone do not satisfy release preflight.
- [ ] Confirm Cargo/package versions, intended release version (`0.1.0` unless
  deliberately changed), tag protections and immutable-release settings.
- [ ] Optionally dispatch `release.yml` on `main` for a final dry run. Dispatch
  never publishes; it cannot prove the OIDC publishing path.

Completion evidence: recorded main SHA, matching version and successful checks.

## 3. Publish GitHub assets and stage npm packages

Owner: maintainer explicitly authorizes tag creation and publication.

- [ ] Create the version tag at the qualified main SHA and push it only after
  approval. This starts the release workflow.
- [ ] Review the verified asset list/digest and approve the first `release`
  environment gate for GitHub publication. Confirm checksums, provenance,
  formula, installer and `.nupkg` are present on the immutable release.
- [ ] Approve the second environment gate for npm staging. Confirm all five
  stage IDs and integrity values in the workflow summary.
- [ ] Approve the four platform npm stages with 2FA first. Wait until every exact
  platform version is visible in the registry, then approve `dotnet-crap`.
  Verify provenance, integrity, optional dependency versions and dist-tags.

Never rebuild a package over an existing feed or replace published bytes.
Retry with the same verified artifacts. If a draft, failed stage or partial
publication remains, inspect it before retrying; changed bytes require a new
version. Disabling `DOTNET_CRAP_PUBLISH` stops future publication jobs, but does
not undo assets already published.

## 4. Complete the Homebrew update

Owner: maintainer approves tap workflow/review/merge as required.

- [ ] Let the scheduled tap sync run, or dispatch it with explicit approval.
  Confirm it verifies the real formula attestation and opens a formula PR.
- [ ] Review formula URLs, hashes and version, then approve/merge the tap PR
  through its documented protection process.
- [ ] Run the real-tap installation checks from step 1.

Completion evidence: formula PR URL, sync run and successful installation runs.

## 5. Verify and document public availability

Owner: implementation agent; documentation changes reviewed through a PR.

- [ ] Run the published-path workflows from step 1 against the exact release.
  Record run URLs, source SHA, platform matrix and package integrity evidence.
- [ ] Remove README's pending notice only for channels that pass. Replace the
  Action example's `<full commit SHA>` with a verified release commit SHA.
- [ ] Keep NuGet.org installation labelled pending until step 6 succeeds;
  meanwhile document installation from the attested GitHub `.nupkg`.
- [ ] Update PLAN step 9 and the implementation plan with evidence. Do not mark
  independent adoption or NuGet publication complete merely because npm works.

## 6. Separate follow-ups: NuGet.org and independent adoption

- [ ] Verify ownership/reservation of the `dotnet-crap` NuGet package ID. Add a
  reviewed, isolated publishing job using trusted publishing, the `release`
  environment and the exact verified `.nupkg`. Record any required NuGet account
  setup; never silently fall back to a long-lived publishing token.
- [ ] With explicit owner approval, publish and then test installation from
  NuGet.org on all five RIDs. Reuse exact package bytes for retries.
- [ ] Evaluate 3–5 independent C# repositories at pinned commits, including a
  library, web application and multi-project solution. Record coverage mapping
  limitations, scores, runtime/memory and integration friction.

These are remaining PLAN step 9 work. They can be separate PRs and do not block
the first GitHub/npm/Homebrew release if their pending status stays explicit.
