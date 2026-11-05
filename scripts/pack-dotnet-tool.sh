#!/usr/bin/env bash
# Packs the dotnet-crap .NET tool from native executables staged at
# <stage>/<rid>/dotnet-crap[.exe]. The launcher project is copied into a
# per-invocation work directory, so concurrent packs never share native
# staging, bin, or obj paths and the checkout is left unchanged.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
stage_arg="${1:-$root/target/dotnet-tool-stage}"
version="${2:-0.1.0}"

if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]]; then
  echo "Invalid package version: $version" >&2
  exit 2
fi

mkdir -p "$stage_arg"
stage="$(cd "$stage_arg" && pwd)"
launcher_source="$root/packaging/dotnet-crap-launcher"
work="$(mktemp -d "${TMPDIR:-/tmp}/dotnet-crap-pack.XXXXXX")"
trap 'rm -rf "$work"' EXIT

# Copy the launcher sources, excluding any local build or native output in the checkout.
project="$work/project"
mkdir -p "$project"
tar -C "$launcher_source" --exclude=bin --exclude=obj --exclude=native -cf - . | tar -C "$project" -xf -

native_dir="$project/native"
staged=0
for rid in linux-x64 linux-arm64 osx-x64 osx-arm64 win-x64; do
  file=dotnet-crap
  [[ "$rid" == win-* ]] && file=dotnet-crap.exe
  source="$stage/$rid/$file"
  if [[ ! -f "$source" ]]; then
    if [[ -n "${DOTNET_CRAP_LOCAL_RID:-}" && "$rid" != "$DOTNET_CRAP_LOCAL_RID" ]]; then continue; fi
    echo "Missing native binary: $source" >&2
    exit 1
  fi
  mkdir -p "$native_dir/$rid"
  install -m 755 "$source" "$native_dir/$rid/$file"
  staged=$((staged + 1))
done

if [[ "$staged" -eq 0 ]]; then
  echo "No native binaries were staged; check DOTNET_CRAP_LOCAL_RID and the stage layout." >&2
  exit 1
fi

dotnet pack "$project/dotnet-crap-launcher.csproj" -c Release -p:Version="$version" -o "$work/feed"

packages=("$work"/feed/*.nupkg)
if [[ "${#packages[@]}" -ne 1 || ! -f "${packages[0]}" ]]; then
  echo "Expected exactly one package from dotnet pack." >&2
  exit 1
fi

mkdir -p "$stage/feed"
# Publish atomically within the feed so an interrupted copy never leaves a partial .nupkg.
partial="$(mktemp "$stage/feed/.package.XXXXXX")"
trap 'rm -f "$partial"; rm -rf "$work"' EXIT
cp "${packages[0]}" "$partial"
destination="$stage/feed/$(basename "${packages[0]}")"
# A hard link creates the final name exclusively and atomically on the same filesystem.
if ! ln "$partial" "$destination" 2>/dev/null; then
  if [[ ! -f "$destination" ]] || ! cmp -s "$partial" "$destination"; then
    echo "Package already exists with different bytes or cannot be published: $destination" >&2
    exit 1
  fi
fi
rm -f "$partial"
echo "Packed $stage/feed/$(basename "${packages[0]}") with $staged native binary file(s)."
