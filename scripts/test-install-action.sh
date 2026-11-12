#!/usr/bin/env bash
# Offline tests of action/install.sh and action/scan.sh. A fake `gh` on PATH
# serves release files from a local directory and records its argv, so the
# download, provenance, checksum and version checks run without network access
# and without real attestations. The archives hold a shell script standing in
# for dotnet-crap, so the tests run on any Linux or macOS host.
#
# Written for bash 3.2 (macOS /bin/bash) as well as newer versions.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
work="$(mktemp -d "${TMPDIR:-/tmp}/dotnet-crap-install-test.XXXXXX")"
trap 'rm -rf "$work"' EXIT

version=9.9.9
identity_base=https://github.com/RuiCostafrg/dotnet-crap/.github/workflows/release.yml
mkdir -p "$work/bin" "$work/runner-temp" "$work/releases"

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum < "$1" | cut -d ' ' -f 1
  else
    shasum -a 256 < "$1" | cut -d ' ' -f 1
  fi
}

# Fake gh: `release download` copies the exact --pattern names from
# $GH_RELEASES/<tag>; `attestation verify` fails for names in $GH_UNATTESTED.
# Every call's arguments are appended to $GH_CALLS, one argument per line,
# with a "--" line after each call.
cat > "$work/bin/gh" <<'FAKE_GH'
#!/usr/bin/env bash
set -euo pipefail
{ printf '%s\n' "$@"; echo --; } >> "$GH_CALLS"
case "${1:-} ${2:-}" in
  "release download")
    tag=$3
    patterns=() dir=
    shift 3
    while (($#)); do
      case "$1" in
        --pattern) patterns+=("$2"); shift 2 ;;
        --dir) dir=$2; shift 2 ;;
        *) shift ;;
      esac
    done
    [[ -d "$GH_RELEASES/$tag" ]] || { echo "release not found: $tag" >&2; exit 1; }
    for name in "${patterns[@]}"; do
      if [[ -f "$GH_RELEASES/$tag/$name" ]]; then cp "$GH_RELEASES/$tag/$name" "$dir/$name"; fi
    done
    ;;
  "attestation verify")
    name=$(basename "$3")
    for unattested in ${GH_UNATTESTED:-}; do
      if [[ "$unattested" == "$name" ]]; then echo "no matching attestation" >&2; exit 1; fi
    done
    ;;
  *) echo "unexpected gh call: $*" >&2; exit 2 ;;
esac
FAKE_GH
chmod 755 "$work/bin/gh"

