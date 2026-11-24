# Rust library API

The `dotnet_crap` library is the scanning and rendering core behind the
`dotnet-crap` CLI. Rust code can scan C# sources and render text, JSON, or SARIF
reports without starting the CLI process. The source crate is version 0.2.0
(unreleased) and this repository has no crates.io publishing workflow, so consume
it from a local path or a git revision. No semantic-versioning guarantee is made
for this surface yet: it is outside the 1.x stability guarantee in the
[compatibility policy](COMPATIBILITY.md).

The documented surface is:

```rust
dotnet_crap::{ScanOptions, scan, ReportFormat, Error, ErrorKind, Result}
```

plus the `ScanResult`, `Report`, `Item`, and `SkippedFile` types and
`output::write_report` described below.

## Consuming the package

```toml
[dependencies]
dotnet-crap = { path = "../dotnet-crap" }
# Or pin a reviewed revision:
# dotnet-crap = { git = "https://github.com/RuiCostafrg/dotnet-crap", rev = "<full commit SHA>" }
```

The Rust crate name is `dotnet_crap`. Building requires stable Rust and a C
compiler, because the C# parser (Tree-sitter) is compiled from source. Use the
CLI, or a prebuilt native executable, when Rust is not available.

## Scanning

```rust
use dotnet_crap::{scan, terminal_safe, ReportFormat, ScanOptions};

fn run() -> dotnet_crap::Result<()> {
    let result = scan(&ScanOptions {
        root: "src".into(),
        threshold: 10.0,
        ..ScanOptions::default()
    })?;

    if result.report().failed {
        eprintln!("CRAP gate failed");
    }
    let json = result.render(ReportFormat::Json)?;
    result.verify_sources()?;
    print!("{json}");
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {}", terminal_safe(&format!("{error:#}")));
        std::process::exit(2);
    }
}
```

`ScanOptions::default()` matches the CLI defaults: `root` is `.`, `threshold` is
`10.0`, there is no coverage input, neither `require_coverage` nor
`fail_on_empty` is set, `paths` is empty (which selects the whole root),
`jobs` is `None` (one worker per available CPU), `defines` is empty (no
conditional compilation symbol), and `skip_unparsed` is `false` (a syntax error
fails the scan).

### `ScanOptions`

| Field | Meaning |
| --- | --- |
| `root: PathBuf` | Directory that contains the selected sources. It is canonicalized by `scan`. Must exist and be a directory. |
| `threshold: f64` | Must be finite and nonnegative. A callable fails only when its CRAP score is strictly greater than this value. |
| `coverage: Option<PathBuf>` | Native coverage JSON (version 1). Read relative to the process working directory, not `root`. |
| `require_coverage: bool` | Makes any callable without coverage evidence a policy failure. |
| `fail_on_empty: bool` | Makes a scan that finds no callables a policy failure, including selected files without callable bodies. |
| `paths: Vec<PathBuf>` | Files or directories to select. Relative values resolve against `root`. Absolute values must be inside `root`. Empty selects `root`. |
| `jobs: Option<NonZeroUsize>` | Worker threads for scanning and source verification, as the CLI's `--jobs`. `None` uses `std::thread::available_parallelism()` (1 if unknown). The value never changes the report, its order, or which error is returned. `Some(1)` scans on the calling thread without spawning. |
| `defines: Vec<String>` | Conditional compilation symbols, as the CLI's `--define`. Used only for a file that does not parse as written: its `#if` groups are resolved with these symbols and it is parsed again, with line numbers and the source hash unchanged. Each must match `[A-Za-z_][A-Za-z0-9_]*`, be at most 128 characters long and not be `true` or `false` in any letter case; otherwise `scan` returns `InvalidInput`, after the root, coverage and path checks. A non-ASCII symbol is rejected, and a directive in a file with a non-ASCII or over-long symbol makes the fallback decline, because no Unicode identifier table is used. A file that parses only after resolution is then subject to the normal analysis limits, so it may report `ResourceLimit` instead of `Syntax`; while it is resolved, its text is held twice. See [conditional compilation](PREPROCESSOR.md). |
| `skip_unparsed: bool` | Skip a selected file whose final failure is a `Syntax` error instead of returning `Err`, as the CLI's `--skip-unparsed`. A skipped file contributes no `Item`s and is listed in `Report::skipped` in selection order. Only syntax failures are skipped: I/O, decoding, resource-limit and every other error still fail the scan. Coverage entries for a skipped file are bound like those of a selected file with no callables. Defaults to `false`. |
| `baseline: Option<PathBuf>` | A baseline file, resolved against the working directory. The score policy then fails only for callables that are new or worse; `require_coverage` and `fail_on_empty` are unaffected. Its `threshold` must equal this scan's, else `InvalidInput`; a malformed, unknown-version or `.cs` baseline is `InvalidInput` naming the file. The path becomes an `input_paths` entry, so reports cannot overwrite it. Defaults to `None`. See [baselines](BASELINES.md). |
| `baseline_moves: Vec<(String, String)>` | Root-relative `(old, new)` file pairs, as `--baseline-move`. Requires `baseline`. |
| `changed_since: Option<String>` | A Git revision, as `--changed-since`: keep only selected files that differ from its merge base with `HEAD`. Requires `git`. |

