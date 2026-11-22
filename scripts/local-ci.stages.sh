# Stage definitions for the shared verified-release-kit runner.
LOCAL_CI_NAME="dotnet-crap"
LOCAL_CI_STAGES=(tests lint security mutation e2e)
LOCAL_CI_DEFAULT_STAGES=(tests lint security)

prepare() { export PYTHONDONTWRITEBYTECODE=1; }

stage_tests() {
  require cargo "Rust toolchain (https://rustup.rs)"
  require python3 ""
  python3 -m unittest discover -s tests -p 'test_*.py'
  cargo test --locked
  cargo test --locked --release
}

stage_lint() {
  require cargo ""
  cargo fmt --check
  cargo clippy --locked --all-targets -- -D warnings
}

stage_security() {
  require cargo ""
  if ! command -v cargo-audit >/dev/null 2>&1; then
    _fail "cargo-audit missing — cargo install cargo-audit --version 0.23.0 --locked"
  fi
  cargo audit --deny warnings
}

stage_mutation() {
  # Very slow; not part of the default set.
  require cargo ""
  if ! command -v cargo-mutants >/dev/null 2>&1; then
    _fail "cargo-mutants missing — cargo install cargo-mutants --version 27.1.0 --locked"
  fi
  python3 -m unittest discover -s tests -p 'test_*.py'
  python3 scripts/mutation-gate.py
}

stage_e2e() {
  # .NET launcher/install E2E contracts; needs the .NET 10 SDK.
  require dotnet ".NET 10 SDK (https://dotnet.microsoft.com/download)"
  require cargo ""
  dotnet run --project packaging/tests/DotnetCrapLauncher.Tests -c Release
  bash scripts/test-install-action.sh
  bash scripts/test-release-publish.sh
  bash scripts/smoke-packaging.sh
  DOTNET_CRAP_E2E=1 cargo test --locked
}
