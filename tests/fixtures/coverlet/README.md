# Pinned VSTest and Coverlet collector fixture

This fixture is a two-test-project VSTest solution. Each test project references
a library containing the measured C# sources, and each pins
`Microsoft.NET.Test.Sdk` 17.12.0 and `coverlet.collector` 6.0.4. The test
projects target `net10.0`; xUnit is pinned to 2.9.2 with the Visual Studio
adapter pinned to 2.8.2. This exercises the VSTest collector path; it makes no
claim about Microsoft Testing Platform.

Run from the repository root with:

```sh
DOTNET_CRAP_COVERLET_E2E=1 cargo test --test coverlet_live -- --exact pinned_coverlet_collector_emits_fresh_multproject_cobertura --nocapture
```

The test invokes `dotnet test CoverletFixtures.sln
--collect:XPlat Code Coverage --results-directory <fresh-temp-path> -m:1` and
requires exactly two nonempty `coverage.cobertura.xml` reports, one for each
referenced library. The serial MSBuild setting avoids concurrent builds of
shared fixture outputs. Outside the opt-in environment variable, the Rust test
returns without launching .NET.

## Hand-derived line coverage and CRAP

CRAP uses `M² × (1 − coverage)³ + M`, where `M` is cyclomatic complexity and
coverage is the fraction of executable sequence-point lines with a positive
hit count. The counts below are derived from the physical source and test calls;
the expected hit counts were then checked against the collector output. Brace
and decision lines appear as Coverlet sequence points, so they are included.

| Project / callable | Source lines | Hand-counted hits | Line coverage | M | Expected CRAP |
| --- | --- | ---: | ---: | ---: | ---: |
| Alpha / `Gauge.Choose` | `Gauge.First.cs:6-10` | 4 / 5 | 0.8 | 2 | 2.016 |
| Alpha / `Gauge.Full` | `Gauge.First.cs:13-17` | 5 / 5 | 1.0 | 2 | 2.0 |
| Alpha / `Gauge.Uncovered` | `Gauge.Second.cs:5` | 0 / 1 | 0.0 | 1 | 2.0 |
| Beta / `OtherGauge.Full` | `OtherGauge.cs:6-10` | 5 / 5 | 1.0 | 2 | 2.0 |

`Choose` takes only the positive branch: its opening brace, decision, positive
return and closing brace are hit, while its fallback return is not. Its score
is `2² × 0.2³ + 2 = 2.016`. `Full` and `OtherGauge.Full` take both branches,
so all five sequence-point lines have hits and the score is `M = 2`. The
uncovered expression-bodied method has complexity one and zero covered lines,
so its score is `1² × 1³ + 1 = 2`.

The class `Gauge` is intentionally partial across two source files. The method
in `Gauge.First.cs` is partially covered while the method in `Gauge.Second.cs`
is uncovered; Cobertura emits separate class records with the same class name
and different filenames. This demonstrates why the source filename is part of
the identity when a consumer merges partial-class records.

## Observed live run

On 2026-11-06 this workspace had .NET SDK 10.0.109 and runtime 10.0.9 on
Linux ARM64. The pinned collector run completed both tests and emitted two
reports. Coverlet reported `Choose` as 4/5 line coverage (`0.8`), `Full` as
5/5 (`1`), `Uncovered` as 0/1 (`0`), and Beta's `Full` as 5/5 (`1`). These
observations are from the actual VSTest collector run on that host; other SDK,
runtime, OS and test-runner combinations require their own live evidence.
