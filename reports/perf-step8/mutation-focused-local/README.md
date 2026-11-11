# Focused local mutation audit (changed functions)

- **Command:** `cargo mutants --output target/mutation-focused --jobs 2
  --jobserver-tasks 2 --timeout 120 --build-timeout 600 --file src/lib.rs
  --file src/cobertura.rs --re 'CoverageIndex|merge_ranges|span_counts|span_coverage| in collect$|replace collect |import_class|import_report|parse_reports' -C --locked`
- **Source and tool:** production source `01d756c` (identical to the PR head's
  `src/`), cargo-mutants 27.1.0.
- **Runner:** the Android/aarch64 PRoot runner described in `../README.md`.
- **Result:** 91 mutants, 81 caught, 8 missed, 0 timeouts, 2 unviable.
- **Missed mutants:** all 8 are in unchanged `import_report` lines 73–80, the
  16 MiB report-size-limit arithmetic. They belong to the 24 survivors already
  reviewed in `../../fresh-coverlet/survivor-review.md` and also survive in the
  full CI run (`../mutation-ci/`).
- **New code:** every mutant in `CoverageIndex::count`, `merge_ranges`,
  `span_counts`, `span_coverage`, `collect`, `import_class` and `parse_reports`
  is caught.

This is corroborating local evidence. The authoritative full gate is the CI run
in `../mutation-ci/`.