# Publishes release v$1 for target $2 with a fake dotnet-crap that reports
# "dotnet-crap $REPORTED". Optional variables: REPORTED, CHECKSUM_LINE (literal
# checksum file content; %DIGEST%, %DIGEST_UPPER% and %NAME% are substituted), NO_CHECKSUM,
# EXTRA_BINARY, NO_BINARY.
publish() {
  local tag="v$1" target=$2 name stage digest line
  name="dotnet-crap-$target.tar.xz"
  stage="$work/stage-$tag-$target"
  rm -rf "$stage"
  mkdir -p "$stage/dotnet-crap-$target" "$work/releases/$tag"
  if [[ -z "${NO_BINARY:-}" ]]; then
    cat > "$stage/dotnet-crap-$target/dotnet-crap" <<EOF
#!/bin/sh
if [ "\$1" = --version ]; then
  printf '%s' "\${GH_TOKEN:-}\${GITHUB_TOKEN:-}" > "\$TOKEN_LOG"
  echo "dotnet-crap ${REPORTED:-$1}"
  exit 0
fi
exit 0
EOF
    chmod 755 "$stage/dotnet-crap-$target/dotnet-crap"
  fi
  if [[ -n "${EXTRA_BINARY:-}" ]]; then
    mkdir -p "$stage/dotnet-crap-$target/other"
    cp "$stage/dotnet-crap-$target/dotnet-crap" "$stage/dotnet-crap-$target/other/dotnet-crap"
  fi
  printf 'MIT\n' > "$stage/dotnet-crap-$target/LICENSE"
  tar -C "$stage" -cJf "$work/releases/$tag/$name" "dotnet-crap-$target"
  rm -f "$work/releases/$tag/$name.sha256"
  if [[ -z "${NO_CHECKSUM:-}" ]]; then
    digest=$(sha256_of "$work/releases/$tag/$name")
    line=${CHECKSUM_LINE:-'%DIGEST%  %NAME%'}
    line=${line//%DIGEST_UPPER%/$(printf '%s' "$digest" | tr 'a-f' 'A-F')}
    line=${line//%DIGEST%/$digest}
    line=${line//%NAME%/$name}
    printf '%s\n' "$line" > "$work/releases/$tag/$name.sha256"
  fi
}

# Runs action/install.sh with a clean environment. Extra KEY=VALUE arguments
# override the defaults (a Linux x86-64 runner, version $version, no extras).
run_install() {
  rm -rf "$work/runner-temp" "$work/token"
  mkdir -p "$work/runner-temp"
  : > "$work/github_path"
  : > "$work/github_output"
  : > "$work/gh_calls"
  env -i \
    PATH="$work/bin:$PATH" \
    HOME="$work" \
    TMPDIR="$work" \
    RUNNER_OS=Linux \
    RUNNER_ARCH=X64 \
    RUNNER_TEMP="$work/runner-temp" \
    GITHUB_PATH="$work/github_path" \
    GITHUB_OUTPUT="$work/github_output" \
    GH_TOKEN=secret-token \
    GH_CALLS="$work/gh_calls" \
    GH_RELEASES="$work/releases" \
    TOKEN_LOG="$work/token" \
    DOTNET_CRAP_ACTION_VERSION="$version" \
    "$@" \
    bash "$root/action/install.sh" > "$work/installer.out" 2>&1
}

expect_ok() {
  local description=$1
  shift
  run_install "$@" || { cat "$work/installer.out" >&2; fail "$description: installer failed"; }
}

# Expects failure with $1 in the output, and nothing staged on PATH or output.
expect_failure() {
  local description=$1 message=$2
  shift 2
  if run_install "$@"; then
    cat "$work/installer.out" >&2
    fail "$description: installer succeeded"
  fi
  grep -qF -- "$message" "$work/installer.out" \
    || { cat "$work/installer.out" >&2; fail "$description: expected message '$message'"; }
  [[ ! -s "$work/github_path" ]] || fail "$description: a failed install changed PATH"
  [[ ! -s "$work/github_output" ]] || fail "$description: a failed install wrote outputs"
}

gh_call_count() {
  grep -c '^--$' "$work/gh_calls" || true
}

# --- 1. Happy path: exact gh arguments, outputs, PATH, no token leak ----------
publish "$version" x86_64-unknown-linux-musl
expect_ok "happy path"
download=$(awk 'prev == "--dir" { print; exit } { prev = $0 }' "$work/gh_calls")
[[ "$download" == "$work/runner-temp/dotnet-crap-action."*/download ]] || fail "unexpected download dir: $download"
archive=dotnet-crap-x86_64-unknown-linux-musl.tar.xz
expected_calls=$(cat <<EOF
release
download
v$version
--repo
RuiCostafrg/dotnet-crap
--pattern
$archive
--pattern
$archive.sha256
--dir
$download
--
attestation
verify
$download/$archive
--repo
RuiCostafrg/dotnet-crap
--cert-identity
$identity_base@refs/tags/v$version
--source-ref
refs/tags/v$version
--deny-self-hosted-runners
--
EOF
)
[[ "$(cat "$work/gh_calls")" == "$expected_calls" ]] \
  || { diff <(echo "$expected_calls") "$work/gh_calls" >&2 || true; fail "gh was not called with the exact expected arguments"; }
bin_dir=$(sed -n 's/^bin-dir=//p' "$work/github_output")
executable=$(sed -n 's/^executable=//p' "$work/github_output")
[[ "$(sed -n 's/^version=//p' "$work/github_output")" == "$version" ]] || fail "version output"
[[ "$(sed -n 's/^target=//p' "$work/github_output")" == x86_64-unknown-linux-musl ]] || fail "target output"
[[ "$executable" == "$bin_dir/dotnet-crap" && -x "$executable" ]] || fail "executable output: $executable"
[[ "$bin_dir" == "$work/runner-temp/dotnet-crap-action."*/tool/* ]] || fail "bin-dir output: $bin_dir"
[[ "$(cat "$work/github_path")" == "$bin_dir" ]] || fail "GITHUB_PATH does not name the bin dir"
[[ "$(wc -l < "$work/github_output" | tr -d ' ')" == 4 ]] || fail "unexpected number of outputs"
[[ ! -s "$work/token" ]] || fail "the downloaded binary saw the GitHub token"
echo "PASS happy path: exact gh arguments, outputs and PATH, token withheld from the binary"

# --- 2. A leading v is accepted; prereleases are accepted ----------------------
expect_ok "v prefix" DOTNET_CRAP_ACTION_VERSION="v$version"
[[ "$(sed -n 's/^version=//p' "$work/github_output")" == "$version" ]] || fail "v prefix was not stripped"
grep -qx "v$version" "$work/gh_calls" || fail "release tag must be v$version"
publish 1.0.0-rc.1 x86_64-unknown-linux-musl
expect_ok "prerelease" DOTNET_CRAP_ACTION_VERSION=1.0.0-rc.1
echo "PASS versions: v prefix and prerelease accepted"

# --- 3. Platform selection and unsupported runners -----------------------------
for pair in Linux/ARM64:aarch64-unknown-linux-musl macOS/X64:x86_64-apple-darwin macOS/ARM64:aarch64-apple-darwin; do
  platform=${pair%%:*}
  target=${pair#*:}
  publish "$version" "$target"
  expect_ok "$platform" RUNNER_OS="${platform%/*}" RUNNER_ARCH="${platform#*/}"
  [[ "$(sed -n 's/^target=//p' "$work/github_output")" == "$target" ]] || fail "$platform selected the wrong target"
  grep -qx "dotnet-crap-$target.tar.xz" "$work/gh_calls" || fail "$platform downloaded the wrong archive"
done
for platform in Windows/X64 Linux/ARM Linux/X86 macOS/X86 FreeBSD/X64; do
  expect_failure "$platform" "supports Linux and macOS runners" RUNNER_OS="${platform%/*}" RUNNER_ARCH="${platform#*/}"
  [[ "$(gh_call_count)" == 0 ]] || fail "$platform still called gh"
done
echo "PASS platforms: Linux/macOS x64/arm64 select static musl or Apple archives; Windows and others fail clearly"

# --- 4. Invalid versions are rejected before any download ----------------------
publish "$version" x86_64-unknown-linux-musl
expect_failure "latest" "'latest' is not accepted" DOTNET_CRAP_ACTION_VERSION=latest
for requested in latest 1.2 1.2.3.4 vv1.2.3 '' ' 1.2.3' '1.2.3 ' 'v' '1.2.x' '../1.2.3'; do
  expect_failure "version '$requested'" "version must be a published release" DOTNET_CRAP_ACTION_VERSION="$requested"
  [[ "$(gh_call_count)" == 0 ]] || fail "version '$requested' still called gh"
done
echo "PASS versions: latest, partial, padded and malformed versions rejected before gh"

# --- 5. Checksum verification --------------------------------------------------
CHECKSUM_LINE="%DIGEST_UPPER% *%NAME%" publish "$version" x86_64-unknown-linux-musl
expect_ok "uppercase digest with binary-mode marker"
CHECKSUM_LINE="0000000000000000000000000000000000000000000000000000000000000000  %NAME%" \
  publish "$version" x86_64-unknown-linux-musl
expect_failure "checksum mismatch" "SHA-256 mismatch"
CHECKSUM_LINE="not-a-checksum" publish "$version" x86_64-unknown-linux-musl
expect_failure "malformed checksum" "malformed checksum file"
CHECKSUM_LINE="%DIGEST%" publish "$version" x86_64-unknown-linux-musl
expect_failure "digest without a name" "malformed checksum file"
CHECKSUM_LINE="%DIGEST%  dotnet-crap-aarch64-unknown-linux-musl.tar.xz" publish "$version" x86_64-unknown-linux-musl
expect_failure "checksum naming another file" "checksum file names 'dotnet-crap-aarch64-unknown-linux-musl.tar.xz'"
CHECKSUM_LINE="%DIGEST%  ../%NAME%" publish "$version" x86_64-unknown-linux-musl
expect_failure "checksum naming a path" "checksum file names"
NO_CHECKSUM=1 publish "$version" x86_64-unknown-linux-musl
expect_failure "missing checksum file" "missing checksum file"
echo "PASS checksums: mismatch, malformed, misbound and missing checksum files rejected"

# --- 6. Provenance, missing assets and releases --------------------------------
publish "$version" x86_64-unknown-linux-musl
expect_failure "attestation failure" "build provenance verification failed" \
  GH_UNATTESTED=dotnet-crap-x86_64-unknown-linux-musl.tar.xz
[[ "$(gh_call_count)" == 2 ]] || fail "attestation failure should follow the download"
expect_failure "unpublished release" "could not download v9.9.8" DOTNET_CRAP_ACTION_VERSION=9.9.8
rm "$work/releases/v$version/dotnet-crap-x86_64-unknown-linux-musl.tar.xz"
expect_failure "missing archive" "has no dotnet-crap-x86_64-unknown-linux-musl.tar.xz"
echo "PASS provenance: attestation failure, missing release and missing archive rejected"

# --- 7. Executable and --version checks ----------------------------------------
REPORTED=9.9.8 publish "$version" x86_64-unknown-linux-musl
expect_failure "wrong --version" "reports 'dotnet-crap 9.9.8', expected 'dotnet-crap $version'"
NO_BINARY=1 publish "$version" x86_64-unknown-linux-musl
expect_failure "no executable" "expected exactly one dotnet-crap"
EXTRA_BINARY=1 publish "$version" x86_64-unknown-linux-musl
expect_failure "two executables" "expected exactly one dotnet-crap"
echo "PASS binary: wrong --version output, missing and ambiguous executables rejected"

# --- 8. Input validation before installing -------------------------------------
publish "$version" x86_64-unknown-linux-musl
expect_failure "sarif without scan" "sarif requires scan: true" DOTNET_CRAP_ACTION_SARIF=true
expect_failure "upload without sarif" "upload-sarif requires sarif: true" \
  DOTNET_CRAP_ACTION_SCAN=true DOTNET_CRAP_ACTION_UPLOAD=true
expect_failure "upload without scan or sarif" "upload-sarif requires sarif: true" DOTNET_CRAP_ACTION_UPLOAD=true
expect_failure "non-boolean scan" "scan must be true or false" DOTNET_CRAP_ACTION_SCAN=yes
expect_failure "empty sarif" "sarif must be true or false" DOTNET_CRAP_ACTION_SARIF=
[[ "$(gh_call_count)" == 0 ]] || fail "invalid inputs still called gh"
expect_ok "scan with sarif and upload" \
  DOTNET_CRAP_ACTION_SCAN=true DOTNET_CRAP_ACTION_SARIF=true DOTNET_CRAP_ACTION_UPLOAD=true
echo "PASS inputs: sarif needs scan, upload needs sarif, booleans are strict"

# --- 9. scan.sh: arguments, SARIF ordering, exit codes -------------------------
cat > "$work/fake-crap" <<'FAKE_CRAP'
#!/bin/sh
for argument in "$@"; do printf '%s\n' "$argument" >> "$SCAN_CALLS"; done
echo --- >> "$SCAN_CALLS"
while [ $# -gt 0 ]; do
  if [ "$1" = --output ]; then
    if [ -n "${FAKE_REPORT_FILE:-}" ]; then
      cat "$FAKE_REPORT_FILE" > "$2"
    else
      printf '%s' '{"version":"2.1.0","runs":[{"results":[]}]}' > "$2"
    fi
    exit "${FAKE_STATUS:-0}"
  fi
  shift
done
exit "${FAKE_STATUS:-0}"
FAKE_CRAP
chmod 755 "$work/fake-crap"

run_scan() {
  : > "$work/github_output"
  : > "$work/scan_calls"
  mkdir -p "$work/runner-temp" "$work/workspace"
  (
    cd "$work/workspace"
    env -i \
      PATH="$PATH" \
      HOME="$work" \
      RUNNER_TEMP="$work/runner-temp" \
      GITHUB_OUTPUT="$work/github_output" \
      GITHUB_WORKSPACE="$work/workspace" \
      SCAN_CALLS="$work/scan_calls" \
      DOTNET_CRAP_ACTION_EXECUTABLE="$work/fake-crap" \
      "$@" \
      bash "$root/action/scan.sh" > "$work/scan.out" 2>&1
  )
}

args_with_traps=$'--root\nsrc with space\n\n*.cs\n$(touch pwned)\n--threshold\n7'
run_scan DOTNET_CRAP_ACTION_ARGS="$args_with_traps" || { cat "$work/scan.out" >&2; fail "plain scan failed"; }
expected=$'--root\nsrc with space\n*.cs\n$(touch pwned)\n--threshold\n7\n---'
[[ "$(cat "$work/scan_calls")" == "$expected" ]] || fail "scan arguments were not passed one per line, unevaluated"
[[ ! -e "$work/workspace/pwned" ]] || fail "scan arguments were evaluated by a shell"
[[ "$(cat "$work/github_output")" == "exit-code=0" ]] || fail "plain scan outputs"

run_scan DOTNET_CRAP_ACTION_SARIF=true DOTNET_CRAP_ACTION_ARGS=$'--threshold\n5\nsrc' FAKE_STATUS=1 \
  || fail "SARIF gate failure should still exit 0 from scan.sh"
report=$(sed -n 's/^sarif-file=//p' "$work/github_output")
[[ "$(sed -n 's/^exit-code=//p' "$work/github_output")" == 1 ]] || fail "gate failure exit-code output"
[[ -f "$report" ]] || fail "gate failure lost the SARIF report"
[[ "$(sed -n '1p;2p;3p;4p' "$work/scan_calls" | tr '\n' ' ')" == "--format sarif --output $report " ]] \
  || fail "SARIF options must precede user arguments"
grep -qF "0 SARIF result(s)" "$work/scan.out" || fail "SARIF summary missing"

mkdir -p "$work/workspace/src with space"
printf '%s' '{"version":"2.1.0","runs":[{"results":[{"locations":[{"physicalLocation":{"artifactLocation":{"uri":"Gauge.cs"}}}]}]}]}' > "$work/report.json"
run_scan DOTNET_CRAP_ACTION_SARIF=true DOTNET_CRAP_ACTION_ARGS=$'--root\nsrc with space' \
  FAKE_REPORT_FILE="$work/report.json" || fail "SARIF scan root with spaces failed"
report=$(sed -n 's/^sarif-file=//p' "$work/github_output")
python3 - "$report" <<'PY'
import json, sys
report = json.load(open(sys.argv[1], encoding='utf-8'))
uri = report['runs'][0]['results'][0]['locations'][0]['physicalLocation']['artifactLocation']['uri']
assert uri == 'src%20with%20space/Gauge.cs', uri
PY

run_scan DOTNET_CRAP_ACTION_SARIF=true DOTNET_CRAP_ACTION_ARGS=$'--format\njson' \
  && fail "--format with sarif was accepted" || true
[[ "$(cat "$work/github_output")" == "exit-code=2" ]] || fail "invalid scan input must publish exit-code=2"
run_scan DOTNET_CRAP_ACTION_SARIF=true DOTNET_CRAP_ACTION_ARGS=$'--output=x' \
  && fail "--output with sarif was accepted" || true
run_scan FAKE_STATUS=64 || fail "unexpected statuses should be recorded"
[[ "$(cat "$work/github_output")" == "exit-code=2" ]] || fail "statuses above 1 must map to 2"
echo "PASS scan: one argument per line, no shell evaluation, SARIF ordering, exit codes"

echo "PASS install-action: all offline action tests passed"
