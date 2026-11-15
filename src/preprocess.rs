//! Conditional-compilation fallback for sources that do not parse as written.
//!
//! The C# grammar accepts preprocessor directives only around whole members or
//! statements, and rejects some valid placements (an `#if` that splits a
//! declaration, a directive on the last line without a line break, file-based
//! app `#:` lines). For a file whose tree has errors, [`resolve`] produces a
//! same-length text in which every directive line and every inactive
//! conditional branch is replaced by ASCII spaces. Every `\n` and `\r` is kept
//! and every other byte of a blanked line becomes one space, so byte offsets,
//! line numbers and columns of the kept text are unchanged, and the result is
//! valid UTF-8 because whole lines are replaced. Files that parse as written
//! never reach this module.
//!
//! Directives are recognised per line, as the first non-whitespace text of the
//! line. A `#` line inside a multi-line comment or string is therefore treated
//! as a directive too; when that changes the meaning, the reparse usually still
//! fails and the scan reports the original syntax error.

use std::collections::BTreeSet;

/// Deepest nesting of parentheses and `!` accepted in one `#if` expression.
/// Keeps the recursive evaluator well within any worker thread's stack.
const MAX_EXPRESSION_DEPTH: usize = 64;

/// Return whether `name` is accepted as a conditional compilation symbol: a
/// letter or `_`, then letters, digits or `_`, and neither `true` nor `false`.
pub(crate) fn is_symbol(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c == '_' || c.is_alphabetic())
        && chars.all(|c| c == '_' || c.is_alphanumeric())
        && name != "true"
        && name != "false"
}

/// One open `#if` group.
struct Group {
    /// Whether the text around the group is active.
    parent_active: bool,
    /// Whether a branch of the group has already been taken.
    taken: bool,
    /// Whether the current branch is active.
    active: bool,
    /// Whether `#else` has been seen; a later `#elif` or `#else` is malformed.
    seen_else: bool,
}

/// Resolve conditional compilation in `text` with `defines` as the initial
/// symbol set, honouring `#define` and `#undef` in order.
///
/// Returns `None` when a directive is malformed (unbalanced groups, an invalid
/// expression or symbol, an unknown directive in active text) or when no line
/// would change, so the caller keeps the original parse result.
pub(crate) fn resolve(text: &str, defines: &BTreeSet<String>) -> Option<String> {
    let mut symbols: BTreeSet<&str> = defines.iter().map(String::as_str).collect();
    let mut groups: Vec<Group> = Vec::new();
    let mut out = text.as_bytes().to_vec();
    let mut start = 0;
    for (index, line) in text.split('\n').enumerate() {
        let end = start + line.len();
        let active = groups.last().is_none_or(|g| g.active);
        let blank = match directive(line, index) {
            None => !active,
            Some(Directive::Plain) => true,
            Some(Directive::Named(name, rest)) => {
                apply(name, rest, active, &mut groups, &mut symbols)?;
                true
            }
        };
        if blank {
            for byte in &mut out[start..end] {
                if *byte != b'\r' {
                    *byte = b' ';
                }
            }
        }
        start = end + 1;
    }
    if !groups.is_empty() || out == text.as_bytes() {
        return None;
    }
    Some(String::from_utf8(out).expect("whole lines of UTF-8 replaced by ASCII"))
}

/// A directive line, as recognised by [`directive`].
enum Directive<'a> {
    /// A file-based app line (`#:`) or a first-line `#!`, blanked without effect.
    Plain,
    /// A named directive and the text after its name.
    Named(&'a str, &'a str),
}

/// Whitespace that may precede or separate directive parts, including the
/// `\r` of a CRLF line break (lines are split on `\n`).
fn is_space(c: char) -> bool {
    c.is_whitespace() && c != '\n'
}

/// Length of the leading run of letters, digits and `_`.
fn word_len(text: &str) -> usize {
    text.find(|c: char| c != '_' && !c.is_alphanumeric())
        .unwrap_or(text.len())
}

/// Classify one line (without its `\n`); `index` is its zero-based line number.
fn directive(line: &str, index: usize) -> Option<Directive<'_>> {
    let body = line.trim_start_matches(is_space).strip_prefix('#')?;
    if body.starts_with(':') || index == 0 && line.starts_with("#!") {
        return Some(Directive::Plain);
    }
    let body = body.trim_start_matches(is_space);
    let len = word_len(body);
    Some(Directive::Named(&body[..len], &body[len..]))
}

