# Compatibility policy

Since 1.0.0, `dotnet-crap` follows [Semantic Versioning](https://semver.org/).
This page defines what that promises. A change that breaks anything listed under
*Stable* needs a new major version.

## Stable in 1.x

- **Command line.** Every flag, its meaning and its default, for scans and for
  `collect`.
- **Exit codes.** `0` pass, `1` policy failure, `2` input or runtime error.
- **JSON report v1** (`--format json`, [schema](../schemas/report.v1.schema.json)).
  Fields may be *added* in minor releases; none is ever removed, renamed or
  retyped, and the `version` field stays `1`.
- **Input formats.** Native coverage v1
  ([schema](../schemas/coverage.v1.schema.json), [guide](../examples/COVERAGE.md)),
  `dotnet-crap.json` v1 ([schema](../schemas/config.v1.schema.json),
  [guide](CONFIG.md)) and baseline v1
  ([schema](../schemas/baseline.v1.schema.json), [guide](BASELINES.md)). Every
  1.x release keeps reading every v1 file that an earlier 1.x release accepted.
  A new version number means a new format; older versions keep being read for
  the rest of 1.x.
- **SARIF.** Rule IDs and `partialFingerprints` ([SARIF.md](SARIF.md)).
- **Scoring.** The CRAP formula (`complexity² × (1 − coverage)³ + complexity`,
  see the [README](../README.md)) and the documented complexity and ownership
  conventions ([SCORING_ACCURACY.md](SCORING_ACCURACY.md),
  [CSHARP_CALLABLES.md](CSHARP_CALLABLES.md),
  [CSHARP_PATTERNS.md](CSHARP_PATTERNS.md)).

## Allowed in minor releases

- New flags, and new optional config keys (existing files stay valid).
- New fields in the JSON report.
- New report formats.
- Parser or grammar upgrades that make more files parse, or that score syntax
  that was previously unsupported. These are called out in the release notes.
- Performance changes.

## Not covered

- The layout of human-readable text output. Do not parse it; use JSON or SARIF.
- The Rust library API (`dotnet_crap::…`). The crate is not published on
  crates.io and may change in minor releases until stated otherwise.
- Exact error message wording.
- Run time.

## Deprecation

A flag or config key scheduled for removal keeps working, with a warning on
standard error, for at least one minor release before the next major release
removes it.

## Schemas

The machine formats have [JSON Schema](https://json-schema.org/) (draft 2020-12)
files in [`schemas/`](../schemas/), with `$id`s of the form
`https://raw.githubusercontent.com/RuiCostafrg/dotnet-crap/main/schemas/<name>.v1.schema.json`:

| Format | Schema | Direction |
| --- | --- | --- |
| JSON report | `report.v1.schema.json` | output |
| Native coverage | `coverage.v1.schema.json` | input |
| Config | `config.v1.schema.json` | input |
| Baseline | `baseline.v1.schema.json` | input |

Do not add a `"$schema"` key to a config or baseline file: the readers reject
unknown keys. Point your editor's schema mapping at the file instead.

Output schemas (the report) allow unknown fields at every level: **consumers
must ignore fields they do not know**, because minor releases may add them.
Input schemas reject unknown fields, as the readers do. The test suite validates
real output against every schema and checks that the input schemas and the
readers accept and reject the same documents. The one rule a schema cannot
express is duplicate JSON map keys in native coverage, which the reader rejects.
