# Configuration and file selection

`dotnet-crap` reads an optional versioned `dotnet-crap.json` and honors
`.gitignore` / `.ignore` files. Explicit command-line values always win.

## File format

```json
{
  "version": 1,
  "threshold": 10,
  "include": ["src/**"],
  "exclude": ["**/Migrations/**"],
  "respect_ignore_files": true,
  "require_coverage": false,
  "fail_on_empty": false,
  "skip_unparsed": false,
  "defines": ["NET10_0"]
}
```

| Field | Type | Meaning |
| --- | --- | --- |
| `version` | integer, required | Must be `1`; anything else is rejected. |
| `threshold` | number | Finite and `>= 0`. Scores strictly above it fail. |
| `include` | string list | Keep only files matching one of these globs. |
| `exclude` | string list | Drop files matching any of these globs. |
| `respect_ignore_files` | boolean | Honor `.gitignore` / `.ignore` files. |
| `require_coverage` | boolean | As `--require-coverage`. |
| `fail_on_empty` | boolean | As `--fail-on-empty`. |
| `skip_unparsed` | boolean | As `--skip-unparsed`. |
| `defines` | string list | As `--define`. |

Unknown fields are rejected. A missing, unreadable or invalid file exits with
code 2 and names the file in the error. `collect` reads the same file and uses
`include`, `exclude`, `respect_ignore_files`, `skip_unparsed` and `defines`.

## Discovery

- `--config PATH` loads that file; a missing file is an error.
- Otherwise `<root>/dotnet-crap.json` is loaded when it exists.
- `--no-config` loads nothing. It conflicts with `--config`.

## Precedence

Explicit command line, then config file, then built-in default.

| Setting | CLI | Config | Default |
| --- | --- | --- | --- |
| threshold | `--threshold N` | `threshold` | `10` |
| require coverage | `--require-coverage` / `--no-require-coverage` | `require_coverage` | off |
| fail on empty | `--fail-on-empty` / `--no-fail-on-empty` | `fail_on_empty` | off |
| skip unparsed | `--skip-unparsed` / `--no-skip-unparsed` | `skip_unparsed` | off |
| ignore files | `--no-ignore` | `respect_ignore_files` | on |
| include / exclude / defines | `--include` / `--exclude` / `--define` | lists | empty |

For each boolean pair the last flag on the command line wins. A list given on
the command line **replaces** the config list; it is not merged.

## Ignore files

- `.gitignore` and `.ignore` files are read at the scan root and below. Rules
  from ancestors of the root, global git ignore files and `.git/info/exclude`
  are never applied. A git repository is not required.
- When a subdirectory is selected (for example `src`), the root's ignore files
  still apply.
- A file named explicitly on the command line is selected even if an ignore
  file excludes it. Directories are walked with the rules.
- `bin`, `obj` and `.git` directories, symlinks, non-`.cs` files and
  `.g.cs` / `.generated.cs` / `.designer.cs` files are always skipped.
- `--no-ignore` (or `"respect_ignore_files": false`) disables ignore files.

## Include and exclude globs

Patterns use gitignore syntax and match the path relative to the root, with
`/` separators: `*`, `?`, `[a-z]`, `**`, a trailing `/` for directories, and a
leading `/` to anchor at the root. A file is kept when `include` is empty or it
matches an include pattern, and it matches no exclude pattern. They apply to
every selected file, including explicit file arguments. An invalid or blank
pattern is an input error (exit 2) naming the pattern.
