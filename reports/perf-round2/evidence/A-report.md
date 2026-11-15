# A — first-party syntax traversal (src/analysis.rs) — investigation report

Status: IN PROGRESS (updated incrementally)

## Grammar facts verified (tree-sitter-c-sharp 0.23.5 node-types.json)
- `&&`, `||`, `??`, `??=`, `case`, `checked`, `implicit`, `explicit` are anonymous tokens only; `discard`,
  `arrow_expression_clause` and all callable kinds are named only.
- binary_expression / assignment_expression `operator` field is always one of the anonymous operator tokens.
- `Node::kind()` == `language.node_kind_for_id(node.kind_id())` (both go through ts_node_symbol incl. aliases),
  so a table indexed by kind_id and filled by *name* is exactly equivalent to string compares, regardless of
  named/anonymous duplicates or aliases.

## Design landed on (prototype, src/analysis.rs)
- `Kinds`: a `Vec<Class>` indexed by `kind_id`, filled by NAME for all `node_kind_count()` symbols (so it's
  exactly the string compare), plus field ids for `body`/`value`/`operator` (`child_by_field_id`).
- `collect` = `single_pass`: one reused `TreeCursor` walked depth-first (goto_first_child / next_sibling / parent),
  visiting only named nodes and never descending into anonymous ones, which is exactly the node set and pre-order
  of `named_children` + reversed stack. A stack of open callables (item index, depth, complexity,
  per-callable count, excluded ranges). The Item is pushed at callable entry (keeps pre-order output) and filled when
  the walk leaves the callable's depth.
- `decision`: int dispatch; `binary/assignment_expression` checks the kind of the `operator` field child instead of
  `utf8_text` on every child (no from_utf8). switch_section: counts `case` children with a reusable scratch cursor.
- Budgets: the fast pass counts the same quantities (outer named-node count; per callable: own root + every
  node whose innermost open callable it is, incl. nested-callable roots). On ANY limit hit (or complexity overflow) it
  truncates `out` back to its entry length and re-runs `collect_reference` (the old code, kept verbatim), so the
  error message, and which of the two limits fires first, are exactly the reference's by construction. Fast Ok ⇒
  every count is within budget ⇒ reference Ok with identical items. Cost: none on the success path; on the error
  path at most ~1M extra node visits once (the error ends the scan anyway).
- Why a fallback rather than exact in-pass ordering: the reference reports "callable resource limit" at the
  callable's PRE-order position (it walks the callable's whole subtree on entry), but a single pass only knows the
  callable's count at subtree EXIT. Between the two, the traversal budget can run out, so the single pass would report
  "syntax tree resource limit" where the reference reports "callable resource limit". Smallest deviation without
  the fallback: "when both limits are exceeded, the message may name the traversal limit instead of the callable
  limit" (same ErrorKind::ResourceLimit, same exit code). With the fallback there's no deviation.

## Correctness evidence
- 5 workloads x {scan, scan-coverage}: `cmp` against /tmp/inv/corpora/*/expected-*.json: all identical.
- `cargo test --locked`: 151 passed, 0 failed (before the prototype tests were added).
- Differential unit test `prototype_variants_match_reference_on_repository_sources`: reference vs ids_only vs
  cursor_two_pass vs single_pass on all .cs under tests/ + examples/ + hand-written edge source, plus (via
  A_DIFF_DIRS) all ~1600 corpus files of the 5 workloads; with and without a coverage index. All identical (Debug of items).
- `prototype_variants_match_reference_on_budget_boundaries`: tb, cb in 0..=40 x 9 sources, Ok/Err + message +
  items identical for all variants.

## In-process micro-benchmark (primary evidence)
Command: `A_BENCH_ITERS=30 A_BENCH_DIRS=<corpora dirs> cargo test --release --locked --lib prototype_bench -- --ignored --nocapture`
(log: /tmp/inv/A-bench2.log). All variants live in one binary on the same pre-parsed trees, order rotated each iteration,
no coverage. ns/node = min time / named nodes. "saved" = (ref_min - variant_min) / (parse_min + ref_min).

| workload | named nodes | parse min ms | ref collect min ms (ns/node) | ids_only | cursor_two_pass | single_pass | single_pass collect speedup | % of parse+collect removed |
|---|---|---|---|---|---|---|---|---|
| dense-file | 177,501 | 307.8 | 162.7 (917) | 131.9 (743) | 88.4 (498) | 51.5 (290) | x3.16 | 23.6% |
| many-callables | 104,001 | 173.1 | 92.5 (889) | 78.6 (756) | 54.6 (525) | 35.7 (343) | x2.59 | 21.4% |
| deep-nesting | 38,018 | 63.3 | 33.2 (874) | 28.3 (745) | 20.3 (533) | 12.5 (328) | x2.67 | 21.5% |
| small-files (500 files) | 19,500 | 31.0 | 18.7 (956) | 15.5 (796) | 10.8 (552) | 6.8 (348) | x2.74 | 23.9% |
| solution (1101 files) | 41,804 | 62.6 | 33.4 (800) | 28.6 (685) | 19.9 (475) | 12.9 (308) | x2.60 | 21.4% |

Per-idea contribution (of the ~65% collect time removed): kind ids alone ≈ 5-7% of parse+collect (collect x1.2);
+ reused cursor (still two walks) ≈ another 8-9% (x1.7 cumulative); + merging into one pass ≈ another 7-8% (x2.6-3.2 cumulative).
The utf8_text→operator-field change is folded into "ids". The first run (A-bench1.log) gave the same numbers within ~5%.

Findings while doing this:
- `TreeCursor::depth()` is O(depth) in tree-sitter 0.27 (ts_tree_cursor_current_depth loops the stack). A cursor walk that
  calls it per step is quadratic on deep-nesting (first run: 3,963 ms vs 33 ms). Track depth by hand (the prototype does).
- Building the 500-symbol kind table costs ~12 µs per call: per file it **halves** the gain on small-files/solution
  ("single_pass+table" row in the log: x1.42 / x1.32). Build it once per scan (`lib.rs` now does
  `analysis::Kinds::new(&language)` next to the parser and calls `collect_with_kinds`).
- Even after the change, collect is still ~290-350 ns per named node, about 20% of parse cost. What remains is tree-sitter cursor
  stepping, `cursor.node()`/`is_named`, `child_by_field_id` for callable checks, positions, and per-Item String building.