### Baselines

`dotnet_crap::baseline` exposes `Baseline`, `Entry`, `read`, `compare` (a pure
function over entries and items) and `parse_move`. With `ScanOptions::baseline`
set, failing `Item`s gain `baseline: Option<BaselineState>` and
`Report::baseline` holds the `BaselineSummary` counts. `ScanResult` adds:

- `write_baseline(path, overwrite) -> Result<usize>`: write the failing callables
  atomically and return the entry count. An existing file is an `Output` error
  unless `overwrite` is set; the path cannot be a C# source, a symbolic link or an
  input of the scan.
- `accept_scores()`: treat score violations as accepted, so `report().failed`
  reflects only the completeness policies (what the CLI does for `--write-baseline`).
- `completeness_failed() -> bool`: whether `require_coverage` or `fail_on_empty` fails.

### Worker count and determinism

Files are scanned on up to `jobs` threads (never more than the number of
selected files), each with its own C# parser. Results are assembled in the
selected-file order, so the report, the JSON, text and SARIF bytes, and the
source hashes are identical for every worker count. When several files fail,
the error is the one the first failing file in selection order produces, with
the same kind and message as a single-threaded scan; files not yet started are
skipped. Larger files are started first so one large file does not run alone
at the end. To bound memory, a file is read only while the source bytes being
read, decoded and parsed at once stay within `max(1 MiB, the size of that
file)`: a file above 1 MiB is processed alone, so a tree of very large files
holds at most one file's source and syntax tree at a time, as a single-threaded
scan does. The admitted size is the file's size just before it is read.
Per-file results are kept until the report is assembled, as before. `verify_sources` uses the scan's worker count
and also reports the first changed file in selection order; it only hashes, so
it holds at most one file's bytes per worker.

## Results, policy, and errors

`scan` distinguishes input errors from successful scans and their policy decisions:

- **`Err(dotnet_crap::Error)`**: the scan could not produce a trustworthy result. This
  covers an invalid threshold, a missing or non-directory root, a path outside
  `root`, an unreadable file, non-UTF-8 or malformed UTF-16 source, a C# syntax
  error in any selected file (unless `skip_unparsed` is set), invalid or stale
  coverage, a coverage line outside its file, or a source that changed during
  the scan. The CLI reports these as exit **2**.
- **`Ok(ScanResult)` with `report().failed == true`**: the scan succeeded but a
  policy gate failed (score above threshold, missing required coverage, or
  a scan with no callables and `fail_on_empty`). The CLI reports this as exit
  **1**. The result is still returned, so the report can be rendered.
- **`Ok(ScanResult)` with `report().failed == false`**: passing. The CLI exits **0**.

The library does not map outcomes to process exit codes. The bundled example
exits 2 on errors but leaves policy failures as successful process exits (see below).

### Errors

Every fallible operation (`scan`, `ScanResult::verify_sources`,
`ScanResult::render`, `output::write_report`, `collector::collect` and
`cobertura::parse_reports`) returns `dotnet_crap::Result<T>`, an alias for
`Result<T, dotnet_crap::Error>`. `Error` implements `std::error::Error`, `Send`,
`Sync`, `Debug` and `Display`, so `?` converts it into `anyhow::Error` or
`Box<dyn Error>`.

Match on `error.kind()`, never on message text. `error.get_ref()` exposes the
wrapped underlying error, such as a `std::io::Error`, for downcasting. `ErrorKind` is
`#[non_exhaustive]`:

