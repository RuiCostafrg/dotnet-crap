//! Syntax traversal and CRAP scoring of C# callables.

use crate::error::{fail, ErrorKind, OptionExt, Result};
use crate::Item;
use tree_sitter::Node;

/// Recognize implemented callable bodies, leaving explicit property/indexer accessors separate.
fn callable(n: Node<'_>) -> bool {
    matches!(
        n.kind(),
        "method_declaration"
            | "constructor_declaration"
            | "operator_declaration"
            | "conversion_operator_declaration"
            | "destructor_declaration"
            | "local_function_statement"
            | "accessor_declaration"
            | "lambda_expression"
            | "anonymous_method_expression"
    ) && (n.child_by_field_name("body").is_some() || n.kind() == "anonymous_method_expression")
        // Expression-bodied indexers store their arrow clause in `value`, not `body`.
        // Indexers with explicit accessors have no direct body; their accessors are callables.
        || matches!(n.kind(), "property_declaration" | "indexer_declaration")
            && n.child_by_field_name("value")
                .is_some_and(|v| v.kind() == "arrow_expression_clause")
}
/// Collapse whitespace in a syntax node's spelling without semantic type normalization.
fn normalized_text(n: Node<'_>, source: &str) -> String {
    n.utf8_text(source.as_bytes())
        .unwrap_or("")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
/// Collect ordered parameter types, omitting parameter names and modifiers.
/// Ordinary parameters wrap their type in `parameter`; `params` arrays expose
/// `type` directly on the list.
fn parameter_types(list: Option<Node<'_>>, source: &str) -> String {
    let Some(list) = list else {
        return String::new();
    };
    let mut types = Vec::new();
    for i in 0..list.child_count() {
        let Some(child) = list.child(i) else {
            continue;
        };
        let ty = if child.kind() == "parameter" {
            child.child_by_field_name("type")
        } else {
            (list.field_name_for_child(i as u32) == Some("type")).then_some(child)
        };
        if let Some(ty) = ty {
            types.push(normalized_text(ty, source));
        }
    }
    types.join(",")
}
/// Preserve the checked modifier when naming arithmetic and conversion operators.
fn checked_prefix(n: Node<'_>) -> &'static str {
    let mut c = n.walk();
    if n.children(&mut c).any(|child| child.kind() == "checked") {
        "checked "
    } else {
        ""
    }
}
/// Derive display names from syntax, including operator signatures and indexer types.
/// These names are not compilation-resolved or globally qualified overload identities.
fn callable_name(n: Node<'_>, source: &str) -> String {
    match n.kind() {
        "operator_declaration" => format!(
            "operator {}{}({})",
            checked_prefix(n),
            n.child_by_field_name("operator")
                .map(|x| normalized_text(x, source))
                .unwrap_or_default(),
            parameter_types(n.child_by_field_name("parameters"), source)
        ),
        "conversion_operator_declaration" => {
            let mut c = n.walk();
            let direction = n
                .children(&mut c)
                .map(|x| x.kind())
                .find(|k| matches!(*k, "implicit" | "explicit"))
                .unwrap_or("conversion");
            format!(
                "{direction} operator {}{}({})",
                checked_prefix(n),
                n.child_by_field_name("type")
                    .map(|x| normalized_text(x, source))
                    .unwrap_or_default(),
                parameter_types(n.child_by_field_name("parameters"), source)
            )
        }
        "destructor_declaration" => format!(
            "~{}",
            n.child_by_field_name("name")
                .map(|x| normalized_text(x, source))
                .unwrap_or_default()
        ),
        "indexer_declaration" => format!(
            "this[{}]",
            parameter_types(n.child_by_field_name("parameters"), source)
        ),
        _ => n
            .child_by_field_name("name")
            .and_then(|x| x.utf8_text(source.as_bytes()).ok())
            .unwrap_or(n.kind())
            .to_owned(),
    }
}
/// Return this node's direct contribution under the documented complexity convention.
fn decision(n: Node<'_>, source: &str) -> u32 {
    match n.kind() {
        "if_statement"
        | "for_statement"
        | "foreach_statement"
        | "while_statement"
        | "do_statement"
        | "catch_clause"
        | "conditional_expression"
        | "when_clause" => 1,
        "switch_section" => {
            let mut c = n.walk();
            let result = n
                .children(&mut c)
                .filter(|child| child.kind() == "case")
                .count() as u32;
            result
        }
        "switch_expression_arm" => {
            let mut c = n.walk();
            let result = u32::from(
                n.named_children(&mut c)
                    .next()
                    .is_none_or(|child| child.kind() != "discard"),
            );
            result
        }
        "binary_expression" | "assignment_expression" => {
            let mut c = n.walk();
            let result = u32::from(n.children(&mut c).any(|x| {
                matches!(
                    x.utf8_text(source.as_bytes()).unwrap_or(""),
                    "&&" | "||" | "??" | "??="
                )
            }));
            result
        }
        _ => 0,
    }
}
/// Per-file coverage evidence indexed for logarithmic range counts.
///
/// `lines` holds every covered line number once in ascending order and
/// `hit_prefix[i]` holds how many of `lines[..i]` have a positive hit count.
/// A callable therefore costs O(log P) per queried interval instead of walking
/// every coverage point in its span, which made deeply nested callables
/// quadratic in the number of points.
pub(crate) struct CoverageIndex {
    /// Covered line numbers in ascending order.
    lines: Vec<usize>,
    /// Prefix sums of `hits > 0` over `lines`, with `hit_prefix.len() == lines.len() + 1`.
    hit_prefix: Vec<usize>,
}

impl CoverageIndex {
    /// Build a sorted line index and its positive-hit prefix sums from coverage points.
    pub(crate) fn new(points: impl IntoIterator<Item = (usize, u64)>) -> Self {
        let mut points: Vec<(usize, u64)> = points.into_iter().collect();
        points.sort_unstable_by_key(|&(line, _)| line);
        let mut lines = Vec::with_capacity(points.len());
        let mut hit_prefix = Vec::with_capacity(points.len() + 1);
        hit_prefix.push(0);
        for (line, hits) in points {
            lines.push(line);
            hit_prefix.push(hit_prefix[hit_prefix.len() - 1] + usize::from(hits > 0));
        }
        Self { lines, hit_prefix }
    }

    /// Count coverage points with `a <= line <= b`, and how many of them have hits > 0.
    /// Callers guarantee `a <= b` (callable spans and clipped non-empty ranges).
    fn count(&self, a: usize, b: usize) -> (usize, usize) {
        let lo = self.lines.partition_point(|&line| line < a);
        let hi = self.lines.partition_point(|&line| line <= b);
        (hi - lo, self.hit_prefix[hi] - self.hit_prefix[lo])
    }
}

/// Sort and merge excluded line intervals into disjoint ascending ranges.
fn merge_ranges(ranges: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    let mut ranges = ranges;
    ranges.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (start, end) in ranges {
        if let Some(last) = merged.last_mut() {
            if start <= last.1 {
                last.1 = last.1.max(end);
                continue;
            }
        }
        merged.push((start, end));
    }
    merged
}

/// Count included and hit coverage points for one callable span.
/// `ranges` must be merged and disjoint; intervals are clipped to the span so
/// nested callable evidence is removed exactly once.
fn span_counts(
    c: &CoverageIndex,
    start: usize,
    end: usize,
    ranges: &[(usize, usize)],
) -> (usize, usize) {
    let (mut total, mut hit) = c.count(start, end);
    for &(range_start, range_end) in ranges {
        let range_start = range_start.max(start);
        let range_end = range_end.min(end);
        if range_start <= range_end {
            let (excluded_total, excluded_hit) = c.count(range_start, range_end);
            debug_assert!(excluded_total <= total && excluded_hit <= hit);
            total -= excluded_total;
            hit -= excluded_hit;
        }
    }
    (total, hit)
}

/// Return the hit fraction for one callable, or `None` when it owns no points.
fn span_coverage(
    c: &CoverageIndex,
    start: usize,
    end: usize,
    ranges: &[(usize, usize)],
) -> Option<f64> {
    let (total, hit) = span_counts(c, start, end, ranges);
    (total > 0).then(|| hit as f64 / total as f64)
}

/// Collect callable scores iteratively with bounded traversal and merged nested-coverage exclusions.
/// The caller supplies independent whole-tree and per-callable named-node budgets.
pub(crate) fn collect(
    n: Node<'_>,
    source: &str,
    file: &str,
    cov: Option<&CoverageIndex>,
    out: &mut Vec<Item>,
    traversal_budget: usize,
    callable_budget: usize,
) -> Result<()> {
    let mut nodes = vec![n];
    let mut visited = 0usize;
    while let Some(n) = nodes.pop() {
        visited += 1;
        if visited > traversal_budget {
            fail!(ResourceLimit, "syntax tree resource limit exceeded");
        }
        let mut cursor = n.walk();
        let children: Vec<_> = n.named_children(&mut cursor).collect();
        nodes.extend(children.into_iter().rev());
        if !callable(n) {
            continue;
        }
        let mut complexity = 1u32;
        let mut excluded = Vec::new();
        let mut stack = vec![(n, true)];
        let mut scored = 0usize;
        while let Some((node, outer)) = stack.pop() {
            scored += 1;
            if scored > callable_budget {
                fail!(ResourceLimit, "callable resource limit exceeded");
            }
            if !outer && callable(node) {
                excluded.push((node.start_position().row + 1, node.end_position().row + 1));
                continue;
            }
            complexity = complexity
                .checked_add(decision(node, source))
                .context(ErrorKind::ResourceLimit, "complexity overflow")?;
            let mut cursor = node.walk();
            stack.extend(node.named_children(&mut cursor).map(|child| (child, false)));
        }
        // Exclude nested spans as intervals, never one allocation per source
        // line. Long overlapping local-function spans must not expand work.
        let ranges = merge_ranges(excluded);
        let start = n.start_position().row + 1;
        let end = n.end_position().row + 1;
        let coverage = cov.and_then(|c| span_coverage(c, start, end, &ranges));
        let missed = 1.0 - coverage.unwrap_or(0.0);
        out.push(Item {
            file: file.to_owned(),
            name: callable_name(n, source),
            kind: n.kind().to_owned(),
            line: n.start_position().row + 1,
            column: n.start_position().column + 1,
            end_line: n.end_position().row + 1,
            complexity,
            coverage,
            crap: f64::from(complexity).powi(2) * missed.powi(3) + f64::from(complexity),
        });
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use tree_sitter::Parser as TsParser;

    /// Run the actual collector with deliberately small independent resource limits.
    fn collect_with_limits(source: &str, traversal: usize, scoring: usize) -> Result<Vec<Item>> {
        let mut parser = TsParser::new();
        parser
            .set_language(&tree_sitter_c_sharp::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();
        assert!(!tree.root_node().has_error());
        let mut items = Vec::new();
        collect(
            tree.root_node(),
            source,
            "C.cs",
            None,
            &mut items,
            traversal,
            scoring,
        )?;
        Ok(items)
    }

    #[test]
    /// Count even the root node and stop whole-tree traversal independently of scoring.
    fn traversal_budget_counts_nodes_and_accepts_the_exact_limit() {
        assert!(collect_with_limits("", 1, 0).unwrap().is_empty());
        let error = collect_with_limits("", 0, 100).err().unwrap();
        assert_eq!(error.to_string(), "syntax tree resource limit exceeded");
        let error = collect_with_limits("class C { void F() {} }", 1, 100)
            .err()
            .unwrap();
        assert_eq!(error.to_string(), "syntax tree resource limit exceeded");
    }

    #[test]
    /// Bound callable scoring separately; this empty method has five named syntax nodes.
    fn callable_budget_counts_nodes_and_accepts_the_exact_limit() {
        let source = "class C { void F() {} }";
        let items = collect_with_limits(source, 100, 5).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].complexity, 1);
        for budget in [0, 4] {
            let error = collect_with_limits(source, 100, budget).err().unwrap();
            assert_eq!(error.to_string(), "callable resource limit exceeded");
        }
    }

    /// Run the collector against native coverage points for one source file.
    fn collect_with_coverage(source: &str, points: &[(usize, u64)]) -> Vec<Item> {
        let mut parser = TsParser::new();
        parser
            .set_language(&tree_sitter_c_sharp::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();
        assert!(!tree.root_node().has_error());
        let coverage = CoverageIndex::new(points.iter().copied());
        let mut items = Vec::new();
        collect(
            tree.root_node(),
            source,
            "C.cs",
            Some(&coverage),
            &mut items,
            1_000_000,
            1_000_000,
        )
        .unwrap();
        items
    }

    /// Deterministic pseudo-random state for test-case generation.
    fn lcg_next(state: &mut u64) -> u64 {
        *state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        *state >> 33
    }

    /// Draw a deterministic value below `bound`.
    fn lcg_below(state: &mut u64, bound: u64) -> u64 {
        lcg_next(state) % bound
    }

    #[test]
    /// Merge hand-written intervals, including touching, overlapping and nested spans.
    fn merge_ranges_merges_touching_overlapping_and_nested_intervals() {
        assert_eq!(merge_ranges(vec![]), Vec::<(usize, usize)>::new());
        assert_eq!(merge_ranges(vec![(5, 8)]), vec![(5, 8)]);
        assert_eq!(merge_ranges(vec![(9, 12), (5, 8)]), vec![(5, 8), (9, 12)]);
        // Intervals that abut without sharing a line stay separate.
        assert_eq!(merge_ranges(vec![(5, 8), (9, 12)]), vec![(5, 8), (9, 12)]);
        assert_eq!(merge_ranges(vec![(5, 8), (8, 12)]), vec![(5, 12)]);
        // Nested and overlapping intervals collapse to the outer span.
        assert_eq!(merge_ranges(vec![(5, 12), (7, 9)]), vec![(5, 12)]);
        assert_eq!(merge_ranges(vec![(7, 9), (5, 12)]), vec![(5, 12)]);
        assert_eq!(
            merge_ranges(vec![(1, 3), (2, 5), (4, 8), (10, 10)]),
            vec![(1, 8), (10, 10)]
        );
        assert_eq!(merge_ranges(vec![(10, 10), (1, 3)]), vec![(1, 3), (10, 10)]);
        assert_eq!(merge_ranges(vec![(5, 8), (5, 8)]), vec![(5, 8)]);
        // A span covering only the first and last callable lines still merges.
        assert_eq!(
            merge_ranges(vec![(1, 1), (1, 100), (100, 100)]),
            vec![(1, 100)]
        );
    }

    #[test]
    /// The indexed range count and clipped subtraction match a point-by-point reference.
    fn coverage_index_and_clipped_subtraction_match_brute_force() {
        let mut state = 0x9e37_79b9_7f4a_7c15u64;
        // 3000 deterministic cases exceed the requested 2000 without new crates.
        for case in 0..3000 {
            let point_count = lcg_below(&mut state, 20) as usize;
            let mut points = Vec::with_capacity(point_count);
            for _ in 0..point_count {
                // Bias toward 9/10/100: their decimal strings sort in a
                // different order than their numeric line numbers.
                let line = match lcg_below(&mut state, 6) {
                    0 => 9 + lcg_below(&mut state, 2) as usize,
                    1 => 100,
                    2 => 10,
                    3 => 1 + lcg_below(&mut state, 15) as usize,
                    _ => 1 + lcg_below(&mut state, 140) as usize,
                };
                let hits = lcg_below(&mut state, 4);
                points.push((line, hits));
            }
            points.sort_unstable_by_key(|&(line, _)| line);
            points.dedup_by_key(|&mut (line, _)| line);
            let start = lcg_below(&mut state, 150) as usize;
            let end = start + lcg_below(&mut state, 20) as usize;
            let mut exclusions = Vec::new();
            match case % 7 {
                // Exclusions touching the first and last line of the span.
                0 => exclusions.push((start, start)),
                1 => exclusions.push((end, end)),
                // Overlapping and nested exclusions.
                2 => {
                    exclusions.push((start, end));
                    exclusions.push((start.saturating_add(3), end.saturating_add(5)));
                }
                // Empty exclusions.
                3 => {}
                // Adjacent intervals, both sharing and not sharing an endpoint.
                4 => {
                    exclusions.push((start.saturating_add(2), start.saturating_add(4)));
                    exclusions.push((start.saturating_add(5), start.saturating_add(7)));
                    exclusions.push((start.saturating_add(9), start.saturating_add(11)));
                    exclusions.push((start.saturating_add(11), start.saturating_add(13)));
                }
                _ => {
                    for _ in 0..lcg_below(&mut state, 5) {
                        let a = lcg_below(&mut state, 160) as usize;
                        let b = a + lcg_below(&mut state, 15) as usize;
                        exclusions.push((a, b));
                    }
                }
            }
            let merged = merge_ranges(exclusions.clone());
            // Reference walks every point and every raw exclusion, independent
            // of the merged index used by the production path.
            let mut reference_total = 0usize;
            let mut reference_hit = 0usize;
            for &(line, hits) in &points {
                let excluded = exclusions.iter().any(|&(a, b)| a <= line && line <= b);
                if start <= line && line <= end && !excluded {
                    reference_total += 1;
                    reference_hit += usize::from(hits > 0);
                }
            }
            let reference_coverage =
                (reference_total > 0).then(|| reference_hit as f64 / reference_total as f64);
            let index = CoverageIndex::new(points.iter().copied());
            assert_eq!(
                span_counts(&index, start, end, &merged),
                (reference_total, reference_hit),
                "case {case}: start={start} end={end} points={points:?} exclusions={exclusions:?}"
            );
            assert_eq!(
                span_coverage(&index, start, end, &merged),
                reference_coverage,
                "case {case}: start={start} end={end}"
            );
        }
    }

    #[test]
    /// Empty evidence stays absent while zero-hit evidence forms a zero denominator ratio.
    fn coverage_index_handles_empty_points_and_out_of_span_lines() {
        let empty = CoverageIndex::new(Vec::<(usize, u64)>::new());
        assert_eq!(empty.count(1, 100), (0, 0));
        assert_eq!(span_counts(&empty, 1, 100, &[]), (0, 0));
        assert_eq!(span_coverage(&empty, 1, 100, &[]), None);

        let index = CoverageIndex::new([(9, 0), (10, 2), (11, 0), (100, 1)]);
        assert_eq!(index.count(10, 99), (2, 1));
        assert_eq!(index.count(9, 9), (1, 0));
        assert_eq!(index.count(12, 99), (0, 0));
        assert_eq!(span_coverage(&index, 9, 11, &[]), Some(1.0 / 3.0));
        assert_eq!(span_coverage(&index, 9, 9, &[]), Some(0.0));
        assert_eq!(span_coverage(&index, 12, 99, &[]), None);
        // A clipped exclusion outside the span removes nothing.
        assert_eq!(span_counts(&index, 10, 11, &[(1, 9), (12, 200)]), (2, 1));
    }

    #[test]
    /// Native coverage maps are keyed by strings ("10" < "9"), so the index must sort numerically.
    fn coverage_index_sorts_points_given_in_string_key_order() {
        let index = CoverageIndex::new([(10, 0), (100, 1), (9, 1), (99, 0)]);
        assert_eq!(index.lines, vec![9, 10, 99, 100]);
        assert_eq!(index.hit_prefix, vec![0, 1, 1, 1, 2]);
        assert_eq!(index.count(9, 10), (2, 1));
        assert_eq!(index.count(11, 100), (2, 1));
    }

    #[test]
    /// Assign coverage to the innermost callable and exclude nested callable spans.
    fn collect_owns_coverage_by_innermost_callable_with_nested_lambdas() {
        let source = "class C
{
    void Outer()
    {
        System.Action a = () =>
        {
            int x = 1;
        };
        int Local()
        {
            System.Action b = () =>
            {
                int z = 3;
            };
            b();
            return 2;
        }
        int y = Local();
    }
}";
        // Every line is covered except 7 (outer lambda), 13 (inner lambda)
        // and 15 (local function body).
        let points: Vec<(usize, u64)> = (1..=20)
            .map(|line| (line, if matches!(line, 7 | 13 | 15) { 0 } else { 1 }))
            .collect();
        let items = collect_with_coverage(source, &points);
        assert_eq!(items.len(), 4);

        let at = |line| items.iter().find(|item| item.line == line).unwrap();

        // Outer: 3, 4, 18, 19 all hit; nested spans 5..=8 and 9..=17 removed.
        let outer = at(3);
        assert_eq!(
            (outer.name.as_str(), outer.end_line, outer.complexity),
            ("Outer", 19, 1)
        );
        assert_eq!(outer.coverage, Some(4.0 / 4.0));

        // Outer lambda: 5, 6, 8 hit and 7 misses.
        let outer_lambda = at(5);
        assert_eq!(outer_lambda.name, "lambda_expression");
        assert_eq!((outer_lambda.end_line, outer_lambda.complexity), (8, 1));
        assert_eq!(outer_lambda.coverage, Some(3.0 / 4.0));

        // Local: 9, 10, 16, 17 hit and 15 misses; nested span 11..=14 removed.
        let local = at(9);
        assert_eq!(
            (local.name.as_str(), local.end_line, local.complexity),
            ("Local", 17, 1)
        );
        assert_eq!(local.coverage, Some(4.0 / 5.0));

        // Inner lambda: 11, 12, 14 hit and 13 misses.
        let inner_lambda = at(11);
        assert_eq!(inner_lambda.name, "lambda_expression");
        assert_eq!((inner_lambda.end_line, inner_lambda.complexity), (14, 1));
        assert_eq!(inner_lambda.coverage, Some(3.0 / 4.0));
    }
}
