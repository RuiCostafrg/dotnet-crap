# Repository security parity with typescript-crap

This records how RuiCostafrg/dotnet-crap's host-side protections were aligned with
[RuiCostafrg/typescript-crap](https://github.com/RuiCostafrg/typescript-crap).
"Before" values were read from the GitHub REST API on 2026-11-10. "After" values
were read back from the API after applying them (see [Verification](#verification)).

Status legend: **applied** = set and read back; **pending** = not yet applied.

## Settings comparison

| Area | typescript-crap (reference) | dotnet-crap before | dotnet-crap after |
| --- | --- | --- | --- |
| Ruleset: owner-reviewed changes to `main` | 1 approval, dismiss stale, code-owner review, thread resolution, extra approval for unattributed changes; admin bypass in `pull_request` mode | none | same as reference — **pending** |
| Ruleset: main integrity and mandatory CI | no deletion, no force-push, strict required checks | none | same rules; dotnet-crap check names (below) — **pending** |
| Ruleset: only owner creates `v*` tags | creation restricted, admin bypass `always` | none | same — **pending** |
| Ruleset: `v*` tags cannot move or be deleted | update + deletion blocked, no bypass | none | same — **pending** |
| Secret scanning | enabled | disabled | enabled — **pending** |
| Secret scanning push protection | enabled | disabled | enabled — **pending** |
| Dependabot vulnerability alerts | enabled | disabled | enabled — **pending** |
| Dependabot security update PRs | disabled | disabled | disabled (unchanged, matches) |
| Private vulnerability reporting | enabled | disabled | enabled — **pending** |
| Allowed actions | selected: GitHub-owned + `dtolnay/rust-toolchain@*`, `taiki-e/install-action@*` | all | same allowlist — **pending** |
| SHA pinning required | yes | no | yes — **pending** |
| Default `GITHUB_TOKEN` permissions | read; cannot approve PRs | read; cannot approve PRs | unchanged (matches) |
| `release` environment | required reviewer RuiCostafrg; custom policy `v*` tags only; admins cannot bypass | exists, no rules; admins can bypass | same as reference — **pending** |
| Delete branch on merge | on | off | on — **pending** |
| Auto-merge allowed | on | off | on — **pending** |
| Merge methods | merge, squash, rebase | merge, squash, rebase | unchanged (matches) |

## Repository files

| File | typescript-crap | dotnet-crap before | dotnet-crap after (this PR) |
| --- | --- | --- | --- |
| `.github/CODEOWNERS` | `* @RuiCostafrg` | missing | `* @RuiCostafrg` |
| `.github/dependabot.yml` | cargo, github-actions weekly | missing | cargo, github-actions, NuGet weekly |
| `.github/actionlint.yaml` | extra runner labels | missing | not needed; actionlint 1.7.12 knows every label used |
| Actions pinned to full commit SHAs | yes | yes (all five verified against their tags) | yes |
| Workflow-level `permissions:` | `contents: read` | `contents: read` on every workflow | unchanged |
| Checkout `persist-credentials: false` | yes | no | yes, on every checkout |
| actionlint CI job (`workflows`) | yes, required | missing | added, required |
| PR-time `cargo audit` | in CI | `security.yml` `audit` | unchanged, required |
| Scheduled advisories | `advisories.yml` daily cargo audit | missing | `advisories.yml` daily cargo audit + NuGet vulnerable-package check |

## Required status checks on `main`

| typescript-crap | dotnet-crap |
| --- | --- |
| `test (1.90)`, `test (stable)` | `rust` |
| `workflows` | `workflows` |
| `audit` | `audit` |
| `coverage` | `packaging-smoke` |
| `mutants` | `mutation` |
| `platform-behavior (macos-latest)` | `dotnet-e2e (macos-15, osx-arm64, dotnet-crap)` |
| `platform-behavior (windows-latest)` | — (Windows not required, see below) |
| — | `dotnet-e2e (ubuntu-24.04, linux-x64, dotnet-crap)` |

All checks come from GitHub Actions (integration 15368). The strict policy requires
branches to be up to date with `main` before merging.

## Intentional differences

- **Windows is not a required check.** The `dotnet-e2e (windows-latest, …)` leg still
  runs on every PR, but supported-platform scope is currently Linux and macOS.
- **CI runs for every author.** typescript-crap skips its CI jobs unless the owner
  triggers them. dotnet-crap's PR workflows have read-only tokens, no secrets and no
  persisted credentials, so they keep running for Dependabot and contributor PRs.
  The scheduled `advisories.yml` keeps the owner-or-schedule guard.
- **NuGet is tracked by Dependabot.** The shipped launcher and the NUnit, MSTest and
  xUnit Coverlet fixtures are tracked and grouped into one PR. Fixture versions back
  recorded coverage evidence (see [COVERLET.md](COVERLET.md)). A bump that changes hit
  signatures must refresh that evidence in the same PR.
- **No `reviewers:` key in `dependabot.yml`.** GitHub deprecated that option in favour
  of CODEOWNERS, which now assigns RuiCostafrg.
- **The `release` environment exists, but no job uses it yet.** `release.yml` only
  builds artifacts. The environment is in place for a future, reviewed publication job.

## What these protections do not cover

The owner holds the admin role. That role can bypass the pull-request ruleset in
`pull_request` mode and can create `v*` tags. A maintainer who can change workflows
can still add a new job. Required reviews and checks protect `main` from unreviewed
changes; they do not protect against a compromised owner account.

## Verification

Commands used to read state (substitute either repository):

```sh
gh api repos/RuiCostafrg/<repo>/rulesets
gh api repos/RuiCostafrg/<repo>/rulesets/<id>
gh api repos/RuiCostafrg/<repo> --jq '{security_and_analysis,delete_branch_on_merge,allow_auto_merge}'
gh api repos/RuiCostafrg/<repo>/actions/permissions
gh api repos/RuiCostafrg/<repo>/actions/permissions/selected-actions
gh api repos/RuiCostafrg/<repo>/actions/permissions/workflow
gh api repos/RuiCostafrg/<repo>/environments/release
gh api repos/RuiCostafrg/<repo>/environments/release/deployment-branch-policies
gh api -i repos/RuiCostafrg/<repo>/vulnerability-alerts   # 204 = enabled
gh api repos/RuiCostafrg/<repo>/private-vulnerability-reporting
```
