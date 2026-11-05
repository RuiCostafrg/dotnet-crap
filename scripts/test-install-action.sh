#!/usr/bin/env bash
# Offline checks for scripts/install-action.sh. A fake curl serves local release
# assets and records every request, so the timeout flags, checksum rejection, and
# version validation are exercised without network access.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
work="$(mktemp -d "${TMPDIR:-/tmp}/dotnet-crap-install-test.XXXXXX")"
trap 'rm -rf "$work"' EXIT

version=9.9.9
case "$(uname -s)/$(uname -m)" in
  Linux/x86_64) runner_os=Linux runner_arch=X64 rid=linux-x64 ;;
  Linux/aarch64 | Linux/arm64) runner_os=Linux runner_arch=ARM64 rid=linux-arm64 ;;
  Darwin/x86_64) runner_os=macOS runner_arch=X64 rid=osx-x64 ;;
  Darwin/arm64) runner_os=macOS runner_arch=ARM64 rid=osx-arm64 ;;
  *) echo "Unsupported host for install-action smoke test" >&2; exit 2 ;;
esac

asset="dotnet-crap-v${version}-${rid}.tar.gz"
payload="$work/payload"
mkdir -p "$payload" "$work/bin" "$work/runner-temp"
printf '#!/bin/sh\necho "dotnet-crap %s from install-action smoke"\n' "$version" > "$payload/dotnet-crap"
chmod 755 "$payload/dotnet-crap"

sha256_of() {
  if command -v sha256sum >/dev/null; then sha256sum "$1"; else shasum -a 256 "$1"; fi
}

serve="$work/serve"
mkdir -p "$serve"
tar -C "$payload" -czf "$serve/$asset" dotnet-crap
(cd "$serve" && sha256_of "$asset" > "$asset.sha256")

bad_serve="$work/serve-bad"
mkdir -p "$bad_serve"
cp "$serve/$asset" "$bad_serve/$asset"
printf '%064d  %s\n' 0 "$asset" > "$bad_serve/$asset.sha256"

# Records each request's arguments, then copies the requested asset from FAKE_RELEASE_DIR.
cat > "$work/bin/curl" <<'FAKE_CURL'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >> "$CURL_LOG"
out=""
url=""
while (($#)); do
  case "$1" in
    -o) out="$2"; shift 2 ;;
    https://*) url="$1"; shift ;;
    *) shift ;;
  esac
done
[[ -n "$url" && -n "$out" ]] || { echo "fake curl: missing URL or -o" >&2; exit 2; }
cp "$FAKE_RELEASE_DIR/${url##*/}" "$out"
FAKE_CURL
chmod 755 "$work/bin/curl"

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

run_installer() {
  # $1 = release directory, $2 = version; prints nothing, returns the installer's status.
  local release_dir="$1" requested="$2"
  : > "$work/curl.log"
  rm -f "$work/github_path"
  env -i \
    PATH="$work/bin:/usr/bin:/bin" \
    HOME="$work" \
    TMPDIR="$work" \
    RUNNER_OS="$runner_os" \
    RUNNER_ARCH="$runner_arch" \
    RUNNER_TEMP="$work/runner-temp" \
    GITHUB_PATH="$work/github_path" \
    DOTNET_CRAP_VERSION="$requested" \
    CURL_LOG="$work/curl.log" \
    FAKE_RELEASE_DIR="$release_dir" \
    bash "$root/scripts/install-action.sh" >"$work/installer.out" 2>&1
}

# 1. Successful install: both requests are bounded, and the verified binary is staged on PATH.
run_installer "$serve" "$version" || { cat "$work/installer.out" >&2; fail "valid release did not install"; }
requests="$(wc -l < "$work/curl.log" | tr -d ' ')"
[[ "$requests" == 2 ]] || fail "expected 2 curl requests, saw $requests"
tls_safe() {
  local line=" $1 "
  [[ "$line" != *" --insecure "* && "$line" != *" -k "* ]]
}
for unsafe in '-k other' 'other -k' '-k' '--insecure other' 'other --insecure'; do
  if tls_safe "$unsafe"; then fail "TLS check accepted $unsafe"; fi
done
tls_safe '--connect-timeout 20' || fail "TLS check rejected safe arguments"
while IFS= read -r line; do
  [[ "$line" == *"--connect-timeout 20"* ]] || fail "request lacks connect timeout: $line"
  [[ " $line " == *" --max-time 240 "* ]] || fail "request lacks attempt timeout: $line"
  [[ " $line " == *" --retry-max-time 60 "* ]] || fail "request lacks retry window: $line"
  [[ " $line " == *" --retry 3 "* ]] || fail "request lacks bounded retry count: $line"
  [[ "$line" == *"--proto =https"* && "$line" == *"--proto-redir =https"* ]] || fail "request is not HTTPS-only: $line"
  tls_safe "$line" || fail "request disables TLS verification: $line"
done < "$work/curl.log"
staged="$(cat "$work/github_path")"
[[ -x "$staged/dotnet-crap" ]] || fail "verified binary was not staged with an execute bit"
"$staged/dotnet-crap" | grep -q "dotnet-crap ${version}" || fail "staged binary did not run"

# 2. Checksum mismatch must fail and must not stage anything on PATH.
if run_installer "$bad_serve" "$version"; then
  fail "checksum mismatch was accepted"
fi
[[ ! -e "$work/github_path" ]] || fail "checksum mismatch staged a binary on PATH"

# 3. Invalid versions are rejected before any download.
if run_installer "$serve" "9.9"; then
  fail "non-exact version was accepted"
fi
[[ ! -s "$work/curl.log" ]] || fail "invalid version still issued a download"

echo "PASS install-action: bounded HTTPS requests, checksum enforcement, and version validation"
