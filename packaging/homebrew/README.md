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

## Current state of the tap (read-only, checked 2026-11-10)

`RuiCostafrg/homebrew-tap` contains only `Formula/ts-crap.rb`. Its
`.github/workflows/sync.yml` runs hourly (cron `17 * * * *`) on `main`:

- a read-only `verify` job finds the newest stable typescript-crap release,
  compares it against the version declared in `Formula/ts-crap.rb`, checks that
  no update PR is already open, downloads the released `ts-crap.rb`, verifies
  it with `gh attestation verify` against
  `RuiCostafrg/typescript-crap/.github/workflows/release.yml`, the tag ref and
  the source commit digest (`--source-digest`), and syntax-checks it with
  `ruby -c`;
- a `publish` job in the owner-approved `homebrew` environment uploads the
  verified formula to a branch `releases/ts-crap-$TAG` via the contents API,
  opens the owner-reviewed PR, and dispatches the tap's `ci.yml` on the branch.

The tap currently syncs **only** `ts-crap.rb`; there is no dotnet-crap entry.

## Proposed change to the tap

Append the following job pair to the tap's `.github/workflows/sync.yml` (it
generalizes the existing sync to a second source: `Formula/dotnet-crap.rb`
from `RuiCostafrg/dotnet-crap`, attestation-verified against dotnet-crap's
`.github/workflows/release.yml`, branching as `releases/dotnet-crap-$TAG`).
Duplicating the jobs instead of matrix-templating the existing ones keeps the
proven ts-crap path byte-for-byte untouched; the duplication is the exact,
reviewable change.

