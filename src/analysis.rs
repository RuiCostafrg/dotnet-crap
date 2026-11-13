//! Syntax traversal and CRAP scoring of C# callables.

use crate::error::{fail, ErrorKind, OptionExt, Result};
use crate::Item;
use std::num::NonZeroU16;
use tree_sitter::{Language, Node, TreeCursor};

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
            (list.field_name_for_child(i) == Some("type")).then_some(child)
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

/// Syntactic role of a node kind, as the string comparisons in [`callable`] and
/// [`decision`] classify it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Class {
    Other,
    /// A callable only when it has a `body` field.
    CallableWithBody,
    /// `anonymous_method_expression`: always a callable.
    AnonymousMethod,
    /// Property or indexer: a callable when its `value` is an arrow clause.
    CallableWithArrowValue,
    Arrow,
    /// Contributes exactly one decision.
    Decision,
    /// Contributes one decision per `case` token among its children.
    SwitchSection,
    /// Contributes one decision unless its pattern is a discard.
    SwitchArm,
    /// `binary_expression` or `assignment_expression`: one decision for a logical operator.
    Operation,
    Case,
    Discard,
    LogicalOperator,
}

/// Kind names and their roles. Must stay in step with [`callable`] and [`decision`];
/// the differential tests compare both classifications on every node of their corpus.
const CLASSES: &[(&str, Class)] = &[
    ("method_declaration", Class::CallableWithBody),
    ("constructor_declaration", Class::CallableWithBody),
    ("operator_declaration", Class::CallableWithBody),
    ("conversion_operator_declaration", Class::CallableWithBody),
    ("destructor_declaration", Class::CallableWithBody),
    ("local_function_statement", Class::CallableWithBody),
    ("accessor_declaration", Class::CallableWithBody),
    ("lambda_expression", Class::CallableWithBody),
    ("anonymous_method_expression", Class::AnonymousMethod),
    ("property_declaration", Class::CallableWithArrowValue),
    ("indexer_declaration", Class::CallableWithArrowValue),
    ("arrow_expression_clause", Class::Arrow),
    ("if_statement", Class::Decision),
    ("for_statement", Class::Decision),
    ("foreach_statement", Class::Decision),
    ("while_statement", Class::Decision),
    ("do_statement", Class::Decision),
    ("catch_clause", Class::Decision),
    ("conditional_expression", Class::Decision),
    ("when_clause", Class::Decision),
    ("switch_section", Class::SwitchSection),
    ("switch_expression_arm", Class::SwitchArm),
    ("binary_expression", Class::Operation),
    ("assignment_expression", Class::Operation),
    ("case", Class::Case),
    ("discard", Class::Discard),
    ("&&", Class::LogicalOperator),
    ("||", Class::LogicalOperator),
    ("??", Class::LogicalOperator),
    ("??=", Class::LogicalOperator),
];

/// Node-kind classification indexed by kind id, so the traversal dispatches on
/// integers instead of comparing kind names and reading operator text.
///
/// Build it once per scan: it costs one pass over the grammar's symbols. It is
/// immutable, so one table can be shared by reference across threads.
pub(crate) struct KindTable {
    /// Role of every kind id, filled by name, so aliases and named/anonymous
    /// kinds that share a name classify exactly as the string comparisons do.
    classes: Box<[Class]>,
    body: Option<NonZeroU16>,
    value: Option<NonZeroU16>,
    operator: Option<NonZeroU16>,
}

impl KindTable {
    /// Classify every node kind of `language`.
    pub(crate) fn new(language: &Language) -> Self {
        let classes = (0..language.node_kind_count())
            .map(|id| {
                let name = u16::try_from(id)
                    .ok()
                    .and_then(|id| language.node_kind_for_id(id));
                CLASSES
                    .iter()
                    .find(|&&(known, _)| Some(known) == name)
                    .map_or(Class::Other, |&(_, class)| class)
            })
            .collect();
        Self {
            classes,
            body: language.field_id_for_name("body"),
            value: language.field_id_for_name("value"),
            operator: language.field_id_for_name("operator"),
        }
    }

