#!/usr/bin/env bash
# Pushes the one .nupkg in a directory to NuGet.org. --skip-duplicate makes a
# retry succeed when this version is already published; every other push error
# (authentication, validation, server failure) exits non-zero.
set -euo pipefail

dir="${1:?usage: publish-nuget.sh <directory-with-one-nupkg>}"
if [[ -z "${NUGET_API_KEY:-}" ]]; then
  echo '::notice::NUGET_API_KEY is absent in the release environment; skipping NuGet publication.'
  exit 0
fi

shopt -s nullglob
packages=("$dir"/*.nupkg)
if [[ "${#packages[@]}" -ne 1 ]]; then
  echo "::error::Expected exactly one .nupkg in $dir; found ${#packages[@]}." >&2
  exit 1
fi

dotnet nuget push "${packages[0]}" --api-key "$NUGET_API_KEY" \
  --source https://api.nuget.org/v3/index.json --skip-duplicate