```yaml
  verify-dotnet-crap:
    if: >-
      github.repository == 'RuiCostafrg/homebrew-tap' &&
      github.ref == 'refs/heads/main' &&
      (github.event_name == 'schedule' || github.actor_id == '35194998')
    runs-on: ubuntu-26.04
    timeout-minutes: 10
    outputs:
      changed: ${{ steps.release.outputs.changed == 'true' && steps.pending.outputs.exists == 'false' }}
      tag: ${{ steps.release.outputs.tag }}
    steps:
      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1
        with:
          persist-credentials: false
      - name: Find a newer stable release
        id: release
        env:
          GH_TOKEN: ${{ github.token }}
        run: |
          gh api repos/RuiCostafrg/dotnet-crap/releases/latest > "$RUNNER_TEMP/release.json"
          python3 - <<'PYTHON'
          import json, os, pathlib, re
          release = json.loads(pathlib.Path(os.environ['RUNNER_TEMP'], 'release.json').read_text())
          tag = release['tag_name']
          if release['draft'] or release['prerelease'] or not re.fullmatch(r'v[0-9]+\.[0-9]+\.[0-9]+', tag):
              raise SystemExit('Expected a stable SemVer tag')
          formula = pathlib.Path('Formula/dotnet-crap.rb')
          current = re.search(r'^  version "([0-9]+\.[0-9]+\.[0-9]+)"$', formula.read_text(), re.M) if formula.exists() else None
          # Bootstrap: no formula yet, so the first stable release must be synced.
          changed = 'true' if current is None else \
              str(tuple(map(int, tag[1:].split('.'))) > tuple(map(int, current[1].split('.')))).lower()
          with open(os.environ['GITHUB_OUTPUT'], 'a') as output:
              print(f'tag={tag}', file=output)
              print(f'changed={changed}', file=output)
          PYTHON
      - name: Check for an existing update PR
        id: pending
        if: steps.release.outputs.changed == 'true'
        env:
          GH_TOKEN: ${{ github.token }}
          TAG: ${{ steps.release.outputs.tag }}
        run: |
          existing=$(gh pr list --repo "$GITHUB_REPOSITORY" --head "releases/dotnet-crap-$TAG" --base main --state open --json number --jq length)
          if [[ "$existing" != 0 ]]; then
            echo 'exists=true' >> "$GITHUB_OUTPUT"
            echo 'An update PR for this tag is already open; no duplicate needed.' >> "$GITHUB_STEP_SUMMARY"
          else
            echo 'exists=false' >> "$GITHUB_OUTPUT"
          fi
      - name: Verify formula provenance and source identity
        if: steps.pending.outputs.exists == 'false'
        env:
          GH_TOKEN: ${{ github.token }}
          TAG: ${{ steps.release.outputs.tag }}
        run: |
          gh release download "$TAG" --repo RuiCostafrg/dotnet-crap \
            --pattern dotnet-crap.rb --dir verified
          source_sha=$(gh api "repos/RuiCostafrg/dotnet-crap/commits/$TAG" --jq .sha)
          gh attestation verify verified/dotnet-crap.rb --repo RuiCostafrg/dotnet-crap \
            --signer-workflow RuiCostafrg/dotnet-crap/.github/workflows/release.yml \
            --source-ref "refs/tags/$TAG" --source-digest "$source_sha" --deny-self-hosted-runners
          # Syntax checking happens without write credentials; never eval a formula.
          ruby -c verified/dotnet-crap.rb
          echo "Verified dotnet-crap $TAG at $source_sha" >> "$GITHUB_STEP_SUMMARY"
      - uses: actions/upload-artifact@cf430e030ddbb5b0abf93d22962f4752f3646cd9 # v7.0.2
        if: steps.pending.outputs.exists == 'false'
        with:
          name: verified-dotnet-crap-formula
          path: verified/dotnet-crap.rb
          if-no-files-found: error
          retention-days: 7

  publish-dotnet-crap:
    needs: verify-dotnet-crap
    if: needs.verify-dotnet-crap.outputs.changed == 'true'
    runs-on: ubuntu-26.04
    timeout-minutes: 10
    environment: homebrew
    permissions:
      contents: write
      pull-requests: write
      actions: write
    steps:
      - uses: actions/download-artifact@9000827ccba6bdab643e8b6fd33ac0654aef8333 # v8.0.2
        with:
          name: verified-dotnet-crap-formula
          path: verified
      - name: Open owner-reviewed formula addition or update
        env:
          GH_TOKEN: ${{ github.token }}
          TAG: ${{ needs.verify-dotnet-crap.outputs.tag }}
        run: |
          branch="releases/dotnet-crap-$TAG"
          base=$(gh api "repos/$GITHUB_REPOSITORY/commits/main" --jq .sha)
          # Branch changes only. Protected main still requires owner merge.
          gh api "repos/$GITHUB_REPOSITORY/git/refs" -X POST -f ref="refs/heads/$branch" -f sha="$base" --silent
          # Bootstrap: Formula/dotnet-crap.rb does not exist yet, so the first
          # sync must create the file (contents PUT without sha) instead of
          # updating it (contents PUT with the existing blob's sha).
          sha=$(gh api "repos/$GITHUB_REPOSITORY/contents/Formula/dotnet-crap.rb" --jq .sha 2>/dev/null || true)
          if [[ -n "$sha" ]]; then
            message="Update dotnet-crap to $TAG (verified provenance)"
            title="Update dotnet-crap to $TAG"
          else
            message="Add dotnet-crap $TAG (verified provenance)"
            title="Add dotnet-crap $TAG"
          fi
          gh api "repos/$GITHUB_REPOSITORY/contents/Formula/dotnet-crap.rb" -X PUT \
            -f message="$message" ${sha:+-f sha="$sha"} -f content="$(base64 -w 0 verified/dotnet-crap.rb)" \
            -f branch="$branch" --silent
          cat > "$RUNNER_TEMP/body.md" <<'BODY'
          The read-only sync job verified this dotnet-crap formula against the source release workflow, tag and commit using GitHub artifact attestations. Review the change and passing Tap CI before merging. This PR is created by the owner-approved homebrew environment job using a temporary token.
          BODY
          gh pr create --repo "$GITHUB_REPOSITORY" --base main --head "$branch" \
            --title "$title" --body-file "$RUNNER_TEMP/body.md"
          # Token-originated pushes do not start CI; dispatch the read-only checks.
          gh workflow run ci.yml --repo "$GITHUB_REPOSITORY" --ref "$branch"
```

The tap's `homebrew` environment and its existing `ci.yml` are reused as-is.

## Bootstrap notes

- `Formula/dotnet-crap.rb` **does not exist yet** in the tap. The current
  ts-crap publish step fetches the existing blob sha and PUTs an update; on the
  first dotnet-crap sync that call returns 404, so the proposed step treats an
  absent file as creation (no `-f sha`) and words the commit/PR as "Add".
- Until dotnet-crap publishes its first stable GitHub release,
  `repos/RuiCostafrg/dotnet-crap/releases/latest` returns 404 and the verify
  job fails benignly (the ts-crap jobs are unaffected).
- `gh attestation verify` requires `release.yml` to attest `dotnet-crap.rb`
  (the publication job attests `release-assets/*`, which contains it).
- `brew audit` is intentionally not part of this pipeline: it needs network
  access and runs in the tap's own CI on the sync PR.

## What this repository does not do

It never pushes to the tap, never holds a tap token, and never merges the sync
PR. Enabling the channel is an owner action listed under
[Publication prerequisites](../../docs/DISTRIBUTION.md#publication-prerequisites).
