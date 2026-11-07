# NUnit Coverlet fixture

This fixture exercises `OtherGauge.Full(0)` and `OtherGauge.Full(1)` through
VSTest and Coverlet's Cobertura collector. The pinned packages are:

- NUnit 4.3.2
- NUnit3TestAdapter 5.0.0
- Microsoft.NET.Test.Sdk 17.12.0
- coverlet.collector 6.0.4

The project targets `net10.0` and references `../Beta.Library/Beta.Library.csproj`.

## Live run evidence

To avoid sharing `bin` or `obj` outputs with the other fixture projects, the
live run used a temporary copy containing only `Beta.Library` and `NUnit.Tests`:

```sh
dotnet test /tmp/coverlet-nunit.0UiIty/NUnit.Tests/NUnit.Tests.csproj \
  --no-build "--collect:XPlat Code Coverage" \
  --results-directory /tmp/coverlet-nunit.0UiIty/results-rerun \
  -- DataCollectionRunSettings.DataCollectors.DataCollector.Configuration.Format=cobertura
```

The runner reported:

```text
Test run for /tmp/coverlet-nunit.0UiIty/NUnit.Tests/bin/Debug/net10.0/NUnit.Tests.dll (.NETCoreApp,Version=v10.0)
VSTest version 18.0.2-dev (arm64)
Passed!  - Failed:     0, Passed:     1, Skipped:     0, Total:     1, Duration: 190 ms - NUnit.Tests.dll (net10.0)
Attachments:
  /tmp/coverlet-nunit.0UiIty/results-rerun/3f17815c-3187-419b-8f5d-808616d6d1d9/coverage.cobertura.xml
```

The generated Cobertura report recorded `OtherGauge` with `complexity="2"`,
`line-rate="1"`, `branch-rate="1"`, and its conditional line as
`condition-coverage="100% (2/2)"`. The observed fixture metric tuple is
CC2, coverage 1, CRAP2.

The current combined fixture adds one extra positive `Full(1)` call after this
initial standalone observation, giving Full hits 3/3/2/1/3. Its paired xUnit
report retains 2/2/1/1/2, so the live harness identifies both reports independently.
The coverage fraction remains 1 and CRAP remains 2.