/// Apply one named directive to the group stack and symbol set.
fn apply<'a>(
    name: &str,
    rest: &'a str,
    active: bool,
    groups: &mut Vec<Group>,
    symbols: &mut BTreeSet<&'a str>,
) -> Option<()> {
    match name {
        "if" => {
            let taken = !active || evaluate(rest, symbols)?;
            groups.push(Group {
                parent_active: active,
                taken,
                active: active && taken,
                seen_else: false,
            });
        }
        "elif" => {
            let group = groups.last_mut().filter(|g| !g.seen_else)?;
            let value = !group.parent_active || evaluate(rest, symbols)?;
            group.active = group.parent_active && !group.taken && value;
            group.taken |= value;
        }
        "else" => {
            let group = groups.last_mut().filter(|g| !g.seen_else)?;
            ends_cleanly(rest)?;
            group.active = group.parent_active && !group.taken;
            group.taken = true;
            group.seen_else = true;
        }
        "endif" => {
            ends_cleanly(rest)?;
            groups.pop()?;
        }
        "define" | "undef" if active => {
            let symbol = strip_comment(rest).trim_matches(is_space);
            if !is_symbol(symbol) {
                return None;
            }
            if name == "define" {
                symbols.insert(symbol);
            } else {
                symbols.remove(symbol);
            }
        }
        "region" | "endregion" | "pragma" | "nullable" | "line" | "warning" | "error" => {}
        _ if active => return None,
        _ => {}
    }
    Some(())
}

/// Remove a trailing `//` comment.
fn strip_comment(text: &str) -> &str {
    text.find("//").map_or(text, |i| &text[..i])
}

/// Accept only whitespace and an optional `//` comment after a directive name.
fn ends_cleanly(rest: &str) -> Option<()> {
    strip_comment(rest)
        .trim_matches(is_space)
        .is_empty()
        .then_some(())
}

/// A token of a `#if` or `#elif` expression.
#[derive(Debug, PartialEq)]
enum Token<'a> {
    Symbol(&'a str),
    Literal(bool),
    Not,
    And,
    Or,
    Equal,
    NotEqual,
    Open,
    Close,
}

/// Split an expression into tokens, stopping at a `//` comment.
fn tokenize(text: &str) -> Option<Vec<Token<'_>>> {
    let mut tokens = Vec::new();
    let mut rest = text;
    loop {
        rest = rest.trim_start_matches(is_space);
        if rest.is_empty() || rest.starts_with("//") {
            return Some(tokens);
        }
        let first = rest.chars().next().expect("nonempty after the check above");
        let (token, len) = if first == '_' || first.is_alphabetic() {
            let len = word_len(rest);
            let token = match &rest[..len] {
                "true" => Token::Literal(true),
                "false" => Token::Literal(false),
                word => Token::Symbol(word),
            };
            (token, len)
        } else {
            [
                ("&&", Token::And),
                ("||", Token::Or),
                ("==", Token::Equal),
                ("!=", Token::NotEqual),
                ("!", Token::Not),
                ("(", Token::Open),
                (")", Token::Close),
            ]
            .into_iter()
            .find(|(text, _)| rest.starts_with(text))
            .map(|(text, token)| (token, text.len()))?
        };
        tokens.push(token);
        rest = &rest[len..];
    }
}

/// Evaluate a conditional expression with C# precedence: `!`, then `==` and
/// `!=`, then `&&`, then `||`, all binary operators left-associative.
fn evaluate(text: &str, symbols: &BTreeSet<&str>) -> Option<bool> {
    let tokens = tokenize(text)?;
    let mut parser = Expression {
        tokens: &tokens,
        position: 0,
        symbols,
    };
    let value = parser.or(0)?;
    (parser.position == tokens.len()).then_some(value)
}

/// Recursive-descent state over one expression's tokens.
struct Expression<'t, 'a> {
    tokens: &'t [Token<'a>],
    position: usize,
    symbols: &'t BTreeSet<&'a str>,
}

impl Expression<'_, '_> {
    /// Consume the next token when it equals `token`.
    fn eat(&mut self, token: &Token<'_>) -> bool {
        let matched = self.tokens.get(self.position) == Some(token);
        self.position += usize::from(matched);
        matched
    }

    fn or(&mut self, depth: usize) -> Option<bool> {
        let mut value = self.and(depth)?;
        while self.eat(&Token::Or) {
            value |= self.and(depth)?;
        }
        Some(value)
    }

    fn and(&mut self, depth: usize) -> Option<bool> {
        let mut value = self.equality(depth)?;
        while self.eat(&Token::And) {
            value &= self.equality(depth)?;
        }
        Some(value)
    }

