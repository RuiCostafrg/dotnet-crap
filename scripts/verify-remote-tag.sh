#!/usr/bin/env bash
# Resolve the remote tag (including annotated tags) and compare it with the event commit.
set -euo pipefail
tag="${1:?usage: verify-remote-tag.sh <tag> <commit>}"
expected="${2:?usage: verify-remote-tag.sh <tag> <commit>}"
git check-ref-format "refs/tags/$tag" >/dev/null
refs="$(git ls-remote origin "refs/tags/$tag" "refs/tags/$tag^{}")"
resolved="$(printf '%s\n' "$refs" | awk -v ref="refs/tags/$tag" '$2 == ref { direct=$1 } $2 == ref "^{}" { peeled=$1 } END { if (peeled != "") print peeled; else print direct }')"
if [[ -z "$resolved" || "$resolved" != "$expected" ]]; then
  echo "::error::Remote tag $tag is missing or does not resolve to event commit $expected (resolved: ${resolved:-missing})." >&2
  exit 1
fi
