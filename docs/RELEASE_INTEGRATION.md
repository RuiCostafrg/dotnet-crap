# First-release integration

This is a read-only audit snapshot from 2026-11-12, not release authorization or
published-path evidence. Refresh the source, checks, reviews and protections
before integration. The integration owner performs actual merges, tagging and
publication approvals after all workers finish; documentation workers do none
of those operations.

## Audited source and publication state

- [PR #16](https://github.com/RuiCostafrg/dotnet-crap/pull/16),
  `feat/distribution`, is open at
  `018d2c006cc1527658715e7c450a5d56d37a3d39`, based on `main` at
  `eeb4c805634dc07b441fd2694fe0c46369ac7f2a`. Main is an ancestor of the
  candidate (0 main-only, 14 candidate-only commits); GitHub reports
  `MERGEABLE` but merge state `BLOCKED`.
- Cargo and the .NET launcher both declare `0.1.0` in the candidate and main.
  This is the planned first tag `v0.1.0`, not an already released version.
- Candidate checks: 38 `SUCCESS`, 2 `SKIPPED`, 1 `NEUTRAL`, no failing or
  pending checks in the inspected rollup. The successful
  [Distribution run](https://github.com/RuiCostafrg/dotnet-crap/actions/runs/43211779595)
  is a `pull_request` run for the candidate SHA, not a final-main push run or
  published installation proof.
- The GitHub releases API returned an empty list. Repository variable
  `DOTNET_CRAP_PUBLISH` is `true` (set 2026-11-11). This configures the tag-time
  publication gate; it does not mean that a release has been published.
- [homebrew-tap#18](https://github.com/RuiCostafrg/homebrew-tap/pull/18) merged
  2026-11-11 at `51ff345a6725eb1753964a2049e2d9e84dbb0de4`. It prepares sync;
  the pre-release successful no-op does not prove live formula installation.
- npm placeholder and stage-only trusted-publisher setup are recorded in
  [Distribution](DISTRIBUTION.md). This audit did not independently inspect npm
  account configuration or install a published `0.1.0` package.

## Merge protections and current blockers

The active [main integrity ruleset](https://github.com/RuiCostafrg/dotnet-crap/rules/27034556)
requires strict up-to-date checks from GitHub Actions (integration ID 15368),
with no bypass actors. Exact required check contexts:

```text
rust
workflows
packaging-smoke
dotnet-e2e (ubuntu-24.04, linux-x64, dotnet-crap)
dotnet-e2e (macos-15, osx-arm64, dotnet-crap)
audit
mutation
```

Windows installed-tool CI runs but is not a required context. Distribution is
not listed as a branch-required context; nevertheless the release preflight
requires its successful push run on the exact main SHA, alongside `ci.yml` and
`security.yml`. Main deletion and non-fast-forward updates are prohibited.

The active [review ruleset](https://github.com/RuiCostafrg/dotnet-crap/rules/27034553)
requires one approval, code-owner review, dismissal of stale approvals and
resolution of review threads. `CODEOWNERS` assigns every path to `RuiCostafrg`.
The inspected review list has one `COMMENTED` Sourcery review at historical
commit `d34e593c217a01d2feafad2e40b726d5e33ebb1a` and no approval; all ten
threads remain unresolved (two are outdated). These are outstanding review
requirements even with green checks. The ruleset exposes an administrator
pull-request-only bypass actor; this plan does not use a bypass or alter rules.
The legacy branch-protection endpoint returns 404, but the active rulesets above
apply: that response does not mean main is unprotected.

Review-thread inventory below records reviewer claims requiring adjudication;
it does not certify each claim as a reproduced defect. An outdated thread is
still unresolved until the reviewer/integration owner closes it appropriately.

| Thread | Reviewer concern | Audit disposition |
| --- | --- | --- |
| [4538694745](https://github.com/RuiCostafrg/dotnet-crap/pull/16#discussion_r4538694745) | PR-controlled Action receives `security-events: write` | Permission is present in the current candidate matrix and caller; resolve the boundary before integration. |
| [4538694754](https://github.com/RuiCostafrg/dotnet-crap/pull/16#discussion_r4538694754) | Out-of-order npm stage approval can move `latest` backwards | Adjudicate stage-order/release-serialization behavior; first-release instructions approve platform packages first. |
| [4538694763](https://github.com/RuiCostafrg/dotnet-crap/pull/16#discussion_r4538694763) | Shared npm concurrency can replace pending releases | Review multi-release queuing/recovery; do not treat a green candidate as proof. |
| [4538694776](https://github.com/RuiCostafrg/dotnet-crap/pull/16#discussion_r4538694776) | Fork PR distribution coverage | Check actual event/permission behavior; the repository equality guard alone does not establish which upstream fork-PR jobs run. Documentation qualifies evidence to the inspected upstream candidate. |
| [4538694792](https://github.com/RuiCostafrg/dotnet-crap/pull/16#discussion_r4538694792) | Leftover draft blocks release retry | Current publisher intentionally refuses an existing release, including drafts; manual inspection is required, not a blind rerun. |
| [4538694802](https://github.com/RuiCostafrg/dotnet-crap/pull/16#discussion_r4538694802) | Older formula PR can downgrade tap | Outdated thread; inspect the merged tap implementation and current formula ordering before resolving. |
| [4538694813](https://github.com/RuiCostafrg/dotnet-crap/pull/16#discussion_r4538694813) | Tap branch creation prevents retries | Outdated thread; inspect actual tap workflow recovery before resolving. |
| [4538694820](https://github.com/RuiCostafrg/dotnet-crap/pull/16#discussion_r4538694820) | Expired npm artifacts prevent retry | Current npm artifact retention is 30 days; establish the supported approval/recovery window. |
| [4538694829](https://github.com/RuiCostafrg/dotnet-crap/pull/16#discussion_r4538694829) | SARIF root with spaces | Review the actual Bash array expansion and existing regression evidence; allegation is not a reproduced failure here. |
| [4538694838](https://github.com/RuiCostafrg/dotnet-crap/pull/16#discussion_r4538694838) | Dynamic subprocess command | Audit trusted command construction; a list-form subprocess invocation alone does not demonstrate shell injection. |

## Ordered first-release gates

1. **Prepare live verification.** Add and review read-only workflows with an
   explicit version and full expected source SHA. Check real GitHub assets,
   checksums and provenance, public npm packages, shell installer, public tap,
   the pinned Action and the release-attached .NET tool. Reject missing or
   mismatched publication; never silently fall back to local candidate assets,
   mocked provenance, a moving latest version or an unqualified source.
   These workflows must exist before integration/tagging; real successful
   verification cannot precede publication. The companion implementation owns
   its workflow/script/test files and published-verification documentation.
2. **Integrate and qualify main.** Resolve the review inventory, obtain the
   required review, integrate reviewed distribution/live-verification/docs
   changes, and record the full final `main` SHA. At that exact SHA require the
   protected contexts above and successful `event=push`, `head_branch=main`
   workflow runs of `ci.yml`, `security.yml` and `distribution.yml`. The release
   preflight selects the latest run by run number/attempt. A PR run, a manual
   dispatch, or a success on an earlier main commit cannot satisfy this gate.
   Any later main change requires qualification of its resulting SHA.
3. **Tag and publish.** The integration owner creates `v0.1.0` at the qualified
   main SHA after all workers finish. The release workflow validates ancestry,
   tag/version agreement and the exact-SHA workflow results, rebuilds and
   verifies candidate assets, then waits for the owner-approved `release`
   environment. Approve GitHub publication first; a second environment approval
   permits npm staging only. Match stage integrities, approve four platform
   stages with owner 2FA, confirm each is public with `npm view`, then approve
   the main npm package. Complete the tap's own approval and reviewed formula
   integration. Run live verification at `0.1.0` and the full tagged SHA, retain
   run links/results, and update pending user-facing claims only after success.

The [tag creation ruleset](https://github.com/RuiCostafrg/dotnet-crap/rules/27034554)
restricts `v*` creation through the configured administrator role; release
preflight additionally checks owner actor ID/login. The
[tag immutability ruleset](https://github.com/RuiCostafrg/dotnet-crap/rules/27034555)
prohibits moving/deleting those tags with no bypass actors. The inspected
`release` environment requires `RuiCostafrg`, allows self-review, disallows
administrator bypass, and accepts only `v*` tags. Do not change these settings
to pass integration or publication.

## Evidence gaps and separate followups

The package supports `linux-x64`, `linux-arm64`, `osx-x64`, `osx-arm64` and
`win-x64`. Current installed-tool CI covers only `linux-x64`, `osx-arm64` and
`win-x64`; building all archive targets does not verify installation on
`linux-arm64` or `osx-x64`. Published verification must include those two and
retain per-RID evidence for all five. Candidate Action verification uses fake
`gh`, candidate npm uses local packages, shell uses candidate assets, and
Homebrew uses a temporary rewritten tap. None proves a live distribution path.

NuGet.org package-ID ownership and an automated trusted publisher remain
separate followups: release-attached `.nupkg` verification is not NuGet.org
publication. Adoption on 3–5 independent pinned C# projects (library, web app,
multi-project solution) also remains uncompleted. Preserve historical mutation,
SDK and benchmark evidence at its named revisions; it cannot certify the final
main SHA or a live first release.
