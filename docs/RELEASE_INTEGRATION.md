# Next-release integration

Updated for 0.1.1 preparation on 2026-11-13. This replaces the stale
2026-11-12 first-release audit; historical review inventories and approval
instructions are available in Git history and are not current authorization.

## Established baseline

- [v0.1.0](https://github.com/RuiCostafrg/dotnet-crap/releases/tag/v0.1.0)
  already exists and is immutable, at tagged commit
  `2565412e3d2326d79e98963ed0b8df7de3020659`. Do not change the tag, release
  or its assets to add later scoring or distribution changes.
- PR #41 is merged on main at
  `022484982d9c034cb01fd775b187af6876c533a3`. The integration handoff reports
  successful post-integration main CI, Security, Distribution and full mutation
  validation for that SHA. Those results are baseline evidence only; they
  cannot qualify the version bump or any later integration tree.
- The owner selected .NET-tool-first distribution and dropping npm. Existing
  npm 0.1.0 stages remain unapproved: do not approve them or access credentials.
  Old release and QA schedules are disabled and must remain disabled.
- [0.1.1 preparation notes](RELEASE_0_1_1.md) describe the audited changes and
  score impact. Version preparation is not release-candidate qualification.

## Required ordering

1. **Integrate reviewed prerequisites.** The owner must integrate the version
   preparation, npm-removal and NuGet publisher PRs before qualification. Check
   the combined diff and version agreement across Cargo, the launcher,
   packaging defaults and release/distribution contracts. Do not infer that
   the publisher is configured or that NuGet.org publication works from a local
   `.nupkg` test. Resolve current reviews and required checks through the normal
   protections; do not bypass or alter repository/account settings.
2. **Qualify the exact final main commit.** Record the full resulting SHA.
   Require all protected checks and successful `event=push`, `head_branch=main`
   runs of `ci.yml`, `security.yml` and `distribution.yml` at that exact SHA.
   The release preflight uses the latest run by number/attempt. A PR run,
   manual dispatch, earlier main success or historical mutation result cannot
   substitute. Requalify the resulting SHA after any further main change.
   Preserve source-qualified mutation and platform evidence with their actual
   revisions and limitations.
3. **Future owner decision: tag and publication.** Only after the preceding
   gates, request explicit further permission for a new `v0.1.1` tag at the
   qualified SHA and the relevant owner publication/environment approvals.
   This preparation authorizes no tags, releases, publishing, merges, new
   secrets, settings changes or environment approval. Do not reuse historical
   first-release approvals or reactivate schedules. Never publish npm stages
   as part of this .NET-tool-first plan.
4. **Verify actual published paths after authorized publication.** Use the
   explicit version and full tagged SHA, original assets, checksums and
   provenance. Verify the intended NuGet.org install path as well as the
   remaining supported distribution paths using their reviewed procedures.
   Record per-platform results and run links; do not substitute local candidate
   assets, mocked provenance, moving latest versions or release-attached
   packages for proof of a public NuGet.org install.

## Evidence boundaries

The native package targets are `linux-x64`, `linux-arm64`, `osx-x64`, `osx-arm64`
and `win-x64`. Building archives is not installed-tool verification on every
RID. Record what actually ran on each platform. Local Action, installer and
package contracts are candidate checks and cannot prove live publication.

Remaining adoption on independent C# projects and unsupported scoring cases
retain their own evidence requirements. Historical SDK, benchmark and mutation
measurements certify only their named trees. Refresh live check/review state at
integration time; this document makes no claim about current approvals or
final-main release readiness.