    /// Role of a node. Ids outside the table, such as the `ERROR` symbol, have none.
    fn class(&self, n: Node<'_>) -> Class {
        self.classes
            .get(usize::from(n.kind_id()))
            .copied()
            .unwrap_or(Class::Other)
    }

    /// Same result as [`callable`].
    fn callable(&self, n: Node<'_>) -> bool {
        let field = |id: Option<NonZeroU16>| id.and_then(|id| n.child_by_field_id(id.get()));
        match self.class(n) {
            Class::CallableWithBody => field(self.body).is_some(),
            Class::AnonymousMethod => true,
            Class::CallableWithArrowValue => {
                field(self.value).is_some_and(|v| self.class(v) == Class::Arrow)
            }
            _ => false,
        }
    }

    /// Same result as [`decision`]; `scratch` is a reusable cursor for this tree.
    fn decision<'t>(&self, n: Node<'t>, scratch: &mut TreeCursor<'t>) -> u32 {
        match self.class(n) {
            Class::Decision => 1,
            Class::SwitchSection => n
                .children(scratch)
                .filter(|child| self.class(*child) == Class::Case)
                .count() as u32,
            Class::SwitchArm => u32::from(
                n.named_child(0)
                    .is_none_or(|child| self.class(child) != Class::Discard),
            ),
            // The grammar stores the operator token in `operator`, and a token's text
            // is its kind name, so no source text is read. Other children of a valid
            // expression cannot spell exactly an operator.
            Class::Operation => u32::from(
                self.operator
                    .and_then(|id| n.child_by_field_id(id.get()))
                    .is_some_and(|op| self.class(op) == Class::LogicalOperator),
            ),
            _ => 0,
        }
    }
}

/// Which traversal produced a [`collect`] result.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Traversal {
    /// The single pass finished within every limit.
    SinglePass,
    /// The single pass hit a limit and the reference traversal decided the result.
    Reference,
}

/// Collect callable scores with bounded traversal and merged nested-coverage exclusions.
/// The caller supplies independent whole-tree and per-callable named-node budgets.
///
/// One depth-first pass visits every named node once, in the reference's pre-order.
/// If it hits any limit, its items are discarded and the reference traversal runs
/// instead, so the error, and which limit is reported first, stay the reference's.
#[allow(clippy::too_many_arguments)]
pub(crate) fn collect(
    kinds: &KindTable,
    n: Node<'_>,
    source: &str,
    file: &str,
    cov: Option<&CoverageIndex>,
    out: &mut Vec<Item>,
    traversal_budget: usize,
    callable_budget: usize,
) -> Result<Traversal> {
    let len = out.len();
    let budgets = (traversal_budget, callable_budget);
    if single_pass(kinds, n, source, file, cov, out, budgets).is_ok() {
        return Ok(Traversal::SinglePass);
    }
    out.truncate(len);
    collect_reference(n, source, file, cov, out, traversal_budget, callable_budget)?;
    Ok(Traversal::Reference)
}

/// The single pass hit a budget or overflowed; the reference decides the error.
struct Exceeded;

/// Count one node against a budget.
fn count(counter: &mut usize, budget: usize) -> std::result::Result<(), Exceeded> {
    *counter += 1;
    if *counter > budget {
        return Err(Exceeded);
    }
    Ok(())
}

/// A callable whose subtree the single pass is still walking.
struct Open {
    /// Index of its reserved item in `out`.
    item: usize,
    /// Cursor depth of the callable node below the walk's root.
    depth: usize,
    complexity: u32,
    /// Named nodes scored for this callable, as the reference counts them.
    scored: usize,
    /// Line spans of directly nested callables.
    excluded: Vec<(usize, usize)>,
}

impl Open {
    /// Fill in the callable's reserved item once its subtree is done.
    fn finish(self, cov: Option<&CoverageIndex>, out: &mut [Item]) {
        let item = &mut out[self.item];
        let ranges = merge_ranges(self.excluded);
        let coverage = cov.and_then(|c| span_coverage(c, item.line, item.end_line, &ranges));
        let missed = 1.0 - coverage.unwrap_or(0.0);
        item.complexity = self.complexity;
        item.coverage = coverage;
        item.crap =
            f64::from(self.complexity).powi(2) * missed.powi(3) + f64::from(self.complexity);
    }
}

