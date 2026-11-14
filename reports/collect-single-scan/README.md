# Collect single-scan validation

The collector retains line counts from the bytes hashed and parsed by its initial
scan. Staged native evidence uses the same reader, validator and hash/line binding
checks as `scan`, followed by source verification and destination validation before
publication. The public Rust API is unchanged.

| Removed work | Remaining check |
| --- | --- |
| Unhashed source reread for line counts | Initial scan computes counts from its hashed and parsed bytes |
| Source verification immediately following that reread | Initial scan still verifies its own sources; post-command and pre-publish verification remain |
| Second full scan of staged evidence | Shared native reader, validator and binding check against snapshot hashes/counts; original syntax/analysis checks and final source hash verification |

The original selected sources define the snapshot. Files created by the command
are outside it. Sources changed after post-command verification are detected by
the final verification as `SourceChanged`; unreadable/deleted sources retain
existing I/O errors. See [the collection contract](../../docs/COVERLET.md).
Literal Unix backslashes are rejected before running the command to prevent
normalization from hiding a native-path collision.

Direct snapshot tests exercise stale hashes, last-line boundaries, invalid keys,
invalid JSON, unsupported versions, unreadable evidence, and subsequent selected
entries. The collector counter test checks four source reads and hashes plus one
decode and parse per source, with different source lengths and evidence lines.
Counters and call sites compile only in unit-test builds. No dependencies or
unsafe code were added.

Independent Terra static review of the integrated implementation found no concrete
binding or publication issue. It did not execute Windows behavior. The private
parallel hash/count vectors are the least directly mutation-tested bookkeeping;
the differing-length two-source collection fixture checks their observable use.

Acceptance requires current-head standard/live tests, complete mutation inventory
and survivor review with a score strictly above 90%, and hosted same-job Benchmark
comparisons. Local timing values are not performance claims. Hosted collect rows
must satisfy the separated-samples rule before a speedup can be claimed.

## Local evidence

At implementation tree `6873463`, collect evidence was byte-identical to the
saved PR #38 baseline binary on all five default-scale generated workloads:
small-files, dense-file, many-callables, deep-nesting and solution. The harness
validated expected scores and exact evidence bytes in every paired run (four
measured pairs per workload). Invocation-private JSON and logs are retained under
`/tmp/collect-single-scan-validation/`; their local timings are not claims.

The current integrated source tree passed formatting, warnings-denied Clippy,
locked debug and release tests, explicit live SDK/Coverlet tests (xUnit, MSTest
and NUnit), and Python contracts (147 tests, two skipped); every command exited
zero. These results precede only documentation additions. The Python suite's
fixture score output is not a production mutation score: the full hosted mutation
run and its inventory/survivor review remain pending.
