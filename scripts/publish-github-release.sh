#!/usr/bin/env bash
# Creates the GitHub release for a tag, or reuses one left by an earlier attempt.
# Existing assets are immutable: retries must supply the same bytes, normally
# by reusing the original retained artifacts.
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
# Never overwrite published assets: doing so could make GitHub downloads differ
# from a NuGet package that has already been published for this same version.
work="$(mktemp -d "${TMPDIR:-/tmp}/dotnet-crap-release-verify.XXXXXX")"
trap 'rm -rf "$work"' EXIT
for asset in "$@"; do
  name="$(basename "$asset")"
  gh release download "$tag" --pattern "$name" --dir "$work"
  if ! cmp -s "$asset" "$work/$name"; then
    echo "::error::Published asset $name differs from this build; no published assets were changed. Reuse the original retained artifacts, or use a new version." >&2
    exit 1
  fi
done
echo "::notice::GitHub release $tag already exists with matching assets; preserving published bytes."
