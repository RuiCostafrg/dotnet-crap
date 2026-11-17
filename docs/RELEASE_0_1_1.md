# 0.1.1 preparation (unreleased)

This is a source preparation milestone, not a qualified release candidate or
permission to tag or publish. The immutable
[v0.1.0 release](https://github.com/RuiCostafrg/dotnet-crap/releases/tag/v0.1.0)
remains unchanged at commit `2565412e3d2326d79e98963ed0b8df7de3020659`.

## Audited changes since v0.1.0

The comparison endpoint for these notes is main at
`022484982d9c034cb01fd775b187af6876c533a3` (PR #41 merged). These notes must be
refreshed after the remaining distribution PRs integrate.

- **C# logical patterns:** each explicit `and` or `or` pattern combinator now
  adds one complexity decision, including nested property/list patterns,
  switch arms and guards. Earlier releases undercount these constructs.
  Parentheses, relational/type/constant tests and `not` add no separate
  decisions. Switch, Boolean-expression and nested-callable ownership rules
  are unchanged. See [the scoring convention and physical examples](CSHARP_PATTERNS.md).
  Complexity and therefore CRAP scores can increase for affected callables;
  an existing threshold can now fail. For example, an `if` with one `and` and
  one `or` has complexity 4; at 50% supplied-line coverage its CRAP is 6,
  and at full coverage it is 4.
- **Published verification pagination:** GitHub workflow-run and artifact
  lookups now flatten every raw JSON response page locally. This removes the
  incompatible `gh api --slurp --jq` combination and retains records beyond
  the first page. This fix does not constitute live published-path evidence.
- **Version preparation:** Cargo manifest/lockfile and the .NET launcher
  project declare 0.1.1. The CLI and SARIF driver obtain the version from Cargo
  at build time. The composite Action has no version default; consumers must
  explicitly choose a published version. Historical 0.1.0 test data and
  published-version examples are intentionally retained.

PRs [#39](https://github.com/RuiCostafrg/dotnet-crap/pull/39) and
[#41](https://github.com/RuiCostafrg/dotnet-crap/pull/41) account for the functional
changes in that comparison. This is not a claim of full Roslyn parity, complete
statement coverage, or implementation of the remaining feature backlog.

## Changes after the audit endpoint

These merged after the comparison endpoint above and are not yet covered by an
audit of the final tree.

### Rust library API break

`ScanOptions` and `CollectOptions` gained public fields: `jobs`
([#46](https://github.com/RuiCostafrg/dotnet-crap/pull/46)), `defines`
([#48](https://github.com/RuiCostafrg/dotnet-crap/pull/48)) and `skip_unparsed`
([#52](https://github.com/RuiCostafrg/dotnet-crap/pull/52)). `Report` gained
`skipped`. Code that builds these structs with a literal listing every field no
longer compiles. To migrate:

- `ScanOptions`: end the literal with `..Default::default()`, or add
  `jobs: None, defines: Vec::new(), skip_unparsed: false`.
- `CollectOptions` has no `Default`: add
  `jobs: None, defines: Vec::new(), skip_unparsed: false`.
- Code that builds a `Report` must add `skipped: Vec::new()`.

These values keep the previous behaviour. Because this breaks the 0.x public
API, Cargo semver calls for 0.2.0 rather than 0.1.1. Choosing the version is an
owner decision; these notes do not change it.

### Behaviour

- **Directive conditions add no complexity**
  ([#51](https://github.com/RuiCostafrg/dotnet-crap/pull/51)): the `&&` and
  `||` in an `#if` or `#elif` condition inside a callable no longer add a
  decision. Affected callables lose one complexity point per such operator, so
  CRAP scores can only go down.
- **Conditional compilation fallback**
  ([#48](https://github.com/RuiCostafrg/dotnet-crap/pull/48)): a file that
  does not parse as written is parsed again with inactive `#if` branches
  blanked, using the symbols from the new repeatable `--define`. Files that
  already parsed score exactly as before. On sampled real repositories,
  unparseable files dropped from 66 to 0 (Newtonsoft.Json), 4 to 0 (Polly) and
  18 to 1 (Serilog). See [conditional compilation](PREPROCESSOR.md).
- **`--skip-unparsed`**
  ([#52](https://github.com/RuiCostafrg/dotnet-crap/pull/52)): opt-in, on
  `scan` and `collect`. Files that still fail with a syntax error are listed
  in text, JSON (`skipped_files`) and SARIF (`toolExecutionNotifications`)
  output instead of failing the run. Without the flag, or when nothing is
  skipped, output is byte-identical to before.
- **`--jobs N`** ([#46](https://github.com/RuiCostafrg/dotnet-crap/pull/46)):
  files are scanned in parallel, one worker per CPU by default. The report,
  its order and the error returned are identical for any value.

### Performance

From the hosted Benchmark workflow, with base and head measured in the same job:

- Single-pass syntax traversal
  ([#36](https://github.com/RuiCostafrg/dotnet-crap/pull/36)): scan wall time
  0.88–0.92× on Ubuntu.
- `collect` quick wins and single scan
  ([#38](https://github.com/RuiCostafrg/dotnet-crap/pull/38),
  [#45](https://github.com/RuiCostafrg/dotnet-crap/pull/45)): `collect` wall
  time 0.59–0.85×.
- Parallel scan ([#46](https://github.com/RuiCostafrg/dotnet-crap/pull/46)):
  about 2.5× faster on real repositories.
- Streaming Cobertura reader
  ([#47](https://github.com/RuiCostafrg/dotnet-crap/pull/47)): no DOM;
  `collect` peak memory 18–55% lower, at 1–6% more import time on Linux.
  Error messages are unchanged.

CI: the mutation gate now runs in four shards
([#49](https://github.com/RuiCostafrg/dotnet-crap/pull/49)) and the required
macOS checks run on macOS 15
([#53](https://github.com/RuiCostafrg/dotnet-crap/pull/53)).

## Pending integration

The owner chose .NET-tool-first distribution and removal of npm. The npm-removal
and NuGet publisher changes belong to separate PRs and are prerequisites for
0.1.1 qualification; these notes do not claim they are integrated or published.
The packaging script's version default and distribution documentation/workflows
are owned by those PRs and must agree with the final version.

Follow [release integration](RELEASE_INTEGRATION.md) in order: integrate the
reviewed prerequisites, qualify the exact final main SHA with CI, Security and
Distribution push runs, then seek fresh owner authorization for a new tag and
publication approvals. A successful preparation PR cannot qualify a later tree.
