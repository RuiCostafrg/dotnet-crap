//! Detects conditional-compilation directives that the 0.24 grammar accepts
//! in a position the 0.23.5 grammar rejected.
//!
//! `docs/PREPROCESSOR.md` promises that a file that parses as written is scored
//! as written and that only a file the parser rejects uses the conditional
//! compilation fallback. Grammar 0.23.5 rejected an `#if` that splits a
//! declaration (`int G()` then `#if X { return 1; } #else ; #endif`); 0.24.0
//! accepts it and builds a `method_declaration` with no `body` field. Scoring
//! that shape would silently change results, so [`misplaced_directive`] reports
//! such a file and the caller treats it exactly like a file with a syntax error.
//!
//! # The allowlist
//!
//! A conditional directive (`preproc_if`, `preproc_if_in_attribute_list`) is
//! *expected* only in the places 0.23.5 allowed it. The tables below were
//! derived mechanically from `src/node-types.json` of tree-sitter-c-sharp
//! 0.23.5: for every named node type that is not a supertype, take each
//! `children` slot (unnamed, key `""`) and each `fields` entry (key = field
//! name), expand supertypes (`statement`, `expression`, `declaration`, ...) to
//! their concrete subtypes, and keep `(parent kind, slot)` when the expansion
//! contains the directive kind. The same derivation over 0.24.0 yields a strict
//! superset; the difference, which is deliberately absent from the tables, is
//!
//! * `preproc_if` as an unnamed child of `method_declaration`,
//!   `constructor_declaration`, `destructor_declaration`,
//!   `operator_declaration`, `conversion_operator_declaration`,
//!   `accessor_declaration` and `local_function_statement` (a body or
//!   constructor initializer that differs per branch);
//! * `preproc_if` as an unnamed child of `if_statement` (the `else` lives inside
//!   the block; 0.23.5 allowed only the `condition`, `consequence` and
//!   `alternative` fields) and of `collection_expression`;
//! * `preproc_if_in_attribute_list` in `extension_declaration` and
//!   `receiver_parameter` (constructs 0.23.5 cannot parse at all).
//!
//! Directives that are grammar extras in both versions (`#region`, `#pragma`,
//! `#nullable`, `#line`, `#define`, `#undef`, `#error`, `#warning`) are
//! accepted anywhere and are never checked.
//!
//! Three shape rules complete the picture, because a position can be allowed in
//! both versions while 0.24 accepts a body or a neighbour that 0.23.5 rejected:
//!
//! * A conditional that is an element of an initializer must be delimited like
//!   an expression, with `{` or `,` before it and `,` or `}` after it. 0.24
//!   accepts it between elements with no comma (`{ 1, #if X #endif 2 }`).
//! * A conditional block that directly holds a `,` or `;` token is new in 0.24
//!   (`{ 1, #if X 2, 3, #endif }`, `c ? a : #if X b; #else d; #endif`). In
//!   0.23.5 every block body was a whole node, so a bare separator never sat
//!   inside it.
//! * A directive nested directly in another conditional (`preproc_if`,
//!   `preproc_elif` or `preproc_else` parent) is accepted whenever its
//!   enclosing conditional was: 0.23.5 allowed nesting in every body that
//!   itself held statements, members or expressions, and rejected it in the two
//!   bodies that cannot hold a conditional (an enum member and an attribute
//!   list), where both versions report an error and the fallback runs anyway.
//!   The outermost conditional of a chain is what is checked against the table.
//!
//! Anything not covered here needs the fallback, so an unknown placement can
//! only cost a second parse, never a wrongly shaped score.

use tree_sitter::{Node, TreeCursor};

