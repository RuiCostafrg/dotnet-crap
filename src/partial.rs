//! Partial scoring of a file that does not parse: keep only the callables
//! whose syntax is intact.

use crate::report::Item;
use std::collections::HashSet;
use tree_sitter::{Node, Point};

/// The tree node an item was scored from: the node of the item's kind that
/// starts at the item's position.
fn node_of<'t>(root: Node<'t>, item: &Item) -> Option<Node<'t>> {
    let start = Point::new(item.line.checked_sub(1)?, item.column.checked_sub(1)?);
    let mut node = root.descendant_for_point_range(start, start)?;
    loop {
        if node.start_position() == start && node.kind() == item.kind {
            return Some(node);
        }
        node = node.parent()?;
    }
}

/// Split the items scored from an erroneous tree into the callables whose
/// syntax is intact, in their original order, and the number dropped.
///
/// A callable is kept only when its node contains no error or missing node,
/// holds no misplaced conditional directive, is not inside an error node, and
/// is not nested in a callable that contains an error. Anything that cannot
/// be traced back to its node is dropped.
pub(crate) fn keep_intact(root: Node<'_>, source: &str, items: Vec<Item>) -> (Vec<Item>, usize) {
    let callables: HashSet<(usize, usize, String)> = items
        .iter()
        .map(|i| (i.line, i.column, i.kind.clone()))
        .collect();
    let is_callable = |n: Node<'_>| {
        let p = n.start_position();
        callables.contains(&(p.row + 1, p.column + 1, n.kind().to_owned()))
    };
    let intact = |item: &Item| {
        let Some(node) = node_of(root, item) else {
            return false;
        };
        if node.has_error() || crate::placement::misplaced_directive(node, source) {
            return false;
        }
        let mut ancestor = node.parent();
        while let Some(a) = ancestor {
            if a.is_error() || a.has_error() && is_callable(a) {
                return false;
            }
            ancestor = a.parent();
        }
        true
    };
    let total = items.len();
    let kept: Vec<Item> = items.into_iter().filter(|i| intact(i)).collect();
    let dropped = total - kept.len();
    (kept, dropped)
}

#[cfg(test)]
mod tests {
    use super::keep_intact;

    /// Names of the callables kept from `source`, and the number dropped.
    fn intact(source: &str) -> (Vec<String>, usize) {
        let language: tree_sitter::Language = tree_sitter_c_sharp::LANGUAGE.into();
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&language).unwrap();
        let tree = parser.parse(source, None).unwrap();
        assert!(tree.root_node().has_error(), "the sample must not parse");
        let kinds = crate::analysis::KindTable::new(&language);
        let mut items = Vec::new();
        crate::analysis::collect(
            &kinds,
            tree.root_node(),
            source,
            "a.cs",
            None,
            &mut items,
            1_000_000,
            1_000_000,
        )
        .unwrap();
        let (kept, dropped) = keep_intact(tree.root_node(), source, items);
        (kept.into_iter().map(|i| i.name).collect(), dropped)
    }

    #[test]
    fn a_callable_with_an_error_and_everything_nested_in_it_is_dropped() {
        let (kept, dropped) = intact(
            "class K {\n int Good() { return 1; }\n int Bad(int x) { if (x > 0 return 1; return 0; }\n\
             int Outer() {\n int Local() { return 1; }\n var z = ) ;\n return Local();\n }\n\
             int AlsoGood() { return 2; }\n}\n",
        );
        assert_eq!(kept, ["Good", "AlsoGood"]);
        assert_eq!(dropped, 3);
    }

    #[test]
    fn errors_outside_every_callable_drop_nothing() {
        let (kept, dropped) =
            intact("class K {\n int A() { return 1; }\n ) ) ]\n int B() { return 2; }\n}\n");
        assert_eq!(kept, ["A", "B"]);
        assert_eq!(dropped, 0);
    }

    #[test]
    fn callables_inside_an_error_node_are_dropped() {
        // Without its closing brace the class is recovered as an error node.
        let (kept, dropped) = intact("class K {\n int A() { return 1; }\n int B() { return 2; }\n");
        assert!(kept.is_empty(), "{kept:?}");
        assert_eq!(dropped, 2);
    }

    #[test]
    fn a_callable_with_a_misplaced_directive_is_dropped() {
        // Scored as written, F would count both branches of the `#if`.
        let (kept, dropped) = intact(
            "class B {\n int Ok() { return 1; }\n int Bad() { return ) ; }\n\
             int F(bool c, int a, int d) {\n return c ? a :\n#if X\n a;\n#else\n d;\n#endif\n}\n}\n",
        );
        assert_eq!(kept, ["Ok"]);
        assert_eq!(dropped, 2);
    }
}