    fn equality(&mut self, depth: usize) -> Option<bool> {
        let mut value = self.unary(depth)?;
        loop {
            if self.eat(&Token::Equal) {
                value = value == self.unary(depth)?;
            } else if self.eat(&Token::NotEqual) {
                value = value != self.unary(depth)?;
            } else {
                return Some(value);
            }
        }
    }

    fn unary(&mut self, depth: usize) -> Option<bool> {
        if depth >= MAX_EXPRESSION_DEPTH {
            return None;
        }
        if self.eat(&Token::Not) {
            return self.unary(depth + 1).map(|v| !v);
        }
        if self.eat(&Token::Open) {
            let value = self.or(depth + 1)?;
            return self.eat(&Token::Close).then_some(value);
        }
        let value = match self.tokens.get(self.position)? {
            Token::Literal(value) => *value,
            Token::Symbol(name) => self.symbols.contains(name),
            _ => return None,
        };
        self.position += 1;
        Some(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|s| (*s).to_owned()).collect()
    }

    /// Resolve with `names` defined; panics when the fallback declines.
    fn kept(text: &str, names: &[&str]) -> String {
        let out = resolve(text, &set(names)).expect("resolvable");
        assert_same_shape(text, &out);
        out
    }

    /// Lines of `text` that are not entirely blank, trimmed of trailing spaces.
    fn visible(text: &str, names: &[&str]) -> Vec<String> {
        kept(text, names)
            .split('\n')
            .map(|l| l.trim_end_matches([' ', '\r']).to_owned())
            .filter(|l| !l.is_empty())
            .collect()
    }

    /// Same byte length, the same `\n`/`\r` positions, and every other byte
    /// either unchanged or an ASCII space; blanking is by whole lines.
    fn assert_same_shape(input: &str, output: &str) {
        let (a, b) = (input.as_bytes(), output.as_bytes());
        assert_eq!(a.len(), b.len());
        for (x, y) in a.iter().zip(b) {
            if matches!(x, b'\n' | b'\r') {
                assert_eq!(x, y);
            } else {
                assert!(x == y || *y == b' ', "{x} -> {y}");
            }
        }
        for (line_in, line_out) in input.split('\n').zip(output.split('\n')) {
            assert!(
                line_in == line_out || line_out.bytes().all(|c| c == b' ' || c == b'\r'),
                "{line_in:?} -> {line_out:?}"
            );
        }
    }

    fn eval(text: &str, names: &[&str]) -> Option<bool> {
        let names: BTreeSet<&str> = names.iter().copied().collect();
        evaluate(text, &names)
    }

    #[test]
    fn symbols_follow_identifier_rules() {
        for good in [
            "A",
            "_",
            "_x1",
            "DEBUG",
            "NET8_0_OR_GREATER",
            "é",
            "if",
            "Truex",
        ] {
            assert!(is_symbol(good), "{good}");
        }
        for bad in [
            "", "1A", "A-B", "A B", "true", "false", "@A", "A.B", " A", "A,B",
        ] {
            assert!(!is_symbol(bad), "{bad}");
        }
    }

    #[test]
    fn expressions_follow_csharp_precedence() {
        let cases: &[(&str, &[&str], bool)] = &[
            ("A", &["A"], true),
            ("A", &[], false),
            ("true", &[], true),
            ("false", &["false"], false),
            ("!A", &[], true),
            ("!!A", &[], false),
            ("A || B && C", &["A"], true),
            ("(A || B) && C", &["A"], false),
            ("A && B || C", &["C"], true),
            ("A && (B || C)", &["C"], false),
            ("!A == B", &[], false),
            ("!(A == B)", &[], false),
            ("A == B && C", &["C"], true),
            ("A == (B && C)", &["C"], true),
            ("A != B", &["B"], true),
            ("A != B", &["A", "B"], false),
            ("A == true", &["A"], true),
            ("A && B", &["A", "B"], true),
            ("A && B", &["A"], false),
            ("A || B", &[], false),
            ("A || B", &["B"], true),
            ("(A)", &["A"], true),
            ("  A\t&&B ", &["A", "B"], true),
            ("A // B && false", &["A"], true),
            ("A//comment", &["A"], true),
            ("NET8_0 || é", &["é"], true),
        ];
        for (text, names, expected) in cases {
            assert_eq!(eval(text, names), Some(*expected), "{text} with {names:?}");
        }
    }

    #[test]
    fn malformed_expressions_are_rejected() {
        for text in [
            "", "// x", "A B", "A &&", "&& A", "(A", "A)", "()", "&A", "A = B", "A | B", "!", "1",
            "A.B", "A ==", "A !", "#A",
        ] {
            assert_eq!(eval(text, &["A", "B"]), None, "{text:?}");
        }
    }