| Kind | Cause |
| --- | --- |
| `InvalidInput` | Invalid threshold, root, input path, conditional compilation symbol or collection options. |
| `Io` | Reading or walking the source tree, a coverage file, a Cobertura report or the results directory failed. |
| `Source` | A source is not valid UTF-8/UTF-16, its path cannot be a native coverage key, or the C# parser could not be initialized. |
| `Syntax` | A selected file is not valid C#. |
| `SourceChanged` | Selected source bytes changed after the scan read them. |
| `Coverage` | Native coverage JSON is invalid, unsupported, stale or outside its file. |
| `Cobertura` | A Cobertura report was rejected by the strict importer. |
| `ResourceLimit` | A syntax-tree or callable traversal budget, or complexity, overflowed. |
| `Collection` | The trusted test command could not start, failed, timed out or produced the wrong reports. |
| `Output` | A report or coverage destination was rejected or could not be written. |
| `Render` | A report could not be serialized, such as SARIF for a non-UTF-8 root. |

`Display` writes the outermost message. The alternate form `{:#}` appends each
underlying cause after `": "`, exactly like `anyhow`'s `{:#}`, so CLI diagnostics
are unchanged. Message wording is diagnostic and may change; the kind is the
contract. Errors from operations that wrap others (for example
`collector::collect` running `scan`) keep the inner operation's kind.

### `Report` and `Item`

These fields are public and match the current JSON output.

`Report`:

| Field | Type | Meaning |
| --- | --- | --- |
| `threshold` | `f64` | The threshold used by the scan. |
| `files_scanned` | `usize` | Number of selected `.cs` files. |
| `skipped` | `Vec<SkippedFile>` | Selected sources skipped because they did not parse, in selection order. Serialized as `skipped_files`, and omitted from JSON when empty. Empty unless `skip_unparsed` is set and a file failed. |
| `coverage_complete` | `bool` | `true` when every callable has coverage evidence. |
| `functions` | `Vec<Item>` | Callable records in deterministic order. |
| `failed` | `bool` | The policy decision described above. |
| `baseline` | `Option<BaselineSummary>` | `new`, `worse`, `unchanged`, `improved` and `resolved` counts; `None` and omitted from JSON without a baseline. |

`SkippedFile`:

| Field | Type | Meaning |
| --- | --- | --- |
| `path` | `String` | Path relative to `root`, with `/` separators, as for `Item::file`. |
| `message` | `String` | The syntax error message that would have failed the scan. |

`Item`:

| Field | Type | Meaning |
| --- | --- | --- |
| `file` | `String` | Path relative to `root`, with `/` separators. |
| `name` | `String` | Name built from syntax. Not a resolved overload identity. |
| `kind` | `String` | Syntax node kind, such as `method_declaration`. |
| `line`, `column`, `end_line` | `usize` | One-based lines and UTF-8 byte column in the source. |
| `complexity` | `u32` | Syntax-based cyclomatic complexity. |
| `coverage` | `Option<f64>` | Fraction from 0 to 1, or `None` when no coverage points fall in the callable. |
| `crap` | `f64` | `complexity² × (1 − coverage)³ + complexity`, with absent coverage as 0. |
| `scope` | `String` | Enclosing namespaces, types, members and callables joined with `.`. |
| `signature` | `String` | Name plus parameter types, such as `Parse(string,int)`. |
| `baseline` | `Option<BaselineState>` | Outcome of a failing callable against a baseline; omitted from JSON when `None`. |

`ScanResult` has these accessors:

- `report() -> &Report`
- `root() -> &Path`: the canonical root.
- `input_paths() -> &[PathBuf]`: the selected source files, then the coverage
  path exactly as supplied, if any. Source entries are absolute paths produced
  by the walk; do not rely on their exact form beyond "selected inputs".
- `verify_sources() -> dotnet_crap::Result<()>`: see below.
- `render(ReportFormat) -> dotnet_crap::Result<String>`: see below.

## Rendering

`ReportFormat` is `Text`, `Json`, or `Sarif`.

- `render` only builds a `String`. It does not write files, and it does not
  print.
- Each returned string ends with a newline. The CLI writes exactly these bytes
  to stdout or to `--output`.
- `Json` is the CLI's existing shape. Field names and order are unchanged,
  except that a non-empty skipped list adds `skipped_files` after
  `files_scanned`.
- `Sarif` returns an error if the canonical root cannot be represented as UTF-8.
  It emits SARIF 2.1.0. Its rule identifiers and location rules are described in
  [SARIF output](SARIF.md).
- `Text` escapes control characters in file names and callable names. A
  non-empty skipped list follows the callable lines as `skipped N unparsed
  file(s):` and one indented `path: message` line per file. JSON and SARIF carry
  the raw strings, so escape them in your own sinks if needed.
