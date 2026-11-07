# MSTest VSTest collector fixture

This folder is a standalone MSTest test project that references
`../Alpha.Library/Alpha.Library.csproj`, the same measured library used by
`Alpha.Tests`. It is included in `CoverletFixtures.sln` and the Rust live harness alongside
xUnit and NUnit.

## Pins

| Package | Version | Role |
| --- | --- | --- |
| `Microsoft.NET.Test.Sdk` | 17.12.0 | VSTest host (`TestHost` 17.12.0 resolved) |
| `coverlet.collector` | 6.0.4 | Coverlet data collector (`PrivateAssets=all`) |
| `MSTest.TestFramework` | 4.5.2 | MSTest attributes and `Assert` |
| `MSTest.TestAdapter` | 4.5.2 | MSTest adapter for VSTest (pulls `MSTest.Analyzers` 4.5.2) |

Target framework: `net10.0`. Version 4.5.2 was the highest stable
`MSTest.TestFramework` and `MSTest.TestAdapter` on the NuGet flat-container
index when this fixture was written (2026-11-07). The `MSTest.TestAdapter` 4.5.2
nuspec also declares `Microsoft.Testing.Platform.MSBuild` 2.5.2 and
`Microsoft.TestPlatform.ObjectModel` 18.10.1; the restore resolved these
transitively. The live run below shows VSTest mode (`VSTest version 18.0.2-dev`),
not Microsoft Testing Platform mode. This README makes no MTP claim.

## Test

`MSTestCoverageTests.Exercises_one_branch_of_partial_method_and_both_full_method_branches`
makes the same calls as `Alpha.Tests`:

- `Gauge.Choose(1)` equals 1
- `Gauge.Full(1)` equals 1
- `Gauge.Full(0)` equals 0

`Gauge.Uncovered` is never called, so it stays at 0 of 1 line covered.

## Command

The evidence was produced on a temporary copy of `Alpha.Library` and
`MSTest.Tests` (sources only, no `bin`/`obj`), so no build output lands in the
shared fixture tree. The command was:

```sh
dotnet test MSTest.Tests.csproj --collect:"XPlat Code Coverage" \
  --results-directory <fresh-temp-path> \
  -- DataCollectionRunSettings.DataCollectors.DataCollector.Configuration.Format=cobertura
```

## Live evidence

Host: .NET SDK 10.0.109, runtime Microsoft.NETCore.App 10.0.9, Linux ARM64.

Runner output (`evidence/mstest-run1-dotnet-test.log`, exit code 0):

```text
VSTest version 18.0.2-dev (arm64)

Starting test execution, please wait...
A total of 1 test files matched the specified pattern.

Passed!  - Failed:     0, Passed:     1, Skipped:     0, Total:     1, Duration: 50 ms - MSTest.Tests.dll (net10.0)

Attachments:
  .../results/6204b0fe-2dcf-4bf6-ba68-90b69f3305ee/coverage.cobertura.xml
```

A second run with `--no-build` (`evidence/mstest-run2-no-build-dotnet-test.log`)
passed the same way and produced an identical per-method result set.

Cobertura evidence (`evidence/mstest-run1.coverage.cobertura.xml`, copied from
the fresh results directory):

| Method (file) | Line hits | Line rate | Complexity | Expected CRAP |
| --- | --- | ---: | ---: | ---: |
| `Choose` (`Gauge.First.cs`) | 6=1, 7=1, 8=1, 9=0, 10=1 | 0.8 (4/5) | 2 | 2.032 |
| `Full` (`Gauge.First.cs`) | 13=2, 14=2, 15=1, 16=1, 17=2 | 1 (5/5) | 2 | 2 |
| `Uncovered` (`Gauge.Second.cs`) | 5=0 | 0 (0/1) | 1 | 2 |

The CRAP values use `M² × (1 − coverage)³ + M`:

- `Choose`: `2² × 0.2³ + 2 = 2.032`
- `Full`: `2² × 0³ + 2 = 2`
- `Uncovered`: `1² × 1³ + 1 = 2`

These match the table in `../README.md`. Hit counts and the Choose 4/5, Full 5/5
and Uncovered 0/1 results match the expected values. The Cobertura report
also shows `Gauge` split across `Gauge.First.cs` and `Gauge.Second.cs` as separate
class records, as in the Alpha fixture.

Resolved versions were verified from `obj/project.assets.json` of the temporary
run: `MSTest.TestAdapter/4.5.2`, `MSTest.TestFramework/4.5.2`,
`MSTest.Analyzers/4.5.2`, `Microsoft.NET.Test.Sdk/17.12.0`,
`coverlet.collector/6.0.4`, `Microsoft.TestPlatform.TestHost/17.12.0`.

## Files

- `MSTest.Tests.csproj`, `MSTestCoverageTests.cs`: the fixture.
- `evidence/mstest-run1.coverage.cobertura.xml`: the actual Cobertura output.
  Its `<sources>` path is the temporary copy from the live run.
- `evidence/mstest-run1-dotnet-test.log`, `evidence/mstest-run2-no-build-dotnet-test.log`:
  the runner output.
- `evidence/mstest-run2-no-build.coverage.cobertura.xml`: the second run's
  report.
