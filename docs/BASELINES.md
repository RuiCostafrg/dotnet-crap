# Regression baselines

A baseline lets a team adopt `dotnet-crap` on code that already has violations:
a committed file records today's failing callables, and later runs fail only
for violations that are **new** or **worse**.

## Workflow

1. Write the baseline once and commit it:

   ```sh
   dotnet-crap --write-baseline dotnet-crap-baseline.json
   git add dotnet-crap-baseline.json
   ```

   Writing exits 0 even though callables fail the threshold. It still exits 1
   when `--require-coverage` or `--fail-on-empty` fails.
2. Compare in CI:

   ```sh
   dotnet-crap --baseline dotnet-crap-baseline.json
   ```

   The run fails (exit 1) if any failing callable is `new` or `worse`.
   Existing, `unchanged` and `improved` violations pass.
3. Refresh it deliberately when the team accepts a change, for example after
   paying down debt, with `--write-baseline PATH --overwrite-baseline`. A
   baseline that already exists is never replaced without `--overwrite-baseline`.

Or set `"baseline": "dotnet-crap-baseline.json"` in `dotnet-crap.json` so every
run compares; see [CONFIG.md](CONFIG.md).

## What a run reports

Each failing callable is classified:

| State | Meaning |
| --- | --- |
| `new` | No baseline entry matches it. Fails the run. |
| `worse` | Matched, and its CRAP score rose (compared at two decimals). Fails the run. |
| `unchanged` | Matched with an equal score at two decimals. |
| `improved` | Matched with a lower score that still exceeds the threshold. |

Baseline entries that match no failing callable are `resolved`: fixed, deleted,
or now at or under the threshold. They never fail a run. Only entries whose file
was scanned, or no longer exists, can be resolved: when a scan covers part of the
tree (explicit paths, `--exclude`, `--changed-since`), entries for the files it
left out are out of scope and are neither counted nor reported as absent in SARIF.
`--write-baseline` cannot be combined with `--changed-since`, because a baseline
records the whole selection.

Text output suffixes failing lines with ` [new]`, ` [worse, was 12.00]`,
` [unchanged]` or ` [improved, was 15.00]` and ends with
`baseline: N new, N worse, N unchanged, N improved, N resolved`. JSON adds
`"baseline": "new"|"worse"|"unchanged"|"improved"` to each failing item and a
top-level `"baseline"` object with the five counts; both are absent without a
baseline. SARIF sets `baselineState` (see [SARIF.md](SARIF.md)).

Completeness policies are never suppressed: `--require-coverage` and
`--fail-on-empty` fail a run whatever the baseline accepts.

## File format

```json
{
  "version": 1,
  "threshold": 10.0,
  "entries": [
    {
      "file": "src/A.cs",
      "kind": "method_declaration",
      "scope": "N.A",
      "signature": "M(int)",
      "occurrence": 0,
      "fingerprint": "9f86d0...",
      "line": 12,
      "complexity": 7,
      "coverage": 0.5,
      "crap": 13.2
    }
  ]
}
```

- `entries` holds only callables that exceed the threshold when it was written,
  ordered by file, then line, then signature, so rewriting unchanged code gives
  identical bytes.
- `file` is root-relative with `/` separators. `kind` is the syntax node kind.
- `scope` joins the enclosing namespaces, types (with generic arity, as in
  ``Box`1``), accessor-bearing members (properties, indexers, events) and, for
  local functions and lambdas, the enclosing callable's name, with `.`.
- `signature` is the name plus parameter types, such as `Parse(string,int)`.
- `occurrence` numbers callables of one file with the same kind, scope and
  signature in scan order, starting at 0.
- `fingerprint` is the hex SHA-256 over the callable's leaf tokens, in order,
  separated by single spaces, ignoring comments and whitespace. The callable's
  own name is not hashed, so renaming a callable keeps its fingerprint. A
  nested callable contributes its own fingerprint in place of its tokens.
- `line`, `complexity`, `coverage` and `crap` are informational, except `crap`,
  which is the score later runs compare with.
- Unknown fields, another `version`, or a `threshold` that differs from the
  scan's effective threshold are input errors (exit 2). The baseline cannot be a
  C# source file or the report's `--output` file.

## Matching

Matching uses only syntax, never source line numbers, so it survives edits above
a callable, reformatting, comments, renamed files, moved callables and moved
files. Each step considers only entries and callables still unmatched:

1. **Move.** `--baseline-move OLD=NEW` (repeatable) rewrites the `file` of
   entries from `OLD` to `NEW` first. Paths are root-relative and match exactly.
2. **Exact.** Same file, kind, scope, signature and occurrence.
3. **Fingerprint.** Same kind and fingerprint, when that fingerprint is held by
   exactly one unmatched entry and exactly one unmatched current callable. This
   follows a moved file or a renamed or relocated callable with an unchanged
   body.
4. **Identity across files.** Same kind, scope, signature and occurrence in
   different files, when exactly one unmatched entry and one unmatched callable
   have that identity. This follows a moved file whose body also changed.

### Conservative limits

Anything else is unmatched, so an ambiguity never hides a violation. A callable
that was renamed **and** edited has no matching fingerprint and identity, so it
counts as `new` (and its old entry as `resolved`). Several identical bodies, or
several callables with one scope and signature in different files, cannot be
told apart once they move; pair them with `--baseline-move`. A `--baseline-move`
whose `OLD` matches no entry has no effect.

## Flags

| Flag | Meaning |
| --- | --- |
| `--baseline PATH` | Compare with this baseline. Overrides the config key. |
| `--no-baseline` | Ignore a baseline configured in `dotnet-crap.json`. |
| `--write-baseline PATH` | Run the scan, write the baseline atomically and exit 0 unless a completeness policy fails. The report is rendered as usual. Conflicts with `--baseline`. A configured baseline is ignored. |
| `--overwrite-baseline` | Allow `--write-baseline` to replace an existing file. |
| `--baseline-move OLD=NEW` | Match baseline entries of `OLD` against `NEW`. Requires a baseline. |

The config key is `"baseline": "path"`, relative to the config file's directory.
`--baseline` overrides it and `--no-baseline` disables it.

The baseline path must not be the `--output` file, and `--write-baseline` and
`--output` must name different files. Library users get the same through
`ScanOptions::baseline`, `ScanOptions::baseline_moves` and
`ScanResult::write_baseline`; see [RUST_API.md](RUST_API.md).
