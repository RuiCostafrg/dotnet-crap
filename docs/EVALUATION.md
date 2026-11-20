# Evaluation on independent C# projects

dotnet-crap was run end to end on four independent open-source C# projects at
pinned revisions: a small library, a larger library, a multi-project web
application and a multi-project solution. This closes the PLAN step 9 item
"Evaluate 3–5 independent C# projects". The run is reproducible with
`.github/workflows/evaluate.yml` (see [Reproducing](#reproducing)).

Evidence: workflow run
[44578188397](https://github.com/RuiCostafrg/dotnet-crap/actions/runs/44578188397)
on 2026-11-19, `ubuntu-26.04` hosted runner, dotnet-crap built from PR #64 at
`6d6e7bfe` merged with `main` at `8c8181c` (after #65 and #66). Artifacts hold
every report, summary and log for 30 days.

## Method

For each project, `scripts/evaluate-projects.py`:

1. Fetches exactly the pinned commit and verifies `HEAD`.
2. Adds `coverlet.collector` 10.1.1 to test projects that lack it.
3. Runs `dotnet-crap collect --skip-unparsed`, which runs `dotnet test` with the
   XPlat Coverlet collector, then merges every Cobertura report into native
   coverage evidence.
4. Runs a separate JSON scan of that evidence with `--skip-unparsed` and the
   default threshold of 10.
5. Times both steps with GNU `time -v`.

Child processes get an allowlisted environment with no credentials.

| Project | Kind | Commit | What runs |
|---|---|---|---|
| [ardalis/GuardClauses](https://github.com/ardalis/GuardClauses) | library | `514f157e` | `GuardClauses.UnitTests` on net10.0 |
| [JoshClose/CsvHelper](https://github.com/JoshClose/CsvHelper) | library | `b4151b6d` | `CsvHelper.Tests` on net10.0 (1,355 tests) |
| [dotnet-architecture/eShopOnWeb](https://github.com/dotnet-architecture/eShopOnWeb) | web, 6 source projects | `4da82121` | the whole `eShopOnWeb.sln`: 4 test projects (xUnit and MSTest, 74 tests), 4 Cobertura reports merged |
| [ardalis/CleanArchitecture](https://github.com/ardalis/CleanArchitecture) | solution | `ecf2f4df` | `Clean.Architecture.UnitTests` only (see friction) |

## Results

Coverage evidence per callable falls into three buckets:

- **hit:** at least one covered line;
- **zero-hit:** instrumented lines, none hit;
- **absent:** no instrumented line in any report.

Absent coverage is scored as 0% covered, so CRAP = complexity² + complexity.

| Project | Files | Callables | Hit | Zero-hit | Absent | Max CRAP | CRAP > 10 | CRAP > 30 | Unparsed files |
|---|---|---|---|---|---|---|---|---|---|
| GuardClauses | 15 | 121 | 51 | 69 | 1 | 12 | 1 | 0 | 0 |
| CsvHelper | 95 | 1,204 | 686 | 51 | 467 | 870 | 170 | 49 | 11 |
| eShopOnWeb | 205 | 556 | 269 | 283 | 4 | 72 | 31 | 3 | 0 |
| CleanArchitecture | 72 | 151 | 4 | 12 | 135 | 30 | 12 | 0 | 0 |

| Project | Collect (tests included) | Collect peak RSS, largest process | Scan | Scan peak RSS |
|---|---|---|---|---|
| GuardClauses | 27.6 s | 293 MiB | 0.03 s | 9.7 MiB |
| CsvHelper | 213.7 s | 444 MiB | 0.3 s | 20.6 MiB |
| eShopOnWeb | 38.2 s | 425 MiB | 0.1 s | 10.7 MiB |
| CleanArchitecture | 16.0 s | 279 MiB | 0.03 s | 8.5 MiB |

The collect peak is the largest single process in the tree, usually the test
host. GNU `time -v` does not report the sum across processes. Timings are single
runs on a shared hosted runner and vary by tens of percent: across the four
runs made while building this evaluation, CsvHelper's collect took 188–298 s.

Highest-scoring callables:

| Project | Callable | File | Complexity | Coverage | CRAP |
|---|---|---|---|---|---|
| CsvHelper | `TerminalAsync` | `src/CsvHelper.Linq/Internal/Execution/QueryExecutor.cs` | 29 | absent | 870 |
| CsvHelper | `WriteField` | `src/CsvHelper/CsvWriter.cs` | 21 | absent | 462 |
| eShopOnWeb | `CreateUserInfo` | `src/Web/Controllers/UserController.cs` | 8 | 0% | 72 |
| eShopOnWeb | `EnableAuthenticator` | `src/Web/Controllers/ManageController.cs` | 6 | 0% | 42 |
| CleanArchitecture | `AddInfrastructureServices` | `src/Clean.Architecture.Infrastructure/InfrastructureServiceExtensions.cs` | 5 | absent | 30 |
| GuardClauses | `ExpressionAsync` | `src/GuardClauses/GuardAgainstExpressionExtensions.cs` | 3 | 0% | 12 |

## Findings

### Scanning is fast; the test run dominates

Scanning the largest corpus, 1,204 callables in 95 files, took 0.3 s and 21 MiB.
More than 99% of each project's evaluation time is building and running its own
tests. dotnet-crap adds no meaningful cost to an existing coverage job.

### Coverage mapping

- **Fixed: entries outside the repository failed the whole import (#65, #66).**
  When one test project references another, Coverlet instruments the program file
  that Microsoft.NET.Test.Sdk compiles from the NuGet package cache. It names that
  file through a `<source>` prefix in the cache. dotnet-crap rejected the whole
  report, so eShopOnWeb could not be evaluated: all 74 tests passed, then
  `collect` failed. Such a file can never be a scanned source, so it is now
  ignored. Directories, missing files and ambiguous in-root matches still fail.
- **Merging four reports from four test projects worked as designed.** That is
  eShopOnWeb's xUnit and MSTest projects, including WebApplicationFactory
  functional tests. Only 4 of 556 callables have no evidence.
- **Absent evidence usually reflects the test setup, not mapping.** CsvHelper's
  467 absent callables are mostly `CsvHelper.Linq`, whose test project was not
  run, plus generator and polyfill code. CleanArchitecture's 135 are code
  exercised only by its Docker-based integration and functional tests.
  `--require-coverage` would have flagged both cases instead of scoring the gaps
  as uncovered.

### Parser limitations

CsvHelper has 11 files, of 106 selected, that tree-sitter-c-sharp 0.23.5 cannot
parse. With `--skip-unparsed` they are listed and not scored. Without it, the
whole scan fails. The constructs:

| Construct | Files | Parses with grammar 0.24.0? |
|---|---|---|
| Assignment to a ref-returning call, `Unsafe.Add(ref x, i) = v;` | 7 (`Internal/*`, `CsvReader.*`, `CsvSeparatorStream.Scan`) | No |
| `#if` splitting a `where` clause, switch arm or initializer | 4, together with the row above | Not needed: the preprocessor fallback already handles these |
| `partial` used as a local variable name | `Generators/RecordParser.cs` | No |
| `and` used as a pattern designation | `Linq/Internal/Planning/QueryRewriter.cs` | No |
| C# 14 `extension(T) { … }` block | `Polyfills/Extensions.cs` | Yes |

So upgrading to tree-sitter-c-sharp 0.24.0, released 2026-11-18, would recover
only one file. The ref-return assignment is a grammar gap in both versions and
needs an upstream fix: `lvalue_expression` does not include
`invocation_expression`, so no call can be an assignment target. Reported as
[tree-sitter-c-sharp#484](https://github.com/tree-sitter/tree-sitter-c-sharp/issues/484).

0.24.0 also changes some parse shapes, so some scores would change after an
upgrade. For example, `(x) && y` now parses as a binary expression instead of a
cast. Adopt it in a separate change with scoring tests, once the release has
matured.

### Integration friction

- **Coverlet must be referenced.** eShopOnWeb's three xUnit test projects did not
  reference `coverlet.collector`, so the harness added it. The other projects'
  test projects already referenced it.
- **The default threshold of 10 fails every project.** It failed even
  GuardClauses, whose callables are 99% instrumented. Real adoption needs an
  explicit `--threshold`, or a baseline, which is not implemented yet.
- **Multi-targeting and Docker-based tests limit what a single `dotnet test`
  covers.**
  - CsvHelper targets net10.0, net8.0 and net48. Running all targets would write
    more reports per project, and net48 does not build on Linux, so the harness
    selects net10.0.
  - CleanArchitecture's solution includes Testcontainers SQL Server tests, so its
    unit-test project is tested directly.
  - Both choices are deliberate setup decisions, not dotnet-crap limitations.
- **Reported names:** lambdas appear by their syntax kind (`lambda_expression`)
  plus file and line, which is less readable in summaries than method names.
- **No changes to the projects** were needed beyond adding Coverlet, and no test
  failed.

## Reproducing

```sh
gh workflow run evaluate.yml --repo RuiCostafrg/dotnet-crap --ref main
```

The `report` job writes the summary table to the run summary. It fails if any
project is missing or errored. Edit `evaluation/projects.json` to add a project
or move a pin. The harness is described in `scripts/evaluate-projects.py`.
