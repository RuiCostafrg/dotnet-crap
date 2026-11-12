#!/usr/bin/env bash
# Installs a published dotnet-crap release for the GitHub Action (see action.yml).
# Downloaded release files are data: nothing downloaded runs before its build
# provenance and SHA-256 checksum verify, and no remote script is executed.
set -euo pipefail

# The release source is fixed. It is deliberately not an input, so a workflow
# cannot redirect the action to another repository's releases or signer.
readonly REPOSITORY=RuiCostafrg/dotnet-crap
# Exact signing-certificate identity: release.yml at the version tag. An exact
# --cert-identity match is used instead of --signer-workflow, which gh 2.101.0
# and older match only as a prefix (GHSA-wjmr-j3rp-mh2g), and it is
# case-sensitive, unlike their --source-ref (GHSA-4mq3-hpgx-9cx8). It works on
# older gh too, such as 2.98.0 on the macos-26 Intel image.
readonly SIGNER_URL="https://github.com/$REPOSITORY/.github/workflows/release.yml"

fail() {
  echo "::error title=dotnet-crap setup::$*" >&2
  exit 1
}

version=${DOTNET_CRAP_ACTION_VERSION:-}
version=${version#v}
if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]]; then
  fail "version must be a published release such as 0.1.0 (got '${DOTNET_CRAP_ACTION_VERSION:-}'); 'latest' is not accepted"
fi

for pair in scan:SCAN sarif:SARIF upload-sarif:UPLOAD; do
  variable=DOTNET_CRAP_ACTION_${pair#*:}
  # Unset means the default; an explicitly empty input is invalid.
  value=${!variable-false}
  [[ "$value" == true || "$value" == false ]] || fail "${pair%%:*} must be true or false"
done
# Fail before installing rather than silently skipping a requested report.
if [[ "${DOTNET_CRAP_ACTION_UPLOAD:-false}" == true && "${DOTNET_CRAP_ACTION_SARIF:-false}" != true ]]; then
  fail "upload-sarif requires sarif: true"
fi
if [[ "${DOTNET_CRAP_ACTION_SARIF:-false}" == true && "${DOTNET_CRAP_ACTION_SCAN:-false}" != true ]]; then
  fail "sarif requires scan: true"
fi

case "${RUNNER_OS:-}/${RUNNER_ARCH:-}" in
  Linux/X64) target=x86_64-unknown-linux-musl ;;
  Linux/ARM64) target=aarch64-unknown-linux-musl ;;
  macOS/X64) target=x86_64-apple-darwin ;;
  macOS/ARM64) target=aarch64-apple-darwin ;;
  *) fail "the action supports Linux and macOS runners, not ${RUNNER_OS:-unknown}/${RUNNER_ARCH:-unknown}" ;;
esac
archive="dotnet-crap-$target.tar.xz"
binary=dotnet-crap

# Hash from stdin so the digest never carries a path (or a backslash prefix).
sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum < "$1" | cut -d ' ' -f 1
  else
    shasum -a 256 < "$1" | cut -d ' ' -f 1
  fi
}

# Accept only "<64 hex> [*]<expected name>" so a checksum file cannot vouch for
# a different file than the one being installed.
verify_checksum() {
  local file=$1 line expected name actual
  [[ -f "$file.sha256" ]] || fail "missing checksum file for $(basename "$file")"
  line=$(tr -d '\r' < "$file.sha256" | sed '/^$/d')
  [[ "$line" =~ ^([0-9a-fA-F]{64})\ \ ?\*?(.+)$ ]] || fail "malformed checksum file for $(basename "$file")"
  expected=$(printf '%s' "${BASH_REMATCH[1]}" | tr 'A-F' 'a-f')
  name=${BASH_REMATCH[2]}
  [[ "$name" == "$(basename "$file")" ]] || fail "checksum file names '$name', not $(basename "$file")"
  actual=$(sha256 "$file")
  [[ "$actual" == "$expected" ]] || fail "SHA-256 mismatch for $(basename "$file")"
}

command -v gh >/dev/null 2>&1 \
  || fail "the GitHub CLI (gh) is required; it is preinstalled on GitHub-hosted runners"
work=$(mktemp -d "${RUNNER_TEMP:?RUNNER_TEMP is not set}/dotnet-crap-action.XXXXXX")
download="$work/download"
mkdir "$download" "$work/tool"
gh release download "v$version" --repo "$REPOSITORY" \
  --pattern "$archive" --pattern "$archive.sha256" --dir "$download" \
  || fail "could not download v$version assets for $target from $REPOSITORY"

[[ -f "$download/$archive" ]] || fail "release v$version has no $archive"
gh attestation verify "$download/$archive" --repo "$REPOSITORY" \
  --cert-identity "$SIGNER_URL@refs/tags/v$version" --source-ref "refs/tags/v$version" \
  --deny-self-hosted-runners >/dev/null \
  || fail "build provenance verification failed for $archive"
verify_checksum "$download/$archive"

tar -xf "$download/$archive" -C "$work/tool"
find "$work/tool" -type f -name "$binary" > "$work/executables"
count=$(wc -l < "$work/executables" | tr -d ' ')
[[ "$count" == 1 ]] || fail "expected exactly one $binary in $archive, found $count"
executable=$(cat "$work/executables")
[[ -x "$executable" ]] || chmod +x "$executable"
# The token is only for gh; the downloaded binary never sees it.
reported=$(unset GH_TOKEN GITHUB_TOKEN; "$executable" --version | tr -d '\r')
[[ "$reported" == "dotnet-crap $version" ]] \
  || fail "installed binary reports '$reported', expected 'dotnet-crap $version'"

bin_dir=$(dirname "$executable")
echo "$bin_dir" >> "$GITHUB_PATH"
{
  echo "version=$version"
  echo "target=$target"
  echo "bin-dir=$bin_dir"
  echo "executable=$executable"
} >> "$GITHUB_OUTPUT"
echo "Installed $reported ($target) from attested $REPOSITORY release v$version"