/// `(parent kind, field name)` slots, sorted, where 0.23.5 allowed `preproc_if`.
/// An empty field name is an unnamed child.
const IF_SLOTS: &[(&str, &str)] = &[
    ("anonymous_object_creation_expression", ""),
    ("argument", ""),
    ("array_rank_specifier", ""),
    ("arrow_expression_clause", ""),
    ("as_expression", "left"),
    ("assignment_expression", "right"),
    ("attribute_argument", ""),
    ("await_expression", ""),
    ("binary_expression", "left"),
    ("binary_expression", "right"),
    ("block", ""),
    ("cast_expression", "value"),
    ("catch_filter_clause", ""),
    ("checked_expression", ""),
    ("compilation_unit", ""),
    ("conditional_access_expression", "condition"),
    ("conditional_expression", "alternative"),
    ("conditional_expression", "condition"),
    ("conditional_expression", "consequence"),
    ("declaration_list", ""),
    ("do_statement", "body"),
    ("do_statement", "condition"),
    ("element_access_expression", "expression"),
    ("enum_member_declaration", "value"),
    ("enum_member_declaration_list", ""),
    ("expression_element", ""),
    ("fixed_statement", ""),
    ("for_statement", "body"),
    ("for_statement", "condition"),
    ("for_statement", "initializer"),
    ("for_statement", "update"),
    ("foreach_statement", "body"),
    ("foreach_statement", "left"),
    ("foreach_statement", "right"),
    ("from_clause", ""),
    ("global_statement", ""),
    ("goto_statement", ""),
    ("group_clause", ""),
    ("if_statement", "alternative"),
    ("if_statement", "condition"),
    ("if_statement", "consequence"),
    ("initializer_expression", ""),
    ("interpolation", ""),
    ("interpolation_alignment_clause", ""),
    ("invocation_expression", "function"),
    ("is_expression", "left"),
    ("is_pattern_expression", "expression"),
    ("join_clause", ""),
    ("labeled_statement", ""),
    ("lambda_expression", "body"),
    ("let_clause", ""),
    ("lock_statement", ""),
    ("makeref_expression", ""),
    ("member_access_expression", "expression"),
    ("order_by_clause", ""),
    ("parameter", ""),
    ("parenthesized_expression", ""),
    ("postfix_unary_expression", ""),
    ("prefix_unary_expression", ""),
    ("preproc_elif", ""),
    ("preproc_else", ""),
    ("preproc_if", ""),
    ("property_declaration", "value"),
    ("range_expression", ""),
    ("ref_expression", ""),
    ("reftype_expression", ""),
    ("refvalue_expression", "value"),
    ("relational_pattern", ""),
    ("return_statement", ""),
    ("select_clause", ""),
    ("spread_element", ""),
    ("subpattern", ""),
    ("switch_expression", ""),
    ("switch_expression_arm", ""),
    ("switch_section", ""),
    ("switch_statement", "value"),
    ("throw_expression", ""),
    ("throw_statement", ""),
    ("using_statement", ""),
    ("using_statement", "body"),
    ("variable_declarator", ""),
    ("when_clause", ""),
    ("where_clause", ""),
    ("while_statement", "body"),
    ("while_statement", "condition"),
    ("with_expression", ""),
    ("with_initializer", ""),
    ("yield_statement", ""),
];

/// Parent kinds, sorted, where 0.23.5 allowed `preproc_if_in_attribute_list`
/// as an unnamed child.
const ATTRIBUTE_LIST_PARENTS: &[&str] = &[
    "accessor_declaration",
    "bracketed_parameter_list",
    "class_declaration",
    "constructor_declaration",
    "conversion_operator_declaration",
    "delegate_declaration",
    "destructor_declaration",
    "enum_declaration",
    "enum_member_declaration",
    "event_declaration",
    "event_field_declaration",
    "field_declaration",
    "indexer_declaration",
    "interface_declaration",
    "lambda_expression",
    "local_function_statement",
    "method_declaration",
    "operator_declaration",
    "parameter",
    "parameter_list",
    "property_declaration",
    "record_declaration",
    "struct_declaration",
    "type_parameter",
];

/// Whether `source` holds a conditional directive that a 0.24 tree accepts in a
/// position 0.23.5 did not. A tree that has errors is not a candidate; the
/// caller already sends it to the fallback.
///
/// One pass over the tree, skipped entirely when the text has no `#` byte.
pub(crate) fn misplaced_directive(root: Node<'_>, source: &str) -> bool {
    if !source.as_bytes().contains(&b'#') {
        return false;
    }
    let mut cursor = root.walk();
    loop {
        let node = cursor.node();
        let kind = node.kind();
        if kind.starts_with("preproc_") {
            if is_conditional(kind) {
                if misplaced(&cursor) {
                    return true;
                }
            } else {
                // `#region`, `#pragma`, ... hold only their argument.
                if !advance_past(&mut cursor) {
                    return false;
                }
                continue;
            }
        }
        if cursor.goto_first_child() {
            continue;
        }
        if !advance_past(&mut cursor) {
            return false;
        }
    }
}

/// Move to the next sibling, climbing as needed. `false` at the end of the tree.
fn advance_past(cursor: &mut TreeCursor<'_>) -> bool {
    loop {
        if cursor.goto_next_sibling() {
            return true;
        }
        if !cursor.goto_parent() {
            return false;
        }
    }
}

fn is_conditional(kind: &str) -> bool {
    matches!(
        kind,
        "preproc_if" | "preproc_elif" | "preproc_else" | "preproc_if_in_attribute_list"
    )
}

