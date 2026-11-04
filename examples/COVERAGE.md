# Native coverage input

Pass `--coverage path.json`. The initial contract is version 1:

```json
{
  "version": 1,
  "files": {
    "src/Calculator.cs": {
      "sha256": "<64 hexadecimal characters: SHA-256 of exact source bytes>",
      "lines": { "5": 1, "8": 1, "11": 0 }
    }
  }
}
```

File keys are normalized slash-separated paths relative to `--root`. Line keys
are canonical positive one-based decimal integers. Hit counts are unsigned
integers; positive counts mean covered. Unknown fields, duplicate map keys,
invalid paths/hashes/lines and selected-source hash mismatches are errors.
Capture hashes before tests and check the bytes again afterward. Hashing an
existing old report's source does not establish freshness.

Each supplied point inside a callable's span contributes equally. The producer
must include zero-hit executable points: omitted points do not enter the
coverage denominator. The scanner does not derive statement completeness yet.
A callable with no supplied points has absent coverage, scored as 0%; enabling
`--require-coverage` makes absent evidence a gate failure. Nested callable line
spans belong to the nested callable and are excluded from the parent. Callables
sharing a source line are ambiguous; put them on separate lines for this MVP.

JSON output uses fractions from 0 to 1. CRAP is
`complexity² × (1 − coverage)³ + complexity`. Missing coverage is never 100%.

## Live demonstration

`CoverageFixture` is an actual .NET 10 program with a calculator and three
explicit probe points. The runner initializes every point to zero and executes
one path (`partial`) or both paths (`full`). It needs no third-party packages.
This is a small instrumentation example, not Coverlet collection.

```sh
DOTNET_CRAP_E2E=1 cargo test --locked --test live_coverage
```

The test snapshots the source, executes C#, writes the native coverage map,
invokes the Rust CLI, and verifies exact results: complexity 2; uncovered score
6; partial coverage 2/3 and score approximately 2.148148; full coverage 1 and
score 2. With threshold 2 only the fully covered run passes. Changing the source
then causes stale-coverage rejection.

Fresh Coverlet/Cobertura and Microsoft coverage integration is the next milestone
in [the parity roadmap](../docs/FEATURE_PARITY.md).