    #[test]
    fn expression_depth_is_bounded() {
        let nested = |n: usize| format!("{}A{}", "(".repeat(n), ")".repeat(n));
        assert_eq!(eval(&nested(MAX_EXPRESSION_DEPTH - 1), &["A"]), Some(true));
        assert_eq!(eval(&nested(MAX_EXPRESSION_DEPTH), &["A"]), None);
        let nots = |n: usize| format!("{}A", "!".repeat(n));
        assert_eq!(eval(&nots(MAX_EXPRESSION_DEPTH - 1), &[]), Some(true));
        assert_eq!(eval(&nots(MAX_EXPRESSION_DEPTH), &[]), None);
        assert_eq!(eval(&nested(100_000), &["A"]), None);
    }

    #[test]
    fn tokens_cover_every_operator() {
        assert_eq!(
            tokenize("!a&&b||c==d!=(true)false").unwrap(),
            [
                Token::Not,
                Token::Symbol("a"),
                Token::And,
                Token::Symbol("b"),
                Token::Or,
                Token::Symbol("c"),
                Token::Equal,
                Token::Symbol("d"),
                Token::NotEqual,
                Token::Open,
                Token::Literal(true),
                Token::Close,
                Token::Literal(false),
            ]
        );
    }

    const SPLIT: &str = "class C {\n    Task OnEmptyBatchAsync()\n#if X\n    {\n        return Task.CompletedTask;\n    }\n#else\n    ;\n#endif\n}\n";

    #[test]
    fn if_else_selects_one_branch() {
        assert_eq!(
            visible(SPLIT, &[]),
            ["class C {", "    Task OnEmptyBatchAsync()", "    ;", "}"]
        );
        assert_eq!(
            visible(SPLIT, &["X"]),
            [
                "class C {",
                "    Task OnEmptyBatchAsync()",
                "    {",
                "        return Task.CompletedTask;",
                "    }",
                "}"
            ]
        );
    }

    const CHAIN: &str = "#if A\na\n#elif B\nb\n#elif C\nc\n#else\nd\n#endif\ne";

    #[test]
    fn elif_chains_take_only_the_first_true_branch() {
        assert_eq!(visible(CHAIN, &[]), ["d", "e"]);
        assert_eq!(visible(CHAIN, &["A"]), ["a", "e"]);
        assert_eq!(visible(CHAIN, &["B"]), ["b", "e"]);
        assert_eq!(visible(CHAIN, &["C"]), ["c", "e"]);
        assert_eq!(visible(CHAIN, &["A", "B", "C"]), ["a", "e"]);
        assert_eq!(visible(CHAIN, &["B", "C"]), ["b", "e"]);
        assert_eq!(
            visible("#if A\na\n#elif B\nb\n#endif\n", &["A", "B"]),
            ["a"]
        );
        assert_eq!(
            visible("#if A\na\n#elif B\nb\n#endif\n", &[]),
            Vec::<String>::new()
        );
    }

    #[test]
    fn nested_groups_inherit_inactive_parents() {
        let text = "#if A\n#if B\nab\n#elif C\nac\n#else\na\n#endif\n#else\n#if B\nb\n#else\nn\n#endif\n#endif\n";
        assert_eq!(visible(text, &["A", "B"]), ["ab"]);
        assert_eq!(visible(text, &["A", "C"]), ["ac"]);
        assert_eq!(visible(text, &["A"]), ["a"]);
        assert_eq!(visible(text, &["B"]), ["b"]);
        assert_eq!(visible(text, &["C"]), ["n"]);
        // Expressions and unknown directives inside inactive text are not evaluated.
        let skipped = "#if false\n#if &&\nx\n#elif ((\ny\n#endif\n#unknown\n#define 1\n#endif\nz";
        assert_eq!(visible(skipped, &[]), ["z"]);
    }

    #[test]
    fn define_and_undef_apply_in_order_in_active_text() {
        let text = "#define A // set\n#if A\na\n#endif\n#undef A\n#if A\nb\n#endif\n#if false\n#define B\n#endif\n#if B\nc\n#endif\n#undef Z\n";
        assert_eq!(visible(text, &[]), ["a"]);
        assert_eq!(
            visible("#undef A\n#if A\na\n#endif\n", &["A"]),
            Vec::<String>::new()
        );
        assert_eq!(visible("#  define\tA\n  #  if A\na\n #endif", &[]), ["a"]);
    }