- `render` uses the `require_coverage` and `fail_on_empty` values captured by
  `scan`, so SARIF and text diagnostics match the scan's policy.

`render` does not call `verify_sources`. Verify before you use the output in a
decision or save it (see the next section).

## Sources, snapshots, and verification

`scan` reads each selected file once, hashes its bytes with SHA-256, and parses
those same bytes. Before returning, it calls `verify_sources` to re-read each
selected file and compare hashes.

`verify_sources` detects a changed or unreadable selected source file at the time
it is called. It does not:

- check the coverage file, which is read once during `scan`;
- check files that were not selected;
- notice files added after the scan;
- prevent a change that happens after it returns.

If you write a report based on a scan, call `verify_sources` immediately before
the write and again after staging the data, as the CLI does. A single check is
not a guarantee against a later edit.

## Writing report files

`render` is not a file writer. Use `output::write_report(&result, &rendered, path)`,
the same function the CLI uses for `--output`. It:

- refuses to write output over a `.cs` file;
- refuses a symbolic link as output;
- refuses output that is the same file as a selected source, the coverage input,
  or any `.cs` file under the root;
- stages the report in a temporary file in the destination directory, verifies
  sources again, and then persists it over `path`.

Call `verify_sources` after rendering and before `write_report`, as the CLI does;
`write_report` verifies again after staging. Exit codes remain the caller's choice.

The alias check compares `path` with `input_paths()`, which holds the coverage
path exactly as supplied. If the coverage path is relative, call `write_report`
from the same working directory as `scan`, or pass an absolute coverage path.
Otherwise a relative coverage input cannot be recognized as an alias.

## Coverage input

The native coverage format is documented in [examples/COVERAGE.md](../examples/COVERAGE.md).
Key points for library callers:

- `coverage` is read and validated during `scan`: version 1 only, root-relative
  slash paths, SHA-256 of the exact source bytes, canonical line numbers, and no
  duplicate keys. Stale hashes are errors.
- Each supplied point inside a callable contributes equally. Producers must
  supply zero-hit executable points, because omitted points are not counted.
- `require_coverage` requires some evidence for each callable. It does not prove
  that every statement is covered.
- Nested callables are scored separately. Callables that share a source line are
  ambiguous and are not resolved.
- `scan` never runs tests. Explicit process collection is available separately via
  `collector::{CollectOptions, collect}`; see [Coverlet collection](COVERLET.md).
  `CollectOptions::jobs`, `CollectOptions::defines` and
  `CollectOptions::skip_unparsed` have the same meaning as the `ScanOptions`
  fields.
  `cobertura::parse_reports` imports line evidence without starting a process.

## Source selection and parsing limits

- Only `.cs` files are selected. Files ending in `.g.cs`, `.generated.cs`, or
  `.designer.cs` are skipped. Directories named `bin`, `obj`, and `.git` are
  skipped, as are symbolic links found during the walk.
- Any syntax error in a selected file fails the whole scan, unless
  `skip_unparsed` is set; then that file is listed in `Report::skipped` and
  contributes no callables, while every non-syntax error still fails. A file
  that fails to parse as written is parsed once more with conditional
  compilation resolved; see [conditional compilation](PREPROCESSOR.md).
- Syntax traversal budgets do not bound aggregate coverage-point visits. Dense
  coverage across deeply nested callables can repeat work; profiling and
  coverage-range optimization remain in the performance backlog.
- Analysis works on the source as written. Compilation symbols apply only to
  files that do not parse as written. There is no semantic binding. See
  [callable support and limits](CSHARP_CALLABLES.md).
- Scoring is a syntax convention. It does not claim equality with Roslyn or an
  IDE metric.

## Example command

From the repository root:

```sh
cargo run --example scan -- <root>
```

`examples/scan.rs` scans `<root>` with default options and prints the text
report after rendering and re-verifying source bytes. It escapes error diagnostics
and exits 2 on input or I/O errors. It does not write report files or set an exit code from
`report().failed`. Use the CLI when you need the gate exit codes.

## Terminal diagnostics helper

`terminal_safe(&str) -> String` escapes control characters for terminal sinks,
and is also used by CLI error diagnostics. It does not perform path validation.
It is public and shares the API's current lack of a stability guarantee.

## Not in this API

- Git or GitHub pull-request selection, configuration files, and baselines.
- XML or HTML output.
- Microsoft coverage and OpenCover adapters (the explicit Coverlet collector is
  available separately from scanning).
- Semantic (Roslyn) analysis.
- Any stability promise beyond this document.
