#!/usr/bin/env bash
# Offline checks for the release publication scripts. Fake gh and dotnet
# executables record their arguments and fail on request, so the retry and
# error-surfacing paths run without publishing anything. The trusted-main check
# uses a real local git remote.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
work="$(mktemp -d "${TMPDIR:-/tmp}/dotnet-crap-publish-test.XXXXXX")"
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/bin" "$work/assets" "$work/published"
printf 'native' > "$work/assets/dotnet-crap-v1.2.3-linux-x64.tar.gz"
printf 'package' > "$work/assets/dotnet-crap.nupkg"
cp "$work"/assets/* "$work/published/"

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

# gh: create/view/upload behavior is controlled by FAKE_GH_* variables.
cat > "$work/bin/gh" <<'FAKE_GH'
#!/usr/bin/env bash
printf '%s\n' "$*" >> "$FAKE_LOG"
case "$1 $2" in
  "release create")
    if [[ "${FAKE_GH_CREATE:-ok}" == ok ]]; then echo "https://example.invalid/release"; exit 0; fi
    echo "HTTP 422: Validation Failed (release already_exists)" >&2; exit 1 ;;
  "release view")
    [[ "${FAKE_GH_VIEW:-ok}" == ok ]] && exit 0
    echo "release not found" >&2; exit 1 ;;
  "release download")
    [[ "${FAKE_GH_DOWNLOAD:-ok}" == ok ]] || exit 1
    shift 3
    while (($#)); do
      case "$1" in
        --pattern) pattern="$2"; shift 2 ;;
        --dir) destination="$2"; shift 2 ;;
        *) exit 99 ;;
      esac
    done
    cp "$FAKE_PUBLISHED/$pattern" "$destination/$pattern" ;;
  *) echo "fake gh: unexpected $*" >&2; exit 99 ;;
esac
FAKE_GH
cat > "$work/bin/dotnet" <<'FAKE_DOTNET'
#!/usr/bin/env bash
printf '%s\n' "$*" >> "$FAKE_LOG"
if [[ "${FAKE_DOTNET_EXIT:-0}" != 0 ]]; then echo "error: push failed" >&2; fi
exit "${FAKE_DOTNET_EXIT:-0}"
FAKE_DOTNET
chmod 755 "$work/bin/gh" "$work/bin/dotnet"

run_with_fakes() {
  # Remaining arguments are the command; FAKE_* variables come from the caller's environment.
  : > "$work/log"
  env PATH="$work/bin:$PATH" FAKE_LOG="$work/log" FAKE_PUBLISHED="$work/published" "$@"
}

# GitHub release: create succeeds.
run_with_fakes env FAKE_GH_CREATE=ok bash "$root/scripts/publish-github-release.sh" v1.2.3 "$work"/assets/* \
  > "$work/out" 2>&1 || { cat "$work/out" >&2; fail "fresh release creation failed"; }
grep -Fq -- "release create v1.2.3 $work/assets/dotnet-crap-v1.2.3-linux-x64.tar.gz $work/assets/dotnet-crap.nupkg " "$work/log" || fail "release create did not receive both expected assets"
grep -q -- "--verify-tag" "$work/log" || fail "create did not pass --verify-tag"
grep -q -- "upload" "$work/log" && fail "fresh release should not upload separately"

# GitHub release: a retry preserves matching existing assets.
run_with_fakes env FAKE_GH_CREATE=exists FAKE_GH_VIEW=ok FAKE_GH_DOWNLOAD=ok \
  bash "$root/scripts/publish-github-release.sh" v1.2.3 "$work"/assets/* > "$work/out" 2>&1 \
  || { cat "$work/out" >&2; fail "retry against an existing release did not reuse it"; }
grep -q -- "release download v1.2.3" "$work/log" || fail "retry did not verify published assets"
grep -q -- "release upload" "$work/log" && fail "retry modified immutable published assets"
grep -q "already exists" "$work/out" || fail "retry did not announce reuse"

# GitHub release: creation fails and no release is readable; the failure must surface.
if run_with_fakes env FAKE_GH_CREATE=fail FAKE_GH_VIEW=missing \
  bash "$root/scripts/publish-github-release.sh" v1.2.3 "$work"/assets/* > "$work/out" 2>&1; then
  fail "unreadable release after a failed create was reported as success"
fi
grep -q "could not be created" "$work/out" || fail "creation failure was not reported"
grep -q "release upload" "$work/log" && fail "upload was attempted without a readable release"

# GitHub release: failed downloads surface and never modify published assets.
if run_with_fakes env FAKE_GH_CREATE=exists FAKE_GH_VIEW=ok FAKE_GH_DOWNLOAD=fail \
  bash "$root/scripts/publish-github-release.sh" v1.2.3 "$work"/assets/* > "$work/out" 2>&1; then
  fail "failed asset verification was reported as success"
fi

# A rebuilt package must not replace published bytes for an immutable version.
printf 'different rebuilt package' > "$work/assets/dotnet-crap.nupkg"
if run_with_fakes env FAKE_GH_CREATE=exists FAKE_GH_VIEW=ok FAKE_GH_DOWNLOAD=ok \
  bash "$root/scripts/publish-github-release.sh" v1.2.3 "$work"/assets/* > "$work/out" 2>&1; then
  fail "mismatched rebuilt assets were accepted"
fi
grep -q "differs from this build" "$work/out" || fail "mismatched assets were not explained"
grep -q "release upload" "$work/log" && fail "mismatched assets were uploaded"
[[ "$(cat "$work/published/dotnet-crap.nupkg")" == package ]] || fail "published package changed"
printf 'package' > "$work/assets/dotnet-crap.nupkg"

# GitHub release: missing assets are rejected before gh runs.
if run_with_fakes bash "$root/scripts/publish-github-release.sh" v1.2.3 "$work/assets/missing.tar.gz" > "$work/out" 2>&1; then
  fail "missing asset was accepted"
fi
[[ ! -s "$work/log" ]] || fail "missing asset still called gh"

# NuGet: a retry passes --skip-duplicate, and a real push error still fails.
run_with_fakes env NUGET_API_KEY=test-key-not-secret FAKE_DOTNET_EXIT=0 \
  bash "$root/scripts/publish-nuget.sh" "$work/assets" > "$work/out" 2>&1 || fail "NuGet push failed"
grep -Fq -- "nuget push $work/assets/dotnet-crap.nupkg " "$work/log" || fail "NuGet push did not receive the expected package"
grep -q -- "--skip-duplicate" "$work/log" || fail "NuGet push lacks --skip-duplicate"
grep -q -- "--api-key test-key-not-secret" "$work/log" || fail "NuGet push did not use the supplied key"

if run_with_fakes env NUGET_API_KEY=test-key-not-secret FAKE_DOTNET_EXIT=1 \
  bash "$root/scripts/publish-nuget.sh" "$work/assets" > "$work/out" 2>&1; then
  fail "real NuGet push error was reported as success"
fi

# NuGet: an absent secret skips publication explicitly and never calls dotnet.
run_with_fakes env -u NUGET_API_KEY bash "$root/scripts/publish-nuget.sh" "$work/assets" > "$work/out" 2>&1 \
  || fail "absent NUGET_API_KEY should skip, not fail"
grep -q "skipping NuGet publication" "$work/out" || fail "absent secret was not announced"
[[ ! -s "$work/log" ]] || fail "absent secret still called dotnet"

# NuGet: more than one package is ambiguous and must not be pushed.
mkdir -p "$work/two"
printf 'a' > "$work/two/a.nupkg"
printf 'b' > "$work/two/b.nupkg"
if run_with_fakes env NUGET_API_KEY=test-key-not-secret bash "$root/scripts/publish-nuget.sh" "$work/two" > "$work/out" 2>&1; then
  fail "ambiguous package directory was accepted"
fi
[[ ! -s "$work/log" ]] || fail "ambiguous package directory still called dotnet"

# Trusted main: a commit on main passes; a tag commit that only exists on a branch fails.
git_identity=(-c user.name=smoke -c user.email=smoke@invalid.example)
remote="$work/remote.git"
seed="$work/seed"
git init -q --bare "$remote"
git -C "$remote" symbolic-ref HEAD refs/heads/main
git init -q -b main "$seed"
git -C "$seed" "${git_identity[@]}" commit -q --allow-empty -m "main commit"
on_main="$(git -C "$seed" rev-parse HEAD)"
git -C "$seed" checkout -q -b side
git -C "$seed" "${git_identity[@]}" commit -q --allow-empty -m "branch-only commit"
off_main="$(git -C "$seed" rev-parse HEAD)"
git -C "$seed" remote add origin "$remote"
git -C "$seed" push -q origin main side

checkout="$work/checkout"
git clone -q "$remote" "$checkout"
(
  cd "$checkout"
  bash "$root/scripts/verify-tag-on-main.sh" "$on_main" > "$work/out" 2>&1 || { cat "$work/out" >&2; fail "main commit rejected"; }
  if bash "$root/scripts/verify-tag-on-main.sh" "$off_main" > "$work/out" 2>&1; then
    fail "branch-only tag commit was accepted"
  fi
  grep -q "not reachable from origin/main" "$work/out" || fail "branch-only rejection was not explained"
)

# Resolve both lightweight and annotated remote tags, then reject moved and missing tags.
git -C "$seed" tag light "$on_main"
git -C "$seed" "${git_identity[@]}" tag -a annotated "$on_main" -m annotated
git -C "$seed" push -q origin light annotated
(
  cd "$checkout"
  bash "$root/scripts/verify-remote-tag.sh" light "$on_main" || fail "matching lightweight tag rejected"
  bash "$root/scripts/verify-remote-tag.sh" annotated "$on_main" || fail "matching annotated tag rejected"
  if bash "$root/scripts/verify-remote-tag.sh" missing "$on_main" > "$work/out" 2>&1; then fail "missing tag accepted"; fi
  git -C "$seed" tag -f light "$off_main" >/dev/null
  git -C "$seed" push -q --force origin light
  if bash "$root/scripts/verify-remote-tag.sh" light "$on_main" > "$work/out" 2>&1; then fail "moved tag accepted"; fi
)

echo "PASS publish scripts: retry reuse, surfaced failures, NuGet skip-duplicate, absent-secret skip, trusted-main check"