    #[test]
    fn other_directives_and_app_lines_are_blanked() {
        let text = "#!/usr/bin/env dotnet\n#:sdk Microsoft.NET.Sdk\n  #:package A@1\n#region R\n#pragma warning disable 1\n#nullable enable\n#line 5\n#line default\n#warning w\n#error e\n#endregion\nclass C { }\n#pragma warning restore 618";
        assert_eq!(visible(text, &[]), ["class C { }"]);
        // `#!` is a shebang only on the first line, and only in column one.
        assert_eq!(resolve("x\n#!/bin/sh\n", &set(&[])), None);
        assert_eq!(resolve(" #!/bin/sh\n", &set(&[])), None);
    }

    #[test]
    fn last_line_directive_without_newline_is_blanked() {
        let text = "class C { }\n#pragma warning restore 618";
        let out = kept(text, &[]);
        assert_eq!(out, format!("class C {{ }}\n{}", " ".repeat(27)));
    }

    #[test]
    fn crlf_and_multibyte_text_keep_every_offset() {
        let text =
            "// é\r\n#if A\r\nvar ü = \"日本\";\r\n#else\r\nvar x = \"€\"; // ✓\r\n#endif\r\n";
        let out = kept(text, &[]);
        let kept_line = "var ü = \"日本\";";
        assert_eq!(kept_line.len(), 18);
        assert_eq!(
            out,
            format!(
                "// é\r\n     \r\n{}\r\n     \r\nvar x = \"€\"; // ✓\r\n      \r\n",
                " ".repeat(18)
            )
        );
        let out = kept(text, &["A"]);
        assert!(out.contains("var ü = \"日本\";\r\n") && !out.contains('€'));
        assert_eq!(kept("#if A\nä\rö\n#endif", &[]), "     \n  \r  \n      ");
    }

    #[test]
    fn unchanged_text_declines_the_fallback() {
        assert_eq!(resolve("class C { int with = 1; }\n", &set(&[])), None);
        assert_eq!(resolve("", &set(&[])), None);
        assert_eq!(resolve("x # y\n\"#if\"\n", &set(&[])), None);
    }

    #[test]
    fn malformed_directives_decline_the_fallback() {
        for text in [
            "#if A\nx\n",
            "#endif\n",
            "#else\n",
            "#elif A\n",
            "#if A\n#else\n#else\n#endif\n",
            "#if A\n#else\n#elif B\n#endif\n",
            "#if A\n#else x\n#endif\n",
            "#if A\n#endif x\n",
            "#if\n#endif\n",
            "#if A B\n#endif\n",
            "#if A\n#elif &&\n#endif\n",
            "#if_A\n#endif\n",
            "#ifdef A\n#endif\n",
            "#define\n",
            "#define 1A\n",
            "#define true\n",
            "#define A B\n",
            "#undef false\n",
            "#unknown\n",
            "#\n",
            "#if A\n#endif\n#endif\n",
        ] {
            assert_eq!(resolve(text, &set(&["A"])), None, "{text:?}");
        }
        // Comments after `#else` and `#endif` are allowed.
        assert_eq!(
            visible("#if A\na\n#else // no\nb\n#endif // A\n", &[]),
            ["b"]
        );
        assert_eq!(visible("#if(A)\na\n#endif\n", &["A"]), ["a"]);
    }

    /// Deterministic xorshift generator for the shape property.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self, n: usize) -> usize {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 % n as u64) as usize
        }
    }

    #[test]
    fn resolved_text_keeps_length_and_line_breaks_for_generated_inputs() {
        let pieces = [
            "#if A",
            "#if !B && (A || C)",
            "#elif B",
            "#elif C == false",
            "#else",
            "#endif",
            "#define A",
            "#undef A",
            "#define C",
            "#region é",
            "#pragma x",
            "#:sdk X",
            "#bad",
            "int x = 1;",
            "  var é = \"日本\";",
            "",
            "\t",
            "{",
            "}",
            "// #if",
            "€",
            "#if A // ü",
        ];
        let breaks = ["\n", "\r\n", "\r", ""];
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
        let mut resolved = 0;
        for _ in 0..4000 {
            let mut text = String::new();
            for _ in 0..rng.next(14) {
                text.push_str(pieces[rng.next(pieces.len())]);
                text.push_str(breaks[rng.next(breaks.len())]);
            }
            let names = [&["A"][..], &["B", "C"], &[]][rng.next(3)];
            if let Some(out) = resolve(&text, &set(names)) {
                assert_same_shape(&text, &out);
                assert_ne!(out, text);
                resolved += 1;
            }
        }
        assert!(resolved > 300, "{resolved}");
    }
}
