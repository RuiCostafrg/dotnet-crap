#!/usr/bin/env bash
# Race two distinct fake pack outputs for the same destination without using a network.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
work="$(mktemp -d "${TMPDIR:-/tmp}/dotnet-crap-pack-test.XXXXXX")"
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/bin" "$work/stage/linux-x64"
printf native > "$work/stage/linux-x64/dotnet-crap"
cat > "$work/bin/dotnet" <<'FAKE'
#!/usr/bin/env bash
set -euo pipefail
while (($#)); do
  if [[ "$1" == -o ]]; then mkdir -p "$2"; printf '%s' "$FAKE_PACKAGE" > "$2/dotnet-crap.1.2.3.nupkg"; exit; fi
  shift
done
exit 1
FAKE
chmod +x "$work/bin/dotnet"
for value in first second; do
  (umask 077; PATH="$work/bin:$PATH" DOTNET_CRAP_LOCAL_RID=linux-x64 FAKE_PACKAGE="$value" bash "$root/scripts/pack-dotnet-tool.sh" "$work/stage" 1.2.3) > "$work/$value.log" 2>&1 &
  if [[ "$value" == first ]]; then first=$!; else second=$!; fi
done
status1=0; status2=0
wait "$first" || status1=$?
wait "$second" || status2=$?
if ! { [[ "$status1" == 0 && "$status2" != 0 ]] || [[ "$status1" != 0 && "$status2" == 0 ]]; }; then
  cat "$work/first.log" "$work/second.log" >&2; echo 'FAIL: expected exactly one differing package to win' >&2; exit 1
fi
winner="$(cat "$work/stage/feed/dotnet-crap.1.2.3.nupkg")"
python3 -I - "$work/stage/feed/dotnet-crap.1.2.3.nupkg" <<'PY'
import os, stat, sys
mode = stat.S_IMODE(os.stat(sys.argv[1]).st_mode)
assert mode == 0o644, f"published package mode is {mode:o}, expected 644"
PY
[[ "$winner" == first || "$winner" == second ]] || exit 1
PATH="$work/bin:$PATH" DOTNET_CRAP_LOCAL_RID=linux-x64 FAKE_PACKAGE="$winner" bash "$root/scripts/pack-dotnet-tool.sh" "$work/stage" 1.2.3 >/dev/null
[[ "$(cat "$work/stage/feed/dotnet-crap.1.2.3.nupkg")" == "$winner" ]]
echo 'PASS pack publication: concurrent differing bytes fail closed; identical bytes accepted'