/// Whether the conditional the cursor is on sits in a slot, or holds a body,
/// that 0.23.5 rejected.
fn misplaced(cursor: &TreeCursor<'_>) -> bool {
    let node = cursor.node();
    let field = cursor.field_name().unwrap_or("");
    let Some(parent) = node.parent() else {
        return true;
    };
    let parent_kind = parent.kind();
    let slot_ok = match node.kind() {
        // Branches hang off their conditional through `alternative`.
        "preproc_elif" | "preproc_else" => field == "alternative" && is_conditional(parent_kind),
        "preproc_if_in_attribute_list" => {
            field.is_empty() && ATTRIBUTE_LIST_PARENTS.binary_search(&parent_kind).is_ok()
        }
        // A nested conditional is judged by the one enclosing it.
        _ if matches!(parent_kind, "preproc_if" | "preproc_elif" | "preproc_else") => true,
        _ => IF_SLOTS.binary_search(&(parent_kind, field)).is_ok(),
    };
    !slot_ok
        || holds_separator(node)
        || (parent_kind == "initializer_expression" && !separated_element(node))
}

/// Whether a conditional that is an element of an initializer is delimited like
/// an expression: `{` or `,` before it and `,` or `}` after it, comments aside.
/// 0.23.5 needed the commas outside the conditional; 0.24.0 lets it stand
/// between elements without them (`{ 1, #if X #endif 2 }`).
fn separated_element(node: Node<'_>) -> bool {
    let mut before = node.prev_sibling();
    while let Some(s) = before.filter(|s| s.kind() == "comment") {
        before = s.prev_sibling();
    }
    let mut after = node.next_sibling();
    while let Some(s) = after.filter(|s| s.kind() == "comment") {
        after = s.next_sibling();
    }
    before.is_some_and(|s| matches!(s.kind(), "{" | ","))
        && after.is_some_and(|s| matches!(s.kind(), "," | "}"))
}

