#!/usr/bin/env bash
set -euo pipefail
version="${DOTNET_CRAP_VERSION:?An exact version is required}"
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]] || { echo 'Invalid release version' >&2; exit 2; }
case "${RUNNER_OS:?}/${RUNNER_ARCH:?}" in
  Linux/X64) rid=linux-x64 ;;
  Linux/ARM64) rid=linux-arm64 ;;
  macOS/X64) rid=osx-x64 ;;
  macOS/ARM64) rid=osx-arm64 ;;
  Windows/X64) rid=win-x64 ;;
  *) echo 'Unsupported runner OS/architecture' >&2; exit 2 ;;
esac
stage="$(mktemp -d "$RUNNER_TEMP/dotnet-crap.XXXXXX")"
asset="dotnet-crap-v${version}-${rid}.tar.gz"
url="https://github.com/RuiCostafrg/dotnet-crap/releases/download/v${version}"
# Both requests allow at most three retries, started within 60 seconds of the
# first attempt. Each attempt has a 240-second limit: even a last retry started
# just before the retry window closes has at most 300 seconds in total.
# TLS and checksum verification stay enabled, including HTTPS-only redirects.
curl_policy=(--fail --silent --show-error --location --proto '=https' --proto-redir '=https'
  --retry 3 --retry-max-time 60 --connect-timeout 20 --max-time 240)
curl "${curl_policy[@]}" "$url/$asset" -o "$stage/$asset"
curl "${curl_policy[@]}" "$url/$asset.sha256" -o "$stage/$asset.sha256"
(
  cd "$stage"
  if command -v sha256sum >/dev/null; then sha256sum -c "$asset.sha256";
  else shasum -a 256 -c "$asset.sha256"; fi
  tar -xzf "$asset"
)
chmod +x "$stage/dotnet-crap" 2>/dev/null || test -f "$stage/dotnet-crap.exe"
echo "$stage" >> "$GITHUB_PATH"