/// Score every callable under `root` in one walk with one reused cursor.
///
/// Each named node counts once against the traversal budget and once against the
/// innermost open callable, exactly as the reference counts it. A nested callable
/// also counts against its parent, which excludes its lines. Items are reserved in
/// pre-order when a callable opens and filled in when its subtree closes, which is
/// the reference's output order.
fn single_pass(
    kinds: &KindTable,
    root: Node<'_>,
    source: &str,
    file: &str,
    cov: Option<&CoverageIndex>,
    out: &mut Vec<Item>,
    (traversal_budget, callable_budget): (usize, usize),
) -> std::result::Result<(), Exceeded> {
    // The cursor is rooted at `root`: it never moves to root's siblings or parent,
    // which ends the walk. Depth is tracked by hand because `TreeCursor::depth()`
    // is O(depth) and would make the walk quadratic on deep nesting.
    let mut cursor = root.walk();
    let mut scratch = root.walk();
    let mut open: Vec<Open> = Vec::new();
    let mut visited = 0usize;
    let mut depth = 0usize;
    'visit: loop {
        let node = cursor.node();
        count(&mut visited, traversal_budget)?;
        let is_callable = kinds.callable(node);
        if let Some(parent) = open.last_mut() {
            count(&mut parent.scored, callable_budget)?;
            if is_callable {
                parent
                    .excluded
                    .push((node.start_position().row + 1, node.end_position().row + 1));
            } else {
                parent.complexity = parent
                    .complexity
                    .checked_add(kinds.decision(node, &mut scratch))
                    .ok_or(Exceeded)?;
            }
        }
        if is_callable {
            let mut scored = 0;
            count(&mut scored, callable_budget)?;
            let complexity = 1u32
                .checked_add(kinds.decision(node, &mut scratch))
                .ok_or(Exceeded)?;
            let start = node.start_position();
            out.push(Item {
                file: file.to_owned(),
                name: callable_name(node, source),
                kind: node.kind().to_owned(),
                line: start.row + 1,
                column: start.column + 1,
                end_line: node.end_position().row + 1,
                complexity: 0,
                coverage: None,
                crap: 0.0,
            });
            open.push(Open {
                item: out.len() - 1,
                depth,
                complexity,
                scored,
                excluded: Vec::new(),
            });
        }
        // Descend to the first named child; anonymous tokens are skipped, never entered.
        if cursor.goto_first_child() {
            depth += 1;
            if seek_named(&mut cursor) {
                continue 'visit;
            }
            cursor.goto_parent();
            depth -= 1;
        }
        // The current node's subtree is done. Close it if it is a callable, then move
        // to the next named sibling of the nearest ancestor that has one.
        loop {
            if let Some(callable) = open.pop_if(|callable| callable.depth == depth) {
                callable.finish(cov, out);
            }
            if cursor.goto_next_sibling() && seek_named(&mut cursor) {
                continue 'visit;
            }
            if !cursor.goto_parent() {
                debug_assert!(open.is_empty());
                return Ok(());
            }
            depth -= 1;
        }
    }
}

/// Move to the first named node among the current node and its following siblings.
fn seek_named(cursor: &mut TreeCursor<'_>) -> bool {
    loop {
        if cursor.node().is_named() {
            return true;
        }
        if !cursor.goto_next_sibling() {
            return false;
        }
    }
}

