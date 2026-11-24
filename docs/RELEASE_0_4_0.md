# 0.4.0 release notes

Minor release on top of
[v0.3.0](https://github.com/RuiCostafrg/dotnet-crap/releases/tag/v0.3.0). It
completes the features planned for 1.0: report views, changed-file scans,
partial scoring of unparsable files, and coverage gaps that count against the
score. The CRAP formula and complexity rules are unchanged.

## Behavior change: executable lines without coverage data count as uncovered

([#84](https://github.com/RuiCostafrg/dotnet-crap/pull/84),
[rule](../examples/COVERAGE.md)) The scanner now derives a callable's
executable lines (expression, `return`, `throw`, `if`, `switch` and similar
statements, initialized non-constant locals, arrow bodies). A derived line with
no point in the coverage file counts as an uncovered point, and the callable
lists it in `unevidenced_lines`. `--require-coverage` fails while any callable
lists one.

Coverlet, `Microsoft.Testing.Extensions.CodeCoverage` and `dotnet-coverage`
record zero-hit lines, so their reports are unaffected: on the four evaluation
projects (eShopOnWeb, CleanArchitecture, GuardClauses, CsvHelper) no line was
flagged and no score changed. A hand-written or filtered coverage file that
leaves out lines it did not execute now scores lower, and may fail the gate
where 0.3.0 passed. Include zero-hit points to keep the old result.

Nothing is derived for a callable with no coverage points (its coverage stays
absent), for a file with a `#line` directive, for code inside `#if`…`#endif`,
or for a file scored from conditional-compilation-resolved text.

## Fix: baselines no longer resolve entries for files a scan skipped

([#82](https://github.com/RuiCostafrg/dotnet-crap/pull/82),
[guide](BASELINES.md)) In 0.3.0, a scan of part of the tree (explicit paths or
`--exclude`) counted baseline entries for the files it did not scan as
`resolved`, and SARIF reported them as absent. An unmatched entry is now
resolved only when its file was scanned or no longer exists, so `resolved`
counts can be lower than 0.3.0 reported for the same scan.

## New

- **Text report views**
  ([#81](https://github.com/RuiCostafrg/dotnet-crap/pull/81)).
  `--sort location|crap`, `--top N` (which on its own sorts by score) and
  `--failing-only` choose what the text report lists, followed by a
  `showing X of N callables (K above threshold T)` footer. They never change
  the exit code, and they are rejected with `--format json` or `sarif`, whose
  reports always hold every callable.
- **`--changed-since REV`**
  ([#82](https://github.com/RuiCostafrg/dotnet-crap/pull/82)). Scores only the
  selected files that differ from the merge base of `REV` and `HEAD`,
  including staged, unstaged and untracked changes; for example
  `--changed-since origin/main` in a pull request. It needs `git` and the
  history of both revisions (for `actions/checkout`, `fetch-depth: 0`). It
  cannot be combined with `--write-baseline`.
- **`--partial-unparsed`**
  ([#83](https://github.com/RuiCostafrg/dotnet-crap/pull/83)). Instead of
  failing on, or skipping, a file that does not parse, scores the callables
  whose syntax is intact and reports how many it left unscored. A callable is
  dropped when it, or a callable enclosing it, contains a syntax error or a
  misplaced conditional directive. Also available as the config key
  `"partial_unparsed"`; `--no-partial-unparsed` turns it off.

## Output changes (additive)

- JSON callables may carry `unevidenced_lines`; the report may carry
  `partial_files` (`path`, `message`, `unscored`). Both are omitted when
  empty, and the [report schema](../schemas/report.v1.schema.json) describes them.
- SARIF: `DNCRAP002` (missing coverage) results carry `unevidencedLines`;
  partially scored files are reported as tool execution notifications.
- Text: a callable's line may end in `[no coverage data for lines …]`, and a
  `partially scored N unparsed file(s)` section lists partial files.

## Distribution

- Supported runtimes remain .NET 10 and .NET 11. Channels are unchanged:
  NuGet.org, Homebrew, the shell installer, the GitHub Action and GitHub
  release assets.
- No new dependencies. `--changed-since` runs the `git` already on `PATH`.
