#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
stage="${1:-$root/target/dotnet-tool-stage}"
version="${2:-0.1.0}"
mkdir -p "$stage"
stage="$(cd "$stage" && pwd)"
project="$root/packaging/dotnet-crap-launcher/dotnet-crap-launcher.csproj"
rm -rf "$root/packaging/dotnet-crap-launcher/native"
mkdir -p "$root/packaging/dotnet-crap-launcher/native"
for rid in linux-x64 linux-arm64 osx-x64 osx-arm64 win-x64; do
  file=dotnet-crap
  [[ "$rid" == win-* ]] && file=dotnet-crap.exe
  source="$stage/$rid/$file"
  if [[ ! -f "$source" ]]; then
    if [[ "${DOTNET_CRAP_LOCAL_RID:-}" != "" && "$rid" != "$DOTNET_CRAP_LOCAL_RID" ]]; then continue; fi
    echo "Missing native binary: $source" >&2; exit 1
  fi
  mkdir -p "$root/packaging/dotnet-crap-launcher/native/$rid"
  install -m 755 "$source" "$root/packaging/dotnet-crap-launcher/native/$rid/$file"
done
dotnet pack "$project" -c Release -p:Version="$version" -o "$stage/feed"
