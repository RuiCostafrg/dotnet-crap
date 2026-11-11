# Architecture boundaries evidence (PLAN step 2a)

This covers [PR #17](https://github.com/RuiCostafrg/dotnet-crap/pull/17) (module
split, `output::write_report`, pure collector planning) and stacked
[PR #18](https://github.com/RuiCostafrg/dotnet-crap/pull/18) (typed
`dotnet_crap::Error`). Host: Linux arm64 (PRoot), Rust/Clippy 1.96,
.NET SDK 10.0.109 (VSTest 18.0.2), cargo-mutants 27.1.0. macOS and Windows
runs are left to hosted CI.

Commit identities: after PR #15 merged, both branches were rebased onto `main`
(`77003a6`). That only added `docs/local-ci.md` and `scripts/local-ci.sh`, so the
pre-rebase SHAs below have byte-identical `src/`, `tests/`, `Cargo.*` trees to
their rebased counterparts.

## Gates

[gate-summary.txt](gate-summary.txt) has extracts from the logs.

| Gate | Tree | Result |
| --- | --- | --- |
| `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings` | PR B `816dee0` ≡ `5745f83` | pass |
| `cargo test --locked`, `cargo test --locked --release`, `cargo doc --no-deps` | PR B `816dee0` | pass |
| `DOTNET_CRAP_E2E=1 DOTNET_CRAP_COVERLET_E2E=1 cargo test --locked` | PR B `816dee0` | pass |
| Live suites `coverlet_live`, `live_coverage`, `real_csharp_accuracy`, `cli` | PR A `dd01ca3` ≡ `e40ad85` | pass |
| `python3 -m unittest discover -s tests -p 'test_*.py'` | PR B `816dee0` | 15 OK |

The live Coverlet test runs pinned Coverlet 6.0.4 VSTest collection for actual
**xUnit, MSTest and NUnit** projects. It also runs the multi-project and failing-test
cases and the negative MTP-mode SDK probe. Both live runs took more than 60
seconds of real SDK work; neither returned early.

One earlier live run, on the split-only commit `9da4af3`, hung in the negative
MTP probe (`dotnet test --solution` with an MTP `global.json`). By then the three
framework collections had passed. The probe sat idle for 30 minutes with no
test host while other worktrees were compiling, and was killed. The
clean rerun above on the later PR A head passed, probe included. This is
recorded as a host/SDK stall, not as evidence for or against the change.

## Mutation gate

`python3 scripts/mutation-gate.py` on PR B head `5745f83`, with no exclusions or
configuration:

| Total | Caught | Missed | Timeout | Unviable | Score |
| --- | --- | --- | --- | --- | --- |
| 323 | 279 | 22 | 0 | 22 | **92.69%** (279 / 301 viable) |

The complete inventory and outcomes are in [mutation/](mutation/): `mutants.json`,
`caught.txt`, `missed.txt`, `timeout.txt` and `unviable.txt`.

### Inventory audit: every production module is mutated

| File | Mutants | Missed | | File | Mutants | Missed |
| --- | --- | --- | --- | --- | --- | --- |
| `analysis.rs` (new) | 86 | 0 | | `cobertura.rs` | 81 | 18 |
| `evidence.rs` (new) | 26 | 0 | | `collector.rs` | 31 | 4 |
| `selection.rs` (new) | 16 | 0 | | `lib.rs` | 30 | 0 |
| `source.rs` (new) | 5 | 0 | | `sarif.rs` | 17 | 0 |
| `report.rs` (new) | 6 | 0 | | `main.rs` | 8 | 0 |
| `output.rs` (new) | 7 | 0 | | | | |
| `error.rs` (new) | 10 | 0 | | | | |

All 12 `src/*.rs` files appear. The seven new modules have no surviving mutants.

### Survivor review

- **21 of 22** have the same function and mutation as cases already reviewed in
  the [PR #15 survivor review](../fresh-coverlet/survivor-review.md), shifted only
  by line number:
  - `cobertura.rs`: `import_report` ×8 (size-limit arithmetic and boundaries),
    `resolve_filename` ×4, `add_candidate` ×2, `normalize_path` ×4 and
    `validate_filename` ×1.
  - `collector.rs`: `collect:59` (colon/backslash rejection after separator
    normalization) and `validate_destination`'s `NotFound` guard.
  This PR does not change those functions' logic. Their reasoning and
  qualifications still apply and are not re-declared equivalent here.
- **1 new**: `collector.rs:155 find_reports`, `&&` → `||` in the report-name and
  regular-file filter. It was reachable but untested. It is fixed in PR A
  `c55d043` (attributes corrected in `883a765`) by
  `find_reports_counts_only_regular_report_files`, which places an
  unrelated file and a directory named `coverage.cobertura.xml` beside a report.
  A targeted re-run (`cargo mutants --file src/collector.rs --re find_reports`)
  run on `883a765` caught all 7 `find_reports` mutants:
  [targeted-find-reports-caught.txt](targeted-find-reports-caught.txt).
  The full-run figures above predate that test.

### Unviable mutants (22)

All are type-level replacements that do not compile, such as
`Ok(Default::default())` for `Result<ScanResult>`/`Document`, `Box::leak` for
borrowed returns, and `Default` for `ErrorKind`, `Coverage` and `Error`. Two are
`cobertura.rs` `>` → `<` replacements after a cast (`x as u64 < y`), which Rust
parses as a generic argument list. Their `==`/`>=` siblings are viable and are
listed above. None is counted as caught.
