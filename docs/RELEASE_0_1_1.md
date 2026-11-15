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
