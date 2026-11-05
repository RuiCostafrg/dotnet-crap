#!/usr/bin/env bash
# Creates the GitHub release for a tag, or reuses one left by an earlier attempt.
# Reuse replaces same-named assets with this run's verified assets. A failure is
# reported unless an existing release is readable, in which case the upload error is returned.
set -euo pipefail

tag="${1:?usage: publish-github-release.sh <tag> <asset>...}"
shift
if (($# == 0)); then
  echo "publish-github-release: no assets were given" >&2
  exit 2
fi
for asset in "$@"; do
  if [[ ! -f "$asset" ]]; then
    echo "publish-github-release: missing asset $asset" >&2
    exit 2
  fi
done

# --verify-tag stops gh from creating the tag at a default-branch commit.
if create_output="$(gh release create "$tag" "$@" --verify-tag --generate-notes 2>&1)"; then
  printf '%s\n' "$create_output"
  exit 0
fi
printf '%s\n' "$create_output" >&2

if ! gh release view "$tag" >/dev/null 2>&1; then
  echo "::error::GitHub release $tag could not be created and no existing release is readable." >&2
  exit 1
fi
echo "::notice::GitHub release $tag already exists; replacing its assets with this run's verified assets."
gh release upload "$tag" "$@" --clobber
