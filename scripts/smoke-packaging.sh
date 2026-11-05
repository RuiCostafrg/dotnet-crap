#!/usr/bin/env bash
# Local packaging smoke for Linux and macOS hosts. It runs the offline script
# checks, launcher tests, and a host-RID package build. Two packs run
# concurrently from separate stage directories against this checkout. The
# package is installed with dotnet tool install and the installed launcher is
# exercised. As root on Linux, the installed tool also runs as nobody from a
# read-only tree. Nothing is published, tagged, or signed.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
work="$(mktemp -d "${TMPDIR:-/tmp}/dotnet-crap-smoke.XXXXXX")"
trap 'rm -rf "$work"' EXIT

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

for script in scripts/*.sh; do
  bash -n "$script" || fail "syntax error in $script"
done
echo "PASS bash -n: scripts/*.sh"

bash scripts/test-install-action.sh
bash scripts/test-release-publish.sh
bash scripts/test-pack-publication.sh

dotnet run --project packaging/tests/DotnetCrapLauncher.Tests -c Release

case "$(uname -s)/$(uname -m)" in
  Linux/x86_64) rid=linux-x64 ;;
  Linux/aarch64 | Linux/arm64) rid=linux-arm64 ;;
  Darwin/x86_64) rid=osx-x64 ;;
  Darwin/arm64) rid=osx-arm64 ;;
  *) fail "no local RID for $(uname -s)/$(uname -m); run this smoke on Linux or macOS" ;;
esac

version="$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -1)"
[[ -n "$version" ]] || fail "could not read the Cargo version"

cargo build --locked --release
native="$root/target/release/dotnet-crap"
[[ -x "$native" ]] || fail "release build did not produce $native"

checkout_native="$root/packaging/dotnet-crap-launcher/native"
snapshot_native() {
  python3 -I - "$checkout_native" "$1" <<'PYTHON'
import hashlib, json, os, stat, sys
root, output = sys.argv[1:]
entries = []
def visit(path, relative):
    """Snapshot entry type, mode and bytes without following symbolic links."""
    try:
        info = os.lstat(path)
    except FileNotFoundError:
        if relative == ".":
            return
        raise
    mode = stat.S_IMODE(info.st_mode)
    if stat.S_ISLNK(info.st_mode):
        value = ("link", os.readlink(path))
    elif stat.S_ISDIR(info.st_mode):
        value = ("directory", None)
    elif stat.S_ISREG(info.st_mode):
        with open(path, "rb") as f:
            value = ("file", hashlib.sha256(f.read()).hexdigest())
    else:
        value = ("other", info.st_mode)
    entries.append((relative, mode, value))
    if stat.S_ISDIR(info.st_mode):
        for name in sorted(os.listdir(path)):
            visit(os.path.join(path, name), os.path.join(relative, name))
visit(root, ".")
with open(output, "w") as f:
    json.dump(entries, f)
PYTHON
}
snapshot_native "$work/native-before.json"

for stage in a b; do
  mkdir -p "$work/stage-$stage/$rid"
  cp "$native" "$work/stage-$stage/$rid/dotnet-crap"
done

# Two packs at once: each must stage natives and build obj/bin in its own work directory.
DOTNET_CRAP_LOCAL_RID="$rid" bash scripts/pack-dotnet-tool.sh "$work/stage-a" "$version" > "$work/pack-a.log" 2>&1 &
pid_a=$!
DOTNET_CRAP_LOCAL_RID="$rid" bash scripts/pack-dotnet-tool.sh "$work/stage-b" "$version" > "$work/pack-b.log" 2>&1 &
pid_b=$!
status_a=0
status_b=0
wait "$pid_a" || status_a=$?
wait "$pid_b" || status_b=$?
if [[ "$status_a" -ne 0 || "$status_b" -ne 0 ]]; then
  cat "$work/pack-a.log" "$work/pack-b.log" >&2
  fail "concurrent pack failed (a=$status_a, b=$status_b)"
fi
echo "PASS concurrent packs: both succeeded from separate stages"

snapshot_native "$work/native-after.json"
cmp -s "$work/native-before.json" "$work/native-after.json" || fail "packing changed $checkout_native"

nupkg_a="$work/stage-a/feed/dotnet-crap.$version.nupkg"
nupkg_b="$work/stage-b/feed/dotnet-crap.$version.nupkg"
[[ -f "$nupkg_a" && -f "$nupkg_b" ]] || fail "expected packages were not written to each feed"

# Inspect archive entries and native bytes; -I isolates Python's import environment.
python3 -I - "$nupkg_a" "$nupkg_b" "$rid" "$version" "$native" <<'PY'
import sys
import zipfile

nupkg_a, nupkg_b, rid, version, native_path = sys.argv[1:6]
expected_native = f"tools/net10.0/any/native/{rid}/dotnet-crap"

def entries(path):
    """List package entries excluding the randomized core-properties metadata name."""
    # NuGet names its core-properties part with a random GUID; ignore that metadata-only name.
    with zipfile.ZipFile(path) as archive:
        return sorted(name for name in archive.namelist()
                      if not name.startswith("package/services/metadata/core-properties/"))

names_a = entries(nupkg_a)
names_b = entries(nupkg_b)
if names_a != names_b:
    sys.exit("concurrent packages differ in their entries")
natives = [name for name in names_a if name.startswith("tools/net10.0/any/native/")]
if natives != [expected_native]:
    sys.exit(f"expected only {expected_native}, found {natives}")
with open(native_path, "rb") as source:
    expected_bytes = source.read()
for path in (nupkg_a, nupkg_b):
    with zipfile.ZipFile(path) as archive:
        if archive.read(expected_native) != expected_bytes:
            sys.exit(f"{path}: native entry differs from the local build input")
for required in ("tools/net10.0/any/dotnet-crap-launcher.dll", "dotnet-crap.nuspec", "README.md"):
    if required not in names_a:
        sys.exit(f"package lacks {required}")
with zipfile.ZipFile(nupkg_a) as archive:
    nuspec = archive.read("dotnet-crap.nuspec").decode("utf-8")
if f"<version>{version}</version>" not in nuspec:
    sys.exit("nuspec version does not match the requested version")
print(f"PASS package contents: {expected_native} is the only native entry")
PY

tool="$work/tool"
printf '%s\n' '<configuration><packageSources><clear /></packageSources></configuration>' > "$work/NuGet.Config"
NUGET_PACKAGES="$work/restore-cache" dotnet tool install --tool-path "$tool" --configfile "$work/NuGet.Config" --add-source "$work/stage-a/feed" --version "$version" dotnet-crap \
  > "$work/install.log" 2>&1 || { cat "$work/install.log" >&2; fail "dotnet tool install failed"; }
launcher="$tool/dotnet-crap"

"$launcher" --version | grep -qF "$version" || fail "installed launcher did not report version $version"
"$launcher" --root examples/Fixture --threshold 100 --fail-on-empty > "$work/fixture-pass.out"
set +e
"$launcher" --root examples/Fixture --threshold 0 > "$work/fixture-fail.out" 2>&1
fail_code=$?
set -e
[[ "$fail_code" -eq 1 ]] || fail "threshold 0 should exit 1 through the launcher, got $fail_code"
echo "PASS installed launcher: --version, passing fixture gate, failing gate exit 1"

installed_native="$(find "$tool/.store" -path "*/tools/net10.0/any/native/$rid/dotnet-crap" -type f | head -1)"
[[ -n "$installed_native" ]] || fail "installed package has no native executable for $rid"
cmp -s "$native" "$installed_native" || fail "installed native bytes differ from the locally built package input"
echo "Installed native mode: $(ls -l "$installed_native" | cut -c1-10)"

if [[ "$(id -u)" == 0 ]] && command -v setpriv >/dev/null 2>&1 && id nobody >/dev/null 2>&1; then
  nobody_uid="$(id -u nobody)"
  nobody_gid="$(id -g nobody)"
  # The tool tree is read-only for everyone, and the nobody user needs only read and traverse access.
  chmod a+rx "$work"
  chmod -R a+rX "$tool"
  chmod -R a-w "$tool"
  setpriv --reuid="$nobody_uid" --regid="$nobody_gid" --clear-groups "$launcher" --version | grep -qF "$version" \
    || fail "read-only installed launcher failed as nobody"
  setpriv --reuid="$nobody_uid" --regid="$nobody_gid" --clear-groups \
    "$launcher" --root examples/Fixture --threshold 100 --fail-on-empty > /dev/null \
    || fail "read-only installed fixture gate failed as nobody"
  echo "PASS read-only install: launcher ran as nobody from an unwritable tool tree"
else
  echo "NOTE: skipped read-only install run as nobody (requires root, setpriv, and a nobody user);"
  echo "      the launcher tests cover read-only permissions when run as a non-root user."
fi

echo "PASS local packaging smoke for $rid, version $version"
