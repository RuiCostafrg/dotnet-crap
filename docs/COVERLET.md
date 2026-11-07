# Fresh Coverlet/Cobertura collection

`dotnet-crap collect` runs an explicitly supplied trusted test command, imports
fresh Cobertura line evidence, and creates native coverage v1. Scanning and
scoring never start a process. OpenCover and Microsoft coverage adapters remain
future work. No native schema or report completeness semantics change here.

## Invocation and trust boundary

Pin `coverlet.collector` in the test project. The fixtures use **6.0.4**, with
Microsoft.NET.Test.Sdk **17.12.0**. The framework matrix pins xUnit **2.9.2** /
adapter **2.8.2**, MSTest framework/adapter **4.5.2**, and NUnit **4.3.2** /
NUnit3TestAdapter **5.0.0**. These pins describe the tested fixture, not a promise that arbitrary
commands or other versions behave identically.

```sh
dotnet-crap collect --root /absolute/path/to/repo \
  --path src --output /absolute/path/to/new-coverage.json --expected-reports 1 \
  -- dotnet test tests/Tests.csproj --collect 'XPlat Code Coverage' \
     --results-directory '{results}' -- \
     DataCollectionRunSettings.DataCollectors.DataCollector.Configuration.Format=cobertura

dotnet-crap --root /absolute/path/to/repo --coverage /absolute/path/to/new-coverage.json \
  --require-coverage src
```

`--path` is repeatable and uses the same selection rules as scanning. The output
path resolves against the invoking process's working directory; the child runs
in the canonical source root. The command must contain exactly one standalone
`{results}` argument, replaced with a new private temporary directory. Arguments
are passed directly; there is no shell splitting or expansion. Supply the exact
expected report count (projects times target frameworks) to detect missing/extra
report files. Cobertura does not identify test projects
or frameworks: matching counts cannot prove that every intended project ran,
especially when several projects cover the same library. The trusted command
must execute the intended project/target set; duplicate reports are not project
identity attestations. An existing destination, including a symlink or
hardlink, is an error. Its parent directory must exist; `.cs` destinations fail.
`collect` is reserved as the first CLI argument; use `./collect` to scan a
directory with that name. The child deadline defaults to 900 seconds
(`--timeout-seconds`); a timeout
kills and reaps the immediate process and publishes nothing. Statuses first
observed at or after the deadline are rejected even if the child has exited;
this deliberately fails closed when observation is delayed. Descendant processes
remain the trusted command's responsibility; this is not a process-tree sandbox.
Collection success exits 0; collection errors exit 2. Scan policy exits stay 0/1/2.

The entire child command, its dependencies, and test code are trusted executable
code. The caller must pin the collector, choose the runner and format explicitly,
and ensure the command actually runs the intended tests without reusing old build
or coverage artifacts. The adapter cannot authenticate arbitrary command output
or sandbox trusted code. It discovers only `coverage.cobertura.xml` files within
its invocation-private directory, rejects symbolic links in that directory, and
requires the exact expected report count. Old files in the project tree are never
searched or imported. A command that ignores `{results}` fails with missing reports.

## Snapshot and publication

Before executing the command, the adapter scans selected sources, retains their
original decoded text and SHA-256 byte hashes in memory, and verifies those hashes.
After the command succeeds it re-verifies original source bytes, parses all fresh
reports, stages native evidence beside the destination, validates it through the
native scanner, and verifies original hashes again immediately before atomic
publication without replacing an existing file. Temporary evidence and the private
results directory are removed on success or error. Failed tests, missing reports,
invalid XML, invalid paths, changed or deleted sources, and publication errors
leave no new native artifact from the adapter.

This is byte verification at explicit boundaries, not a filesystem lock. It does
not detect a transient edit restored before verification, inspect unselected
source changes, or freeze files added after selection. Those limits match the
existing scanner snapshot model. Reports and sources are never overwritten by
the adapter. Trusted commands retain their normal filesystem authority.

## Mapping contract

