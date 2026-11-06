#!/usr/bin/env bash
# dotnet-crap local CI — Linux only, powered by basecamp/gh-signoff.
# Run the same checks as the Linux GitHub Actions jobs on your own machine.
# Usage:
#   scripts/local-ci.sh               all default stages: tests lint security
#   scripts/local-ci.sh tests         selected stages only
#   scripts/local-ci.sh mutation      slow additional stages: mutation, e2e
#   scripts/local-ci.sh --list        list stages
#   scripts/local-ci.sh --no-signoff  run checks, don't post signoff statuses
#   scripts/local-ci.sh --help        this text
# Docs: docs/local-ci.md
set -euo pipefail

AVAILABLE_STAGES=(tests lint security mutation e2e)
DEFAULT_STAGES=(tests lint security)

_fail()   { printf '✗ %s\n' "$*" >&2; exit 1; }
_log()    { printf '▸ %s\n' "$*"; }
_ok()     { printf '✓ %s\n' "$*"; }
require() { command -v "$1" >/dev/null 2>&1 || _fail "missing '$1'${2:+ — $2}"; }

case "$(uname -s)" in
  Linux) ;;
  *) _fail "Linux-only local CI; refusing to run on $(uname -s) (macOS/Windows stay in GitHub Actions)." ;;
esac

NO_SIGNOFF=0
SELECTED=()
while [ $# -gt 0 ]; do
  case "$1" in
    --no-signoff) NO_SIGNOFF=1 ;;
    --list) printf '%s\n' "stages: ${AVAILABLE_STAGES[*]}"; exit 0 ;;
    -h|--help) sed -n '2,14p' "$0"; exit 0 ;;
    -*) _fail "unknown option: $1 (try --help)" ;;
    *) SELECTED+=("$1") ;;
  esac
  shift
done
[ ${#SELECTED[@]} -gt 0 ] || SELECTED=("${DEFAULT_STAGES[@]}")

if [ "$NO_SIGNOFF" = 0 ]; then
  require gh "GitHub CLI is required for gh-signoff (or pass --no-signoff)"
  gh extension list 2>/dev/null | grep -qi "gh-signoff" \
    || _fail "gh-signoff extension missing — run: gh extension install basecamp/gh-signoff (or pass --no-signoff)"
fi

# ── stages (contexts posted: signoff/<name>) ──────────────────────────────────

stage_tests() {
  require cargo "Rust toolchain (https://rustup.rs)"
  require python3 ""
  python3 -m unittest discover -s tests -p 'test_*.py'
  cargo test --locked
  cargo test --locked --release
}

stage_lint() {
  require cargo ""
  cargo fmt --check
  cargo clippy --locked --all-targets -- -D warnings
}

stage_security() {
  require cargo ""
  if ! command -v cargo-audit >/dev/null 2>&1; then
    _fail "cargo-audit missing — cargo install cargo-audit --version 0.22.2 --locked"
  fi
  cargo audit --deny warnings
}

stage_mutation() {
  # Very slow; not part of the default set.
  require cargo ""
  if ! command -v cargo-mutants >/dev/null 2>&1; then
    _fail "cargo-mutants missing — cargo install cargo-mutants --version 27.1.0 --locked"
  fi
  python3 -m unittest discover -s tests -p 'test_*.py'
  python3 scripts/mutation-gate.py
}

stage_e2e() {
  # .NET launcher/install E2E contracts; needs the .NET 10 SDK.
  require dotnet ".NET 10 SDK (https://dotnet.microsoft.com/download)"
  require cargo ""
  dotnet run --project packaging/tests/DotnetCrapLauncher.Tests -c Release
  bash scripts/test-install-action.sh
  bash scripts/test-release-publish.sh
  bash scripts/smoke-packaging.sh
  DOTNET_CRAP_E2E=1 cargo test --locked
}

# ── runner ────────────────────────────────────────────────────────────────────

# Validate all selected stage names before running anything.
for s in "${SELECTED[@]}"; do
  declare -F "stage_$s" >/dev/null || _fail "unknown stage '$s' (stages: ${AVAILABLE_STAGES[*]})"
done

if [ -n "$(git status --porcelain)" ]; then
  # gh-signoff refuses to attest a dirty tree, so fail fast BEFORE the long
  # stages instead of finishing a full run with nothing posted.
  if [ "$NO_SIGNOFF" = 0 ]; then
    _fail "working tree has uncommitted changes — commit or stash them, or pass --no-signoff"
  fi
  _log "warning: working tree has uncommitted changes — results may not match HEAD"
fi

for s in "${SELECTED[@]}"; do
  _log "stage: $s"
  # Run the stage in a subshell with errexit, OUTSIDE any if-condition context:
  # inside `if ! (...)` bash suppresses errexit, so a failing command mid-stage
  # could be masked and the stage wrongly reported green (bot review P0).
  set +e
  ( set -e; "stage_$s" )
  rc=$?
  set -e
  if [ "$rc" -ne 0 ]; then
    [ "$NO_SIGNOFF" = 0 ] && gh signoff fail "$s" --description "$s failed locally" >/dev/null 2>&1 || true
    _fail "stage '$s' failed"
  fi
  _ok "$s passed"
  if [ "$NO_SIGNOFF" = 0 ]; then
    gh signoff "$s" >/dev/null 2>&1 \
      || printf '⚠ could not post signoff status for %s — is HEAD pushed? gh authorized?\n' "$s" >&2
  fi
done

if [ "$NO_SIGNOFF" = 1 ]; then
  _log "all done — no signoff statuses posted (was --no-signoff)."
else
  _log "all done."
fi
