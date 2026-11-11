# Local CI — `gh-signoff` (Linux only)

Run the Linux CI checks on your own machine, then post a green `signoff/<stage>`
commit status with [basecamp/gh-signoff](https://github.com/basecamp/gh-signoff).

Use it when free Actions minutes run out — or as a fast pre-push gate. The GitHub
Actions workflows stay untouched; this lane covers **Linux only** (the macOS and
Windows matrix jobs remain hosted).

## Setup (Linux)

```bash
gh auth login                          # if not logged in yet
gh extension install basecamp/gh-signoff
```

Toolchains: Rust (rustup — `.cargo/config.toml`/lockfile is honored), Python 3, and
optionally the .NET 10 SDK for the `e2e` stage and cargo-audit for the `security` stage.

## Usage

```bash
scripts/local-ci.sh                # stages: tests lint security
scripts/local-ci.sh tests lint     # run selected stages only
scripts/local-ci.sh --list         # all stages (incl. optional: mutation e2e)
scripts/local-ci.sh --no-signoff   # checks only, no statuses posted
```

Push your commits first: `gh signoff` only attests pushed work. When everything is
green you get `signoff/tests`, `signoff/lint`, `signoff/security` statuses on HEAD;
a failing stage posts a red status automatically (`gh signoff fail`).

## Optional: make sign-off a merge requirement

```bash
gh signoff install tests lint security   # ruleset on the default branch
gh signoff uninstall                     # ...undo it
```

Admins bypass by default; see the [gh-signoff README](https://github.com/basecamp/gh-signoff#to-require-signoff-for-pr-merges).

## Notes

- The `mutation` stage is deliberately not in the default set: it can run tens of minutes.
- `release.yml` (tag-driven builds for all five targets) remains CI-only — the local
  script mirrors the checks, not release artifact production.
