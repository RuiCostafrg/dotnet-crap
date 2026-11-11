#!/usr/bin/env bash
# Optional scan step for the GitHub Action (see action.yml). Arguments come
# from the newline-separated `args` input, one argument per line, and are
# never evaluated by a shell. The gate's exit code is recorded, not raised, so
# the SARIF upload can still run; action.yml re-raises it afterwards.
set -euo pipefail

fail() {
  echo "::error title=dotnet-crap scan::$*" >&2
  # Invalid input still publishes the documented exit-code output.
  if [[ -n "${GITHUB_OUTPUT:-}" ]]; then echo "exit-code=2" >> "$GITHUB_OUTPUT"; fi
  exit 2
}

executable=${DOTNET_CRAP_ACTION_EXECUTABLE:?executable is not set}
sarif=${DOTNET_CRAP_ACTION_SARIF:-false}
[[ "$sarif" == true || "$sarif" == false ]] || fail "sarif must be true or false"

args=()
while IFS= read -r line || [[ -n "$line" ]]; do
  line=${line%$'\r'}
  if [[ -n "${line//[[:space:]]/}" ]]; then args+=("$line"); fi
done <<< "${DOTNET_CRAP_ACTION_ARGS:-}"

# With SARIF, one scan produces the report, so the uploaded report and the
# gate result always come from the same inputs. Options that would change or
# conflict with the SARIF report are rejected. The scan root is tracked so
# report paths can be made relative to the checkout.
scan_root=.
if [[ "$sarif" == true ]]; then
  previous=
  for arg in ${args[@]+"${args[@]}"}; do
    if [[ "$previous" == --root ]]; then scan_root=$arg; fi
    previous=$arg
    case "$arg" in
      --root=*) scan_root=${arg#--root=} ;;
      --format|--format=*|--output|--output=*)
        fail "do not pass $arg in args when sarif is true; the action writes the SARIF report" ;;
    esac
  done
  report=$(mktemp -d "${RUNNER_TEMP:?RUNNER_TEMP is not set}/dotnet-crap-sarif.XXXXXX")/dotnet-crap.sarif
fi

# Scan output contains PR-controlled text (paths, function names). Suspend
# workflow commands while it prints so it cannot forge annotations or outputs.
token=$(od -An -N16 -tx1 /dev/urandom | tr -d ' \n')
echo "::stop-commands::$token"
warning=
set +e
if [[ "$sarif" == false ]]; then
  "$executable" ${args[@]+"${args[@]}"}
  status=$?
else
  "$executable" --format sarif --output "$report" ${args[@]+"${args[@]}"}
  status=$?
  if [[ ! -f "$report" ]]; then
    :
  elif command -v python3 >/dev/null 2>&1; then
    helper="$(dirname "${BASH_SOURCE[0]}")/sarif.py"
    python3 -I -X utf8 "$helper" "$report" "$scan_root" "${GITHUB_WORKSPACE:-$PWD}"
    case $? in
      0) ;;
      3) warning="scan root $scan_root is outside the checkout; SARIF paths stay relative to it" ;;
      *) warning="could not post-process the SARIF report; its paths stay relative to the scan root" ;;
    esac
  else
    warning="python3 is not available; SARIF paths stay relative to the scan root"
  fi
fi
set -e
echo "::$token::"
if [[ -n "$warning" ]]; then echo "::warning title=dotnet-crap scan::$warning"; fi

# 0 = pass, 1 = gate failure, anything else = invalid setup or input.
if (( status > 1 )); then status=2; fi
echo "exit-code=$status" >> "$GITHUB_OUTPUT"
if [[ "$sarif" == true && -f "$report" ]]; then
  echo "sarif-file=$report" >> "$GITHUB_OUTPUT"
fi