/// Whether a bare `,` or `;` is a direct child of the conditional.
fn holds_separator(node: Node<'_>) -> bool {
    (0..node.child_count())
        .filter_map(|i| node.child(i))
        .any(|c| !c.is_named() && matches!(c.kind(), "," | ";"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tree_sitter::Parser;

    fn tree(source: &str) -> tree_sitter::Tree {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_c_sharp::LANGUAGE.into())
            .expect("C# grammar");
        parser.parse(source, None).expect("tree")
    }

    /// Run the check on a tree that must be free of syntax errors.
    fn misplaced_in(source: &str) -> bool {
        let tree = tree(source);
        assert!(
            !tree.root_node().has_error(),
            "fixture must parse: {source}"
        );
        misplaced_directive(tree.root_node(), source)
    }

    #[test]
    fn tables_are_sorted_for_binary_search() {
        assert!(IF_SLOTS.windows(2).all(|w| w[0] < w[1]));
        assert!(ATTRIBUTE_LIST_PARENTS.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn tables_exclude_the_parents_only_024_allows() {
        for parent in [
            "method_declaration",
            "constructor_declaration",
            "destructor_declaration",
            "operator_declaration",
            "conversion_operator_declaration",
            "accessor_declaration",
            "local_function_statement",
            "collection_expression",
        ] {
            assert!(
                !IF_SLOTS.iter().any(|&(p, _)| p == parent),
                "{parent} must not be allowed"
            );
        }
        // `if_statement` is allowed through its fields only.
        assert!(IF_SLOTS
            .iter()
            .any(|&(p, f)| p == "if_statement" && f == "alternative"));
        assert!(!IF_SLOTS.contains(&("if_statement", "")));
        assert!(!ATTRIBUTE_LIST_PARENTS.contains(&"extension_declaration"));
        assert!(!ATTRIBUTE_LIST_PARENTS.contains(&"receiver_parameter"));
    }

    #[test]
    fn a_split_method_body_is_misplaced() {
        assert!(misplaced_in(
            "class B {\n int G()\n#if X\n { return 1; }\n#else\n ;\n#endif\n}\n"
        ));
    }

    /// Each of these parses with grammar 0.24.0 and was a syntax error with
    /// 0.23.5 (checked against both when the table was written).
    #[test]
    fn declarations_and_bodies_that_0235_rejected_are_misplaced() {
        for source in [
            // A local function, an accessor, a destructor and an operator whose
            // body differs per branch.
            "class B { void F() {\n void L()\n#if X\n { }\n#else\n ;\n#endif\n} }\n",
            "class B { int P { get\n#if X\n { return 1; }\n#else\n ;\n#endif\n } }\n",
            "class B { ~B()\n#if X\n { }\n#else\n ;\n#endif\n}\n",
            "class B { public static B operator +(B x, B y)\n#if X\n { return x; }\n#else\n => y;\n#endif\n}\n",
            "class B { void M()\n#if A\n { }\n#else\n => x();\n#endif\n}\n",
            // The `;` of a conditional's last operand sits inside each branch.
            "class B { int F(bool c, int a, int d) {\n return c ? a :\n#if X\n a;\n#else\n d;\n#endif\n} }\n",
            // An empty conditional between initializer elements, no comma around it.
            "class B { int[] F() { return new int[] { 1,\n#if X\n#endif\n 2, 3 }; } }\n",
            // An `else` branch that lives inside the conditional.
            "class B { void F() { if (a) { b(); }\n#if X\n else if (c) { d(); }\n#endif\n else { e(); } } }\n",
        ] {
            assert!(misplaced_in(source), "{source}");
        }
    }

    #[test]
    fn an_else_inside_a_conditional_after_an_if_statement_is_misplaced() {
        assert!(misplaced_in(
            "class B { void F(int x) {\n if (x > 0) { a(); }\n#if X\n else { b(); }\n#endif\n} }\n"
        ));
    }

    #[test]
    fn a_conditional_collection_or_initializer_element_list_is_misplaced() {
        assert!(misplaced_in(
            "class B { int[] F() { int[] a = [\n#if X\n 1, 2,\n#endif\n 3 ]; return a; } }\n"
        ));
        assert!(misplaced_in(
            "class B { int[] F() { return new int[] {\n#if X\n 1,\n 2,\n#endif\n 3 }; } }\n"
        ));
    }

    #[test]
    fn a_conditional_around_whole_members_is_not_misplaced() {
        assert!(!misplaced_in(
            "class B {\n#if X\n int G() { return 1; }\n#else\n int G() { return 2; }\n#endif\n}\n"
        ));
        assert!(!misplaced_in(
            "#if X\nusing System;\n#endif\nnamespace N {\n#if X\n class C { }\n#endif\n}\n"
        ));
    }

    #[test]
    fn a_conditional_around_whole_statements_is_not_misplaced() {
        assert!(!misplaced_in(
            "class B { void F(int x) {\n#if X\n a();\n#elif Y\n b();\n#else\n c();\n#endif\n if (x > 0) a();\n} }\n"
        ));
        // `else` outside the conditional: the conditional is the alternative.
        assert!(!misplaced_in(
            "class B { void F(int x) {\n if (x > 0) { a(); }\n else\n#if X\n { b(); }\n#else\n { c(); }\n#endif\n} }\n"
        ));
    }

    #[test]
    fn a_conditional_expression_or_single_element_is_not_misplaced() {
        assert!(!misplaced_in(
            "class B { int F() =>\n#if X\n 1\n#else\n 2\n#endif\n ; }\n"
        ));
        assert!(!misplaced_in(
            "class B { int[] F() { return new int[] {\n 1,\n#if X\n 2\n#endif\n }; } }\n"
        ));
    }

    #[test]
    fn nested_conditionals_follow_their_enclosing_one() {
        assert!(!misplaced_in(
            "class B {\n#if X\n#if Y\n int G() { return 1; }\n#endif\n int H() { return 2; }\n#endif\n}\n"
        ));
        // A split declaration stays misplaced when it is itself nested.
        assert!(misplaced_in(
            "class B {\n#if X\n int G()\n#if Y\n { return 1; }\n#else\n ;\n#endif\n#endif\n}\n"
        ));
    }

    #[test]
    fn attribute_list_conditionals_on_whole_declarations_are_not_misplaced() {
        assert!(!misplaced_in(
            "class B {\n#if X\n [Obsolete]\n#endif\n int G() { return 1; }\n}\n"
        ));
    }

    #[test]
    fn extras_are_accepted_anywhere() {
        assert!(!misplaced_in(
            "class B {\n#region R\n int G()\n#pragma warning disable 1\n { return 1; }\n#endregion\n}\n"
        ));
    }

    #[test]
    fn resolving_leaves_no_directive_for_the_second_parse() {
        // `resolve` blanks every directive line it reads as code, so the tree
        // of the resolved text needs no second placement check.
        let source = "class B {\n int G()\n#if X\n { return 1; }\n#else\n ;\n#endif\n#region R\n#endregion\n}\n";
        let resolved = crate::preprocess::resolve(source, &Default::default()).expect("resolves");
        assert!(!resolved.contains('#'));
        assert!(!misplaced_in(&resolved));
    }

    #[test]
    fn a_source_without_hash_skips_the_walk() {
        let source = "class B { int G() { return 1; } }";
        assert!(!misplaced_directive(tree(source).root_node(), source));
        // Even a tree that would be flagged is not looked at without a `#`.
        let flagged = "class B {\n int G()\n#if X\n { return 1; }\n#else\n ;\n#endif\n}\n";
        assert!(misplaced_directive(tree(flagged).root_node(), flagged));
        assert!(!misplaced_directive(
            tree(flagged).root_node(),
            "no directive"
        ));
    }
}
