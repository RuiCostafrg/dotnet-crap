#!/usr/bin/env bash
# Fails unless the given commit is reachable from origin/main after a fresh fetch.
# Run in a checkout with full history (fetch-depth: 0).
set -euo pipefail

commit="${1:?usage: verify-tag-on-main.sh <commit>}"
git fetch --no-tags origin '+refs/heads/main:refs/remotes/origin/main'
if ! git merge-base --is-ancestor "$commit" refs/remotes/origin/main; then
  echo "::error::Tag commit $commit is not reachable from origin/main; refusing to publish." >&2
  exit 1
fi
echo "Tag commit $commit is reachable from origin/main."
