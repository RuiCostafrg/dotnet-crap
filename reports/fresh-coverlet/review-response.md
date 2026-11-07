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

- Final independent Sol review of `18fc47a` found no blocking production correctness/process-safety issue. Its two nonblocking findings are fixed: the combined harness now validates both Alpha and both Beta reports plus one passed, non-skipped test in every project, and the MSTest README reflects solution integration. The reviewer reported its focused test result at `18fc47a`; that raw reviewer log was not archived and is not cited as final-tree validation. Parent validation at `151356d` is recorded separately in `focused-151356d.json` / `focused-151356d.log` using `cargo test --locked --test collector --test cobertura_parser --lib`.

- Follow-up Cubic findings 4501329615/18/19/21/22/26: kept the expanded live-validation checklist pending until combined validation lands; restored the independently enumerated source-module list in the audit; clarified class-level zero-hit and optional-line documentation; identified the standalone NUnit observation as unarchived; and reject child statuses first observed at or after the deadline. CodeRabbit 4501331329 and Cubic 4501329613 refer to the stale MSTest README already corrected in `4fdff9e`.

- Cubic 4501335130 (review-test provenance): explicitly separated the historical independent review note from current parent validation, captured the current focused command/log/revision, and captured a source-qualified parent validation record. Existing `tested-files.json` data is historical until final validation finishes; regenerate it from the final production/test/Cargo inputs, then verify any later evidence-only commit preserves those bytes.

- Parent mapping audit: preserved literal source-prefix edge whitespace. A regression creates two real selected directories so trimming would demonstrably transfer evidence to the wrong source. This completes the same exact-path contract as the comment/CDATA fix.

- Follow-up documentation findings 4501364351/4501416076/4501432383: clarified that every package still requires a `classes` container; the final evidence document will name the validated production/test revision explicitly, and its manifest will be regenerated after final validation rather than retaining historical hashes.

- CodeRabbit review 5811662696 (nonblocking report distinction): MSTest/NUnit now add one extra positive Full call; the harness requires exactly one 2-call and one 3-call report per library. This verifies per-project evidence signatures without changing the supplied-line coverage or CRAP expectations.

- Live signature validation caught an incorrect parent expectation: Beta tests `value == 0`, so NUnit's additional Full(1) call increments its fallback return. Corrected NUnit hits to 3/3/1/2/3 (MSTest remains 3/3/2/1/3); both coverage ratios and CRAP scores remain unchanged.

- CodeRabbit outside-diff review 5811728529 (superseded inventory wording): final evidence must replace intermediate current-run language with the completed source-qualified inventory and outcomes. Superseded/partial local runs remain explicitly excluded from final mutation-score claims.
