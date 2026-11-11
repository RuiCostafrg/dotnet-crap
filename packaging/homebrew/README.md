# Homebrew delivery: RuiCostafrg/homebrew-tap

cargo-dist generates `dotnet-crap.rb` (class `DotnetCrap`) as part of the
`release-assets` artifact; it is never committed here. The formula travels to
[`RuiCostafrg/homebrew-tap`](https://github.com/RuiCostafrg/homebrew-tap)
through the tap's **own** owner-approved sync workflow, exactly like
`ts-crap.rb` does for typescript-crap. This repository never pushes to the tap
and holds no tap token (see [token isolation in the distribution design](../../docs/DISTRIBUTION.md)).

## Pipeline

1. Pre-publication: `.github/workflows/installation.yml` installs the candidate
   formula from a temporary `brew tap-new --no-git` tap with every download URL
   rewritten to the local candidate archives (checksum lines unchanged), runs
   the installed binary, and confirms by byte identity which archive (GNU or
   musl) was installed.
2. After publication: the tap's sync workflow verifies the released formula's
   provenance and opens a pull request in the tap; the owner reviews and merges
   it, and tap CI validates the change.

## Tap configuration

[homebrew-tap#18](https://github.com/RuiCostafrg/homebrew-tap/pull/18) (merged
2026-11-11) adds `verify-dotnet-crap` and `publish-dotnet-crap` to the tap's
`.github/workflows/sync.yml`, next to the unchanged ts-crap jobs:

- The read-only verify job runs hourly or on owner dispatch. It finds a newer
  stable dotnet-crap release, downloads `dotnet-crap.rb`, and runs
  `gh attestation verify` with the exact signer identity
  `https://github.com/RuiCostafrg/dotnet-crap/.github/workflows/release.yml@refs/tags/$TAG`,
  the tag's commit digest and `--deny-self-hosted-runners`, then `ruby -c`.
  Until the first release, `releases/latest` returns 404 and the job succeeds
  with nothing to sync; an unreachable repository fails it.
- The publish job, in the owner-approved `homebrew` environment, creates
  `Formula/dotnet-crap.rb` on the first sync (and updates it later) on branch
  `releases/dotnet-crap-$TAG`. It dispatches the tap's read-only CI, then opens
  the PR. The owner merges it after CI installs the formula on macOS and Linux.

The tap's CI syntax-checks every formula and installs dotnet-crap once it exists.

## What this repository does not do

It never pushes to the tap, never holds a tap token, and never merges the sync
PR.
