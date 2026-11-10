//! Syntax traversal and CRAP scoring of C# callables.

use crate::Item;
use anyhow::{bail, Context, Result};
use std::collections::BTreeMap;
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
/// Collect callable scores iteratively with bounded traversal and merged nested-coverage exclusions.
/// The caller supplies independent whole-tree and per-callable named-node budgets.
pub(crate) fn collect(
    n: Node<'_>,
    source: &str,
    file: &str,
    cov: Option<&BTreeMap<usize, u64>>,
    out: &mut Vec<Item>,
    traversal_budget: usize,
    callable_budget: usize,
) -> Result<()> {
    let mut nodes = vec![n];
    let mut visited = 0usize;
    while let Some(n) = nodes.pop() {
        visited += 1;
        if visited > traversal_budget {
            bail!("syntax tree resource limit exceeded");
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
                bail!("callable resource limit exceeded");
            }
            if !outer && callable(node) {
                excluded.push((node.start_position().row + 1, node.end_position().row + 1));
                continue;
            }
            complexity = complexity
                .checked_add(decision(node, source))
                .context("complexity overflow")?;
            let mut cursor = node.walk();
            stack.extend(node.named_children(&mut cursor).map(|child| (child, false)));
        }
        // Exclude nested spans as intervals, never one allocation per source
        // line. Long overlapping local-function spans must not expand work.
        excluded.sort_unstable();
        let mut ranges: Vec<(usize, usize)> = Vec::new();
        for (start, end) in excluded {
            if let Some(last) = ranges.last_mut() {
                if start <= last.1 {
                    last.1 = last.1.max(end);
                    continue;
                }
            }
            ranges.push((start, end));
        }
        let coverage = cov.and_then(|c| {
            let mut total = 0usize;
            let mut hit = 0usize;
            for (&line, &hits) in c.range((n.start_position().row + 1)..=(n.end_position().row + 1))
            {
                let position = ranges.partition_point(|&(start, _)| start <= line);
                if position == 0 || ranges[position - 1].1 < line {
                    total += 1;
                    hit += usize::from(hits > 0);
                }
            }
            (total > 0).then(|| hit as f64 / total as f64)
        });
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
        parser.set_language(&tree_sitter_c_sharp::LANGUAGE.into())?;
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
}
