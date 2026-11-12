# Repository security parity with typescript-crap

This records how RuiCostafrg/dotnet-crap's host-side protections were aligned with
[RuiCostafrg/typescript-crap](https://github.com/RuiCostafrg/typescript-crap).
"Before" values were read from the GitHub REST API on 2026-11-10. "After" values
were applied on 2026-11-11 and read back from the API (see [Verification](#verification)).

Status legend: **applied** = set, then read back and compared with the reference.

## Settings comparison

| Area | typescript-crap (reference) | dotnet-crap before | dotnet-crap after |
| --- | --- | --- | --- |
| Ruleset: owner-reviewed changes to `main` | 1 approval, dismiss stale, code-owner review, thread resolution, extra approval for unattributed changes; admin bypass in `pull_request` mode | none | same as reference — **applied** |
| Ruleset: main integrity and mandatory CI | no deletion, no force-push, strict required checks | none | same rules; dotnet-crap check names (below) — **applied** |
| Ruleset: only owner creates `v*` tags | creation restricted, admin bypass `always` | none | same — **applied** |
| Ruleset: `v*` tags cannot move or be deleted | update + deletion blocked, no bypass | none | same — **applied** |
| Secret scanning | enabled | disabled | enabled — **applied** |
| Secret scanning push protection | enabled | disabled | enabled — **applied** |
| Dependabot vulnerability alerts | enabled | disabled | enabled — **applied** |
| Dependabot security update PRs | disabled | disabled | disabled (unchanged, matches) |
| Private vulnerability reporting | enabled | disabled | enabled — **applied** |
| Allowed actions | selected: GitHub-owned + `dtolnay/rust-toolchain@*`, `taiki-e/install-action@*` | all | same allowlist — **applied** |
| SHA pinning required | yes | no | yes — **applied** |
| Default `GITHUB_TOKEN` permissions | read; cannot approve PRs | read; cannot approve PRs | unchanged (matches) |
| `release` environment | required reviewer RuiCostafrg; custom policy `v*` tags only; admins cannot bypass | exists, no rules; admins can bypass | same as reference — **applied** |
| Delete branch on merge | on | off | on — **applied** |
| Auto-merge allowed | on | off | on — **applied** |
| Merge methods | merge, squash, rebase | merge, squash, rebase | unchanged (matches) |

## Repository files

| File | typescript-crap | dotnet-crap before | dotnet-crap after (this PR) |
| --- | --- | --- | --- |
| `.github/CODEOWNERS` | `* @RuiCostafrg` | missing | `* @RuiCostafrg` |
| `.github/dependabot.yml` | cargo, github-actions weekly | missing | cargo, github-actions, NuGet weekly |
| `.github/actionlint.yaml` | extra runner labels | missing | not needed; actionlint 1.7.12 knows every label used |
| Actions pinned to full commit SHAs | yes | yes (all five verified against their tags) | yes |
| Workflow-level `permissions:` | `contents: read` | `contents: read` on every workflow | unchanged |
| Checkout `persist-credentials: false` | yes | no | yes, on every checkout (including `benchmark.yml`) |
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
- **The `release` environment gates publication.** In `release.yml`, only the
  `publish` (GitHub release with attestations) and `publish-npm` (staged npm trusted
  publishing) jobs use it. Both also require the `DOTNET_CRAP_PUBLISH` variable;
  see [DISTRIBUTION.md](DISTRIBUTION.md).

## What these protections do not cover

The owner holds the admin role. That role can bypass the pull-request ruleset in
`pull_request` mode and can create `v*` tags. A maintainer who can change workflows
can still add a new job. Required reviews and checks protect `main` from unreviewed
changes; they do not protect against a compromised owner account.

## Verification

dotnet-crap ruleset IDs: 27034553 (owner-reviewed changes), 27034556 (main integrity
and CI), 27034554 (owner-only release tags), 27034555 (immutable release tags).
After applying, each ruleset was normalised with `jq` and diffed against its
typescript-crap counterpart (25595099, 25595100, 25595101, 25595103). All four were
identical apart from the adapted required-check list. Every repository, Actions,
token, environment, vulnerability-alert and private-reporting value in the tables
above was read back from both repositories and matched.

Applying the strict CI ruleset means that open PRs created before the `workflows`
job existed (for example #10) need a rebase onto `main` before they can merge.

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
