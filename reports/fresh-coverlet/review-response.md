# PR review decisions

This is an integration record, not a substitute for rerunning final-tree gates.

- Sourcery 4501284506 (report count/project identity): narrowed the contract. Cobertura has no test-project identity and multiple projects legitimately cover the same assembly. The exact file count detects count mismatches, not project attestation. Trusted argv must run the intended projects; native missing evidence stays absent and can fail a later `scan --require-coverage`. A regression demonstrates duplicate report union does not mark a missing callable complete. Rejecting duplicate assembly evidence would break the requested multi-project merge semantics.
- Sourcery 4501284513 (ignored gates): removed embedded `ScanOptions`; `CollectOptions` exposes source root/paths only. Gates apply to subsequent scanning, so requested collection policies cannot silently be discarded.
- Sourcery 4501284518 (stalled tests): added a positive configurable deadline (CLI default 900 seconds), kill and reap of the immediate child, and a timeout/no-artifact regression. Descendant process authority remains part of the explicitly trusted command, documented rather than claimed as a sandbox.
- Sourcery 4501284522 (XML memory): bounded reads to 16 MiB (metadata check plus limited read for growth races) and DOM allocation to 500,000 nodes. Sparse oversized-file and small node-budget regressions check controlled rejection. Aggregate native coverage traversal optimization remains deferred to step 8.
- Sourcery 4501284525 (empty classes): empty selected or unselected class line lists now preserve absent evidence instead of rejecting valid input.
- Sourcery 4501284531 (stale report test): uses syntactically valid stale Cobertura outside the private directory and asserts the missing-fresh-report diagnostic.
- Cubic 4501284678 (MTP negative): uses correct `--solution` syntax and captures the SDK diagnostic naming both VSTest projects under an MTP-mode global.json. Generic nonzero status alone is no longer treated as runner evidence.
- Cubic 4501284682 (inventory provenance): the earlier JSON was generated from the actual `run-2ntvch4c` inventory, not copied from `initial/`; equal per-file counts did not imply equal inventories. Added exact inventory path/hash and superseded status. The final audit will be regenerated from complete final-run evidence.
- Cubic 4501284684 (XML assertions): removed the always-matching outer error context; unsafe/malformed XML tests require a forbidden/malformed reason.
- Cubic 4501284685 (fixture command): documented the explicit Cobertura format argument actually passed by the harness.
- Cubic 4501284698 (preflight assertions): invalid destination commands would create a marker if run; tests assert that marker is absent.
- Cubic 4501284701 (cleanup assertions): every post-spawn failure records the actual private directory path and asserts its removal.
- Cubic 4501284706 (source-prefix text): concatenates text/CDATA children across comments and rejects nested elements. A regression has two distinct selected files so truncation would assign evidence to the wrong file.
- Cubic 4501284710 (live opt-in documentation): names both E2E flags and disclaims skipped bodies as evidence.
- Cubic 4501284714 (plan grammar): ownership sentence corrected and wrapped.
- Cubic 4501284718 (underscore directory): only absolute deterministic `/_/` is reserved; exact local `_/A.cs` works and has a regression.
- Cubic 4501284726 (discoverability): top-level help points to `dotnet-crap collect --help`.
- Cubic 4501284732 (live provenance): fixture README links source-qualified evidence and identifies its initial observation separately. Final PR description/evidence will state actual completed results.
- Cubic 4501284736 (output mode): documents intentionally private native evidence (normally 0600 on Unix). Sharing is an explicit caller action after collection, preserving safe staging.

- CodeRabbit 4501301472 (empty packages): permits excluded/empty packages beside valid class records, but still rejects an entire report with no class records. Regression preserves both positive and zero-hit evidence when an empty package is present.

- Final independent Sol review of `18fc47a` found no blocking production correctness/process-safety issue. Its two nonblocking findings are fixed: the combined harness now validates both Alpha and both Beta reports plus one passed, non-skipped test in every project, and the MSTest README reflects solution integration. Focused library/parser/collector tests independently passed (44 tests).