/// Reference traversal: an outer pre-order walk plus one inner walk per callable.
///
/// It defines the budget semantics, including which limit is reported first, so
/// [`collect`] falls back to it whenever the single pass hits a limit. Tests use it
/// as the oracle for the single pass.
fn collect_reference(
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
    use tree_sitter::{Parser as TsParser, Tree};

    fn csharp() -> Language {
        tree_sitter_c_sharp::LANGUAGE.into()
    }

    fn parse(source: &str) -> Tree {
        let mut parser = TsParser::new();
        parser.set_language(&csharp()).unwrap();
        let tree = parser.parse(source, None).unwrap();
        assert!(
            !tree.root_node().has_error(),
            "invalid test source: {source}"
        );
        tree
    }

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
            &KindTable::new(&csharp()),
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
        let traversal = collect(
            &KindTable::new(&csharp()),
            tree.root_node(),
            source,
            "C.cs",
            Some(&coverage),
            &mut items,
            1_000_000,
            1_000_000,
        )
        .unwrap();
        assert_eq!(traversal, Traversal::SinglePass);
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

    /// Hand-written source that uses every kind named in [`CLASSES`]: every callable
    /// form, nested callables, every decision form, logical and other operators,
    /// comments as extras, and operator spellings inside literals and comments.
    const EDGE: &str = r#"
class C<T> {
    /* c0 */ int P => a && b ? 1 : 2; // c1
    int this[int i, params string[] r] => i ?? 0;
    int Q { get { return x ?? y; } set => z ??= value; }
    int R { get; set; }
    abstract void A();
    public static C<T> operator checked +(C<T> a, C<T> b) => a;
    public static explicit operator checked int(C<T> c) { return 0; }
    ~C() { }
    C() : base() { Action a = delegate { if (x) { } }; Func<int,int> f = x => x switch { 1 => 2, _ => 3, var y when y > 2 => 4 }; }
    void M() {
        switch (x) { case 1: case 2 when q: break; default: break; }
        int L() { Action b = () => { while (p || q) { } }; return 1; }
        try { } catch (E e) when (e != null) { } catch { }
        foreach (var i in xs) for (;;) do { } while (a /* && */ || b);
        var s = "&&" + '|' + (a & b) + (c | d);
        z += 1;
    }
}"#;

    /// Small sources for the budget grid, where limits fall inside nested callables.
    const SMALL: [&str; 5] = [
        "",
        "class C { void F() {} }",
        "class C { void F() { Action a = () => { int x = 1; }; } int G() => 1; }",
        "class C { void F() { if (a && b) { void L() { var f = () => () => 1; } } } }",
        "class C { int P { get => 1; set { } } void G() { switch (x) { case 1: break; } } }",
    ];

    /// Deterministic generator of valid C# with nested callables and decisions.
    struct Generator(u64);

    impl Generator {
        fn below(&mut self, bound: u64) -> u64 {
            lcg_below(&mut self.0, bound)
        }

        fn gap(&mut self) -> &'static str {
            if self.below(3) == 0 {
                "\n"
            } else {
                " "
            }
        }

        fn block(&mut self, depth: u32) -> String {
            let count = self.below(4);
            let mut text = String::new();
            for _ in 0..count {
                text.push_str(&self.statement(depth));
                text.push_str(self.gap());
            }
            text
        }

        fn statement(&mut self, depth: u32) -> String {
            if depth == 0 {
                return ["x++;", "z ??= q;", "var u = a != b;", "var t = c ? a : b;"]
                    [self.below(4) as usize]
                    .to_owned();
            }
            let d = depth - 1;
            let g = self.gap();
            match self.below(18) {
                0 => format!(
                    "if (a && b) {{{g}{}}} else {{ {} }}",
                    self.block(d),
                    self.block(d)
                ),
                1 => format!("if (c) x++; else {{ {} }}", self.block(d)),
                2 => format!("for (int i = 0; i < n; i++) {{{g}{}}}", self.block(d)),
                3 => format!("foreach (var e in xs) {{ {} }}", self.block(d)),
                4 => format!("while (a || b) {{{g}{}}}", self.block(d)),
                5 => format!("do {{ {} }} while (p ?? q);", self.block(d)),
                6 => format!(
                    "try {{ {} }} catch (E e) when (e != null) {{{g}{}}} catch {{ }}",
                    self.block(d),
                    self.block(d)
                ),
                7 => format!(
                    "switch (x) {{ case 1:{g}case 2: {{ {} }} break; default: break; }}",
                    self.block(d)
                ),
                8 => "var s = x switch { 1 => a, var y when y > 2 => b, _ => c };".to_owned(),
                9 => format!("Action a = () => {{{g}{}}};", self.block(d)),
                10 => "Func<int, int> f = y => y ?? 0;".to_owned(),
                11 => format!("Action h = delegate {{ {} }};", self.block(d)),
                12 => format!("int L(int p) {{{g}{} return p; }}", self.block(d)),
                13 => "int K(int p) => p > 0 ? p : -p;".to_owned(),
                14 => format!(
                    "Func<Func<int>> w = () =>{g}() => {{ {} return 1; }};",
                    self.block(d)
                ),
                15 => "// a && b || c\n".to_owned(),
                _ => self.statement(0),
            }
        }

        fn member(&mut self, index: usize) -> String {
            let b = self.block(3);
            let g = self.gap();
            match self.below(10) {
                0 => format!("int P{index} => a && b ? 1 : 2;"),
                1 => "int this[int i] => i ?? 0;".to_owned(),
                2 => format!("int Q{index} {{ get {{{g}{b} return 1; }} set => z ??= value; }}"),
                3 => format!("C() {{{g}{b}}}"),
                4 => format!("~C() {{ {b} }}"),
                5 => format!("public static C operator +(C a, C b) {{{g}{b} return a; }}"),
                6 => "public static explicit operator int(C c) => 0;".to_owned(),
                7 => format!("int R{index} {{ get; set; }}"),
                _ => format!("void M{index}() {{{g}{b}}}"),
            }
        }

        fn program(&mut self) -> String {
            let mut text = String::from("class C {\n");
            for index in 0..1 + self.below(6) as usize {
                text.push_str(&self.member(index));
                text.push('\n');
            }
            text.push('}');
            text
        }
    }

    /// Repository C# files, the hand-written sources and generated programs.
    fn corpus() -> Vec<(String, String)> {
        fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            let mut entries: Vec<_> = std::fs::read_dir(dir)
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .collect();
            entries.sort();
            for path in entries {
                if path.is_dir() {
                    walk(&path, out);
                } else if path.extension().is_some_and(|e| e == "cs") {
                    out.push(path);
                }
            }
        }
        let mut files = Vec::new();
        walk(std::path::Path::new("tests"), &mut files);
        walk(std::path::Path::new("examples"), &mut files);
        assert!(files.len() >= 10, "repository C# fixtures not found");
        let mut sources: Vec<(String, String)> = files
            .iter()
            .map(|p| (p.display().to_string(), std::fs::read_to_string(p).unwrap()))
            .collect();
        sources.push(("<edge>".into(), EDGE.into()));
        sources.extend(
            SMALL
                .iter()
                .map(|s| ("<small>".to_owned(), (*s).to_owned())),
        );
        let mut generator = Generator(0x5eed_cafe_f00d_0001);
        sources.extend((0..150).map(|i| (format!("<generated {i}>"), generator.program())));
        sources
    }

    /// Every named node under `root`, in pre-order.
    fn named_nodes(root: Node<'_>) -> Vec<Node<'_>> {
        let mut cursor = root.walk();
        let mut stack = vec![root];
        let mut nodes = Vec::new();
        while let Some(node) = stack.pop() {
            nodes.push(node);
            let children: Vec<_> = node.named_children(&mut cursor).collect();
            stack.extend(children.into_iter().rev());
        }
        nodes
    }

    /// Run one traversal and render its items and outcome, partial items included.
    fn render(out: &[Item], result: Result<()>) -> String {
        match result {
            Ok(()) => format!("{out:?}"),
            Err(error) => format!("ERR {:?} {error} {out:?}", error.kind()),
        }
    }

    fn sentinel() -> Item {
        Item {
            file: "Earlier.cs".into(),
            name: "Earlier".into(),
            kind: "method_declaration".into(),
            line: 1,
            column: 1,
            end_line: 1,
            complexity: 1,
            coverage: None,
            crap: 1.0,
        }
    }

    /// Compare [`collect`] with the reference on one root and budget pair, starting from
    /// an earlier file's item. Returns the traversal and whether the reference succeeded.
    fn compare(
        kinds: &KindTable,
        root: Node<'_>,
        source: &str,
        cov: Option<&CoverageIndex>,
        budgets: (usize, usize),
        context: &str,
    ) -> (Traversal, bool) {
        let (tb, cb) = budgets;
        let mut want = vec![sentinel()];
        let reference = collect_reference(root, source, "F.cs", cov, &mut want, tb, cb);
        let reference_ok = reference.is_ok();
        let want = render(&want, reference);
        let mut got = vec![sentinel()];
        let result = collect(kinds, root, source, "F.cs", cov, &mut got, tb, cb);
        let traversal = *result.as_ref().unwrap_or(&Traversal::Reference);
        let got = render(&got, result.map(|_| ()));
        assert_eq!(got, want, "{context} tb={tb} cb={cb}");
        (traversal, reference_ok)
    }

    #[test]
    /// The kind table classifies every node of the corpus exactly as the string comparisons do.
    fn kind_table_matches_string_classification_on_every_corpus_node() {
        let kinds = KindTable::new(&csharp());
        for (name, source) in corpus() {
            let tree = parse(&source);
            let mut scratch = tree.walk();
            for node in named_nodes(tree.root_node()) {
                let context = format!("{} at {:?} in {name}", node.kind(), node.start_position());
                assert_eq!(kinds.callable(node), callable(node), "callable: {context}");
                assert_eq!(
                    kinds.decision(node, &mut scratch),
                    decision(node, &source),
                    "decision: {context}"
                );
            }
        }
    }

    #[test]
    /// Every classified name and field exists in the grammar and occurs in the edge source,
    /// so the classification test above exercises every entry of the table.
    fn kind_table_entries_exist_in_the_grammar_and_the_edge_source() {
        let language = csharp();
        let kinds = KindTable::new(&language);
        assert_eq!(kinds.classes.len(), language.node_kind_count());
        assert!(kinds.body.is_some() && kinds.value.is_some() && kinds.operator.is_some());
        let tree = parse(EDGE);
        let mut seen = std::collections::BTreeSet::new();
        let mut cursor = tree.walk();
        let mut stack = vec![tree.root_node()];
        while let Some(node) = stack.pop() {
            seen.insert(node.kind());
            stack.extend(node.children(&mut cursor));
        }
        for &(name, class) in CLASSES {
            let ids: Vec<u16> = (0..language.node_kind_count() as u16)
                .filter(|&id| language.node_kind_for_id(id) == Some(name))
                .collect();
            assert!(!ids.is_empty(), "{name} is not a kind of the grammar");
            for id in ids {
                assert_eq!(kinds.classes[usize::from(id)], class, "{name}");
            }
            assert!(seen.contains(name), "{name} does not occur in EDGE");
        }
        let unclassified = language.id_for_node_kind("identifier", true);
        assert_eq!(kinds.classes[usize::from(unclassified)], Class::Other);
    }

    #[test]
    /// With production budgets the single pass finishes on every corpus source, with and
    /// without coverage, and its items equal the reference's.
    fn single_pass_matches_reference_on_corpus() {
        let kinds = KindTable::new(&csharp());
        let mut callables = 0;
        for (name, source) in corpus() {
            let tree = parse(&source);
            let lines = source.lines().count().max(1);
            let index = CoverageIndex::new((1..=lines).map(|l| (l, (l % 3) as u64)));
            for cov in [None, Some(&index)] {
                let budgets = (1_000_000, 1_000_000);
                let (traversal, ok) =
                    compare(&kinds, tree.root_node(), &source, cov, budgets, &name);
                assert!(ok, "{name}");
                assert_eq!(traversal, Traversal::SinglePass, "{name}");
            }
            callables += named_nodes(tree.root_node())
                .into_iter()
                .filter(|&n| callable(n))
                .count();
        }
        assert!(callables > 1000, "corpus has only {callables} callables");
    }

    #[test]
    /// Started below the tree root, the single pass stays inside the given subtree.
    fn single_pass_matches_reference_on_every_subtree() {
        let kinds = KindTable::new(&csharp());
        let mut generator = Generator(0x0dd_ba11);
        let sources = [EDGE.to_owned(), generator.program(), generator.program()];
        for source in &sources {
            let tree = parse(source);
            let index = CoverageIndex::new((1..=40).map(|l| (l, (l % 2) as u64)));
            for node in named_nodes(tree.root_node()) {
                let context = format!("subtree {} at {:?}", node.kind(), node.start_position());
                let budgets = (1_000_000, 1_000_000);
                let (traversal, _) = compare(&kinds, node, source, Some(&index), budgets, &context);
                assert_eq!(traversal, Traversal::SinglePass, "{context}");
            }
        }
    }

    #[test]
    /// On every budget pair up to 40, the result, error, partial items and limit reported
    /// first equal the reference's, and the reference runs exactly when a limit is hit.
    fn collect_matches_reference_on_budget_boundaries() {
        let kinds = KindTable::new(&csharp());
        let mut generator = Generator(0x00b0_d6e7);
        let mut sources: Vec<String> = SMALL.iter().map(|s| (*s).to_owned()).collect();
        sources.push(EDGE.to_owned());
        sources.extend((0..3).map(|_| generator.program()));
        let (mut fast, mut fallback) = (0, 0);
        for source in &sources {
            let tree = parse(source);
            for tb in 0..=40 {
                for cb in 0..=40 {
                    let context = format!("{source:.60}");
                    let (traversal, ok) =
                        compare(&kinds, tree.root_node(), source, None, (tb, cb), &context);
                    let expected = if ok {
                        Traversal::SinglePass
                    } else {
                        Traversal::Reference
                    };
                    assert_eq!(traversal, expected, "{context} tb={tb} cb={cb}");
                    if ok {
                        fast += 1;
                    } else {
                        fallback += 1;
                    }
                }
            }
        }
        assert!(
            fast > 100 && fallback > 100,
            "fast={fast} fallback={fallback}"
        );
    }

    #[test]
    /// The callable limit fires inside the first callable before the tree limit is reached,
    /// and the fallback reports it, as the reference does.
    fn fallback_reports_the_limit_the_reference_reports_first() {
        let source = "class C { void F() { int a = 1; int b = 2; } void G() {} }";
        let tree = parse(source);
        let kinds = KindTable::new(&csharp());
        let mut items = Vec::new();
        let error = collect(
            &kinds,
            tree.root_node(),
            source,
            "C.cs",
            None,
            &mut items,
            9,
            6,
        )
        .err()
        .unwrap();
        assert_eq!(error.to_string(), "callable resource limit exceeded");
        assert!(items.is_empty());
    }

    #[test]
    /// One table built once serves parsers on several threads by shared reference.
    fn kind_table_is_shared_across_threads() {
        let kinds = KindTable::new(&csharp());
        let mut generator = Generator(0x7ead);
        let sources: Vec<String> = (0..8).map(|_| generator.program()).collect();
        let sequential: Vec<String> = sources
            .iter()
            .map(|source| {
                let tree = parse(source);
                let mut items = Vec::new();
                collect_reference(
                    tree.root_node(),
                    source,
                    "F.cs",
                    None,
                    &mut items,
                    1_000_000,
                    1_000_000,
                )
                .unwrap();
                format!("{items:?}")
            })
            .collect();
        let kinds = &kinds;
        let parallel: Vec<String> = std::thread::scope(|scope| {
            let workers: Vec<_> = sources
                .iter()
                .map(|source| {
                    scope.spawn(move || {
                        let tree = parse(source);
                        let mut items = Vec::new();
                        let traversal = collect(
                            kinds,
                            tree.root_node(),
                            source,
                            "F.cs",
                            None,
                            &mut items,
                            1_000_000,
                            1_000_000,
                        )
                        .unwrap();
                        assert_eq!(traversal, Traversal::SinglePass);
                        format!("{items:?}")
                    })
                })
                .collect();
            workers.into_iter().map(|w| w.join().unwrap()).collect()
        });
        assert_eq!(parallel, sequential);
    }
}