Cobertura class-level `<lines>` supplies one evidence point per file and line.
Method-level copies and summary percentages are not alternative scoring inputs.
Zero-hit executable lines count in the denominator; missing lines are absent,
not invented zeros. Duplicate lines across classes, partial classes, projects or
assemblies merge by maximum hit count. This is union-of-runs line coverage:
a hit in any report marks the line covered. It does not prove coverage separately
for every target framework or assembly. Select a single target/run if that is the
policy you need. Counts are not summed, so duplicate instrumentation cannot change
scores or overflow during merging.

Collection options select sources only (`root`, `paths`); scoring gates belong
to a subsequent `scan`, not evidence production. Native evidence is staged with
owner-only permissions on Unix (normally 0600); callers can explicitly grant
read access after successful collection if needed.

Existing syntax ownership is retained: nested callables own their line ranges,
and outer callables exclude nested ranges. Same-line nested callables remain
ambiguous because Cobertura has no usable column ownership. This adapter does
not infer sequence points, branches, statement completeness, signatures, or
compiler-generated callable identities. `--require-coverage` still means some
supplied evidence for every callable. A callable without any supplied points
retains absent coverage; a callable with only zero-hit points has coverage 0.
Partial class files are distinct source identities and follow the same rules.

Paths must resolve exactly within the scan root. No basename or suffix guessing,
network lookup, or implicit checkout mapping is allowed. UNC roots (including host-native Windows UNC paths) are unsupported in this
first adapter. Windows drive-absolute paths are accepted on Windows; foreign
Windows roots,
SourceLink URLs and deterministic `/_/` paths without a resolvable exact local
identity fail. Native evidence retains slash-separated root-relative identities.
Tests cover host-native absolute paths and foreign Windows roots separately;
rejecting a foreign drive on Unix is not evidence of a live Windows collector run.

XML parsing rejects malformed XML, DTD/entity declarations, external references,
invalid/zero/out-of-range line numbers and invalid hit counts. Reports are
limited to 16 MiB and 500,000 XML nodes before further
processing. Empty class line lists leave evidence absent. The parser does
not retrieve network or filesystem entities. Source hashes are generated from the
pre-command snapshot, never from bytes first observed after tests.

## Runner evidence and limits

The executable fixtures and independent score derivations are in
[tests/fixtures/coverlet](../tests/fixtures/coverlet/README.md). Live collection is
explicitly enabled with `DOTNET_CRAP_COVERLET_E2E=1`; an ordinary skipped test is
not collector evidence. Current-head validation and exact host/toolchain details
belong in the feature evidence report, separate from historical API measurements.

VSTest collector support and Microsoft Testing Platform support must be evaluated
separately. Coverlet's official documentation explicitly distinguishes
`coverlet.collector` (VSTest) from `coverlet.MTP`; this adapter does not claim MTP
parity from VSTest runs. MTP and Microsoft coverage formats remain unsupported
until their own pinned live evidence and adapter contract exist. Arbitrary argv
is not runner detection: unsupported runner invocations may fail in the runner
or produce no accepted report, rather than a preflight mode diagnostic.

Primary references used for the design:

- [Coverlet VSTest integration](https://github.com/coverlet-coverage/coverlet/blob/master/Documentation/VSTestIntegration.md): collector invocation, configuration and runner distinction. Format is explicit here because documentation defaults are not a versioned contract.
- [Coverlet deterministic builds](https://github.com/coverlet-coverage/coverlet/blob/master/Documentation/DeterministicBuild.md): deterministic path behavior; this adapter does not guess remappings.
- [Microsoft unit-test coverage](https://learn.microsoft.com/en-us/dotnet/core/testing/unit-testing-code-coverage): XPlat collector workflow.
- [Microsoft Testing Platform coverage](https://learn.microsoft.com/en-us/dotnet/core/testing/microsoft-testing-platform-code-coverage): separate runner extensions.
- [roxmltree parsing options](https://docs.rs/roxmltree/0.20.0/roxmltree/struct.ParsingOptions.html): DTD parsing is disabled by default; the importer additionally rejects declarations explicitly.
