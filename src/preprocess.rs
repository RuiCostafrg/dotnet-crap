//! Conditional-compilation fallback for sources that do not parse as written.
//!
//! The C# grammar (tree-sitter-c-sharp 0.24.x) accepts preprocessor directives
//! only around whole members or statements, and rejects some valid placements
//! (an `#if` that splits a declaration, a directive on the last line without a
//! line break, file-based app `#:` lines). It also accepts some placements it
//! builds a wrong shape for: 0.24 parses an `#if` that splits a method header
//! from its body as a method without a body. A file "does not parse as written"
//! when its tree has errors or holds a conditional directive in a position the
//! 0.23.5 grammar rejected (see `placement`). For such a file [`resolve`]
//! produces a same-length text in which every directive line and every
//! inactive conditional branch is replaced by ASCII spaces. Every `\n` and
//! `\r` is kept and every other byte of a blanked line becomes one space, so
//! byte offsets, line numbers and columns of the kept text are unchanged, and
//! the result is valid UTF-8 because whole lines are replaced. Files that
//! parse as written never reach this module.
//!
//! The rule throughout: the resolved text must be what the C# compiler would
//! compile, or the fallback declines and the caller reports the original
//! error. Active text is lexed (comments, character literals and every string
//! form, including interpolation holes) so that a `#` line inside a multi-line
//! comment or string is kept as text, exactly as the compiler reads it.
//! Inactive branches are skipped without lexing, as the compiler skips them,
//! but their directives are still checked. Anything the lexer does not model
//! confidently, every directive the compiler rejects, and line breaks other
//! than `\n` and `\r\n` make the fallback decline.

use std::collections::BTreeSet;

/// Deepest nesting of parentheses and `!` accepted in one `#if` expression:
/// 256 levels are accepted, 257 make the fallback decline.
/// Keeps the recursive evaluator well within any worker thread's stack.
const MAX_EXPRESSION_DEPTH: usize = 256;

/// Longest directive identifier the fallback accepts. The compiler truncates
/// longer identifiers in directives to this many characters, so two distinct
/// long symbols could collide; the fallback declines instead of guessing.
const MAX_SYMBOL_LEN: usize = 128;

/// Return whether `name` is accepted as a conditional compilation symbol: an
/// ASCII letter or `_`, then ASCII letters, digits or `_`, at most
/// [`MAX_SYMBOL_LEN`] characters, and not `true` or `false` in any letter case.
///
/// The compiler reads `true` and `false` in a directive expression with
/// `bool.TryParse`, which ignores case, so every casing is a literal and can
/// never be a symbol. Non-ASCII identifiers are not accepted: Rust has no
/// table of the C# identifier categories (its `Alphabetic` property admits
/// combining marks that C# rejects at the start of an identifier), so a
/// directive with one makes the fallback decline rather than guess.
pub(crate) fn is_symbol(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes
        .next()
        .is_some_and(|b| b == b'_' || b.is_ascii_alphabetic())
        && bytes.all(|b| b == b'_' || b.is_ascii_alphanumeric())
        && name.len() <= MAX_SYMBOL_LEN
        && !name.eq_ignore_ascii_case("true")
        && !name.eq_ignore_ascii_case("false")
}

/// Whether an open group is `#if` or `#region`.
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    If,
    Region,
}

/// One open `#if` or `#region` group. They nest within each other strictly.
struct Group {
    kind: Kind,
    /// Whether the text around the group is active.
    parent_active: bool,
    /// Whether a branch of the group has already been taken.
    taken: bool,
    /// Whether the current branch is active.
    active: bool,
    /// Whether `#else` has been seen; a later `#elif` or `#else` is malformed.
    seen_else: bool,
}

/// Whether `text` contains a line break the fallback does not split on: a
/// carriage return not followed by `\n`, U+0085, U+2028 or U+2029. The C#
/// compiler ends lines at each of them; the fallback declines instead.
fn has_other_line_break(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes
        .iter()
        .enumerate()
        .any(|(i, &b)| b == b'\r' && bytes.get(i + 1) != Some(&b'\n'))
        || text.contains(['\u{85}', '\u{2028}', '\u{2029}'])
}

/// Whether `line` starts with `#` after optional whitespace.
fn starts_with_hash(line: &str) -> bool {
    line.trim_start_matches(is_space).starts_with('#')
}

/// Resolve conditional compilation in `text` with `defines` as the initial
/// symbol set, honouring `#define` and `#undef` in active text in order.
///
/// Returns `None`, so the caller keeps the original parse result, when no line
/// starts with `#` (without allocating), when no line would change, when the
/// text has a line break other than `\n` or `\r\n`, when active text is not
/// lexed confidently (an unterminated comment or string, a `#` line inside an
/// interpolation hole, an unsupported string form), or when the compiler would
/// reject a directive: unbalanced or crossed `#if` and `#region` groups, an
/// invalid expression or symbol (also in inactive text), an unknown directive,
/// an active `#error`, or `#define`, `#undef` or `#:` after the first token.
pub(crate) fn resolve(text: &str, defines: &BTreeSet<String>) -> Option<String> {
    if !text.split('\n').any(starts_with_hash) || has_other_line_break(text) {
        return None;
    }
    let mut symbols: BTreeSet<&str> = defines.iter().map(String::as_str).collect();
    let mut groups: Vec<Group> = Vec::new();
    let mut lexer = Lexer::default();
    // Allocated once with the input's exact length; blanking only overwrites
    // bytes by index, so the output can never grow.
    let mut out = text.as_bytes().to_vec();
    let mut start = 0;
    for (index, line) in text.split('\n').enumerate() {
        let end = start + line.len();
        let active = groups.last().is_none_or(|g| g.active);
        // Lexical state only advances through active lines, and directives
        // need code state, so text inside a comment or string is active.
        let blank = if lexer.in_code() {
            match directive(line, index) {
                None => {
                    if active {
                        lexer.line(line)?;
                    }
                    !active
                }
                Some(Directive::Plain) => {
                    // A `#:` line after a token (a `#!` is always first).
                    if active && lexer.seen_token {
                        return None;
                    }
                    true
                }
                Some(Directive::Named(name, rest)) => {
                    let state = State {
                        active,
                        seen_token: lexer.seen_token,
                    };
                    apply(name, rest, state, &mut groups, &mut symbols)?;
                    true
                }
            }
        } else {
            if lexer.in_hole() && starts_with_hash(line) {
                return None;
            }
            lexer.line(line)?;
            false
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
    if !groups.is_empty() || !lexer.in_code() || out == text.as_bytes() {
        return None;
    }
    Some(String::from_utf8(out).expect("whole lines of UTF-8 replaced by ASCII"))
}

/// A directive line, as recognised by [`directive`].
enum Directive<'a> {
    /// A file-based app line (`#:`) or a first-line `#!`, blanked without
    /// effect.
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

/// Where a directive appears.
#[derive(Clone, Copy)]
struct State {
    /// Whether the directive is in active text.
    active: bool,
    /// Whether active text before it held a token.
    seen_token: bool,
}

/// Apply one named directive to the group stack and symbol set. Directives in
/// inactive text are checked as the compiler checks them, but do not act.
fn apply<'a>(
    name: &str,
    rest: &'a str,
    state: State,
    groups: &mut Vec<Group>,
    symbols: &mut BTreeSet<&'a str>,
) -> Option<()> {
    let active = state.active;
    match name {
        "if" => {
            let value = evaluate(rest, symbols)?;
            groups.push(Group {
                kind: Kind::If,
                parent_active: active,
                taken: !active || value,
                active: active && value,
                seen_else: false,
            });
        }
        "elif" => {
            let value = evaluate(rest, symbols)?;
            let group = groups
                .last_mut()
                .filter(|g| g.kind == Kind::If && !g.seen_else)?;
            group.active = group.parent_active && !group.taken && value;
            group.taken |= value;
        }
        "else" => {
            ends_cleanly(rest)?;
            let group = groups
                .last_mut()
                .filter(|g| g.kind == Kind::If && !g.seen_else)?;
            group.active = group.parent_active && !group.taken;
            group.taken = true;
            group.seen_else = true;
        }
        "endif" => {
            ends_cleanly(rest)?;
            groups.pop_if(|g| g.kind == Kind::If)?;
        }
        "region" => groups.push(Group {
            kind: Kind::Region,
            parent_active: active,
            taken: true,
            active,
            seen_else: true,
        }),
        "endregion" => {
            groups.pop_if(|g| g.kind == Kind::Region)?;
        }
        "define" | "undef" => {
            let symbol = strip_comment(rest).trim_matches(is_space);
            if !is_symbol(symbol) || state.seen_token {
                return None;
            }
            if active && name == "define" {
                symbols.insert(symbol);
            } else if active {
                symbols.remove(symbol);
            }
        }
        // The compiler skips inactive `#nullable` and `#line` lines without
        // checking their arguments.
        "nullable" if active => nullable(rest)?,
        "line" if active => line(rest)?,
        "error" if active => return None,
        "nullable" | "line" | "pragma" | "warning" | "error" => {}
        _ => return None,
    }
    Some(())
}

/// Accept a `#nullable` setting: `enable`, `disable` or `restore`, optionally
/// followed by `warnings` or `annotations`, then an optional `//` comment.
fn nullable(rest: &str) -> Option<()> {
    let mut words = strip_comment(rest).split_whitespace();
    let setting = words.next()?;
    let target = words.next();
    (matches!(setting, "enable" | "disable" | "restore")
        && target.is_none_or(|t| matches!(t, "warnings" | "annotations"))
        && words.next().is_none())
    .then_some(())
}

/// Accept an active `#line` argument the compiler accepts: `default`,
/// `hidden`, or a decimal line number of at least 1 optionally followed by
/// whitespace and a double-quoted file name, then an optional `//` comment.
/// The number is at most `i32::MAX`, with any leading zeros. The file name has no escapes and runs to the next `"`. The C# 11 span form
/// `(l, c) - (l, c) [offset] "file"` is not followed and declines.
fn line(rest: &str) -> Option<()> {
    let args = rest.trim_start_matches(is_space);
    for keyword in ["default", "hidden"] {
        if let Some(after) = args.strip_prefix(keyword) {
            return (!after.starts_with(|c: char| c == '_' || c.is_alphanumeric()))
                .then(|| ends_cleanly(after))
                .flatten();
        }
    }
    let digits = args.bytes().take_while(u8::is_ascii_digit).count();
    let (number, after) = args.split_at(digits);
    // At least 1, and an `int` as in the compiler (a larger number is an
    // error there; a number above 16707565 only warns).
    if !number
        .parse::<u32>()
        .is_ok_and(|n| (1..=i32::MAX as u32).contains(&n))
    {
        return None;
    }
    // The number ends at whitespace, a comment or the end of the line, so
    // hexadecimal, digit separators and a glued file name decline.
    if !after.is_empty() && !after.starts_with(is_space) && !after.starts_with("//") {
        return None;
    }
    let after = after.trim_start_matches(is_space);
    match after.strip_prefix('"') {
        Some(name) => ends_cleanly(&name[name.find('"')? + 1..]),
        None => ends_cleanly(after),
    }
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

/// The literal form of an open string.
#[derive(Clone, Copy, PartialEq)]
enum Quote {
    /// `"..."`, with backslash escapes, on one line.
    Regular,
    /// `@"..."`, with `""` escapes, over any number of lines.
    Verbatim,
    /// A raw string closed by this many quotes; `multi_line` when its opening
    /// quotes end their line.
    Raw { quotes: usize, multi_line: bool },
}

/// One level of lexical nesting inside active text.
#[derive(Clone, Copy, PartialEq)]
enum Mode {
    /// Inside `/* ... */`.
    Comment,
    /// Literal text of a string; `dollars` is 0 when it is not interpolated.
    Text { quote: Quote, dollars: usize },
    /// Code in an interpolation hole of the string below it, with the depth of
    /// open parentheses, brackets and braces.
    Hole { depth: usize },
}

/// Lexical state across the active lines of a file. Only as much of C# is
/// tokenized as is needed to know where comments and strings end and whether
/// a token has been seen; every form it cannot follow makes [`Lexer::line`]
/// return `None`.
#[derive(Default)]
struct Lexer {
    /// Open comments, strings and holes, innermost last; empty in plain code.
    /// Change it only through [`Lexer::push`] and [`Lexer::pop`].
    stack: Vec<Mode>,
    /// How many open strings in `stack` must end on their line (regular and
    /// single-line raw), kept equal to a scan of `stack` so the end-of-line
    /// check is constant time however deeply holes nest.
    line_bound: usize,
    /// Whether any token has been seen in active text.
    seen_token: bool,
}

/// Whether `b` is C# whitespace in the ASCII range (the `\r` of `\r\n`
/// included).
fn is_ascii_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r' | 0x0b | 0x0c)
}

/// Length of the run of `byte` at the start of `bytes`.
fn run(bytes: &[u8], byte: u8) -> usize {
    bytes.iter().take_while(|&&b| b == byte).count()
}

/// Whether an open string of this mode must end on the line it starts on.
fn ends_on_line(mode: Mode) -> bool {
    matches!(
        mode,
        Mode::Text {
            quote: Quote::Regular
                | Quote::Raw {
                    multi_line: false,
                    ..
                },
            ..
        }
    )
}

impl Lexer {
    /// Open a level of nesting.
    fn push(&mut self, mode: Mode) {
        self.line_bound += usize::from(ends_on_line(mode));
        self.stack.push(mode);
    }

    /// Close the innermost level.
    fn pop(&mut self) {
        if let Some(mode) = self.stack.pop() {
            self.line_bound -= usize::from(ends_on_line(mode));
        }
    }

    /// Whether the next line starts in plain code, where directives apply.
    fn in_code(&self) -> bool {
        self.stack.is_empty()
    }

    /// Whether the next line starts inside an interpolation hole.
    fn in_hole(&self) -> bool {
        self.stack.iter().any(|m| matches!(m, Mode::Hole { .. }))
    }

    /// The string whose hole is innermost, as `(quote, dollars)`.
    fn hole_owner(&self) -> Option<(Quote, usize)> {
        let below = self.stack.len().checked_sub(2)?;
        match self.stack[below] {
            Mode::Text { quote, dollars } => Some((quote, dollars)),
            _ => None,
        }
    }

    /// Lex one active line (without its `\n`).
    ///
    /// Every step advances at least one byte and pushes at most one level, so
    /// the loop runs at most `line.len()` times and the stack grows by at most
    /// the line's length. A line not finished within that bound declines.
    fn line(&mut self, line: &str) -> Option<()> {
        let mut i = 0;
        for _ in 0..line.len() {
            if i >= line.len() {
                break;
            }
            i = match self.stack.last().copied() {
                None | Some(Mode::Hole { .. }) => self.code(line, i)?,
                Some(Mode::Comment) => match line[i..].find("*/") {
                    Some(at) => {
                        self.pop();
                        i + at + 2
                    }
                    None => line.len(),
                },
                Some(Mode::Text { quote, dollars }) => self.text(line, i, quote, dollars)?,
            };
        }
        if i < line.len() {
            return None;
        }
        // Regular and single-line raw strings end on their line. A hole that
        // continues past the end of such a string's line is not followed.
        (self.line_bound == 0).then_some(())
    }

    /// Lex code (plain or in a hole) at byte `i` of `line`; return where to
    /// continue.
    fn code(&mut self, line: &str, i: usize) -> Option<usize> {
        let bytes = line.as_bytes();
        let b = bytes[i];
        if is_ascii_space(b) {
            return Some(i + 1);
        }
        let next = bytes.get(i + 1).copied();
        let in_hole = !self.stack.is_empty();
        match (b, next) {
            (b'/', Some(b'/')) => {
                // A comment would end the hole's line; not followed.
                return (!in_hole).then_some(line.len());
            }
            (b'/', Some(b'*')) => {
                self.push(Mode::Comment);
                return Some(i + 2);
            }
            _ => {}
        }
        if !b.is_ascii() {
            let c = line[i..].chars().next().expect("char boundary");
            self.seen_token |= !c.is_whitespace();
            return Some(i + c.len_utf8());
        }
        self.seen_token = true;
        match b {
            b'"' | b'@' | b'$' => self.string(line, i),
            b'\'' => {
                // A character literal: one character or one escape sequence.
                let end = if next == Some(b'\\') {
                    i + 3 + line.get(i + 3..)?.find('\'')?
                } else {
                    let c = line.get(i + 1..)?.chars().next()?;
                    let end = i + 1 + c.len_utf8();
                    (c != '\'' && bytes.get(end) == Some(&b'\'')).then_some(end)?
                };
                Some(end + 1)
            }
            _ if in_hole => self.hole_code(line, i),
            _ => Some(i + 1),
        }
    }

    /// Lex one ASCII byte of hole code that is not whitespace, a comment, a
    /// string or a character literal: track nesting, a format clause and the
    /// closing braces.
    fn hole_code(&mut self, line: &str, i: usize) -> Option<usize> {
        let bytes = line.as_bytes();
        let (_, dollars) = self.hole_owner()?;
        let Some(Mode::Hole { depth }) = self.stack.last_mut() else {
            unreachable!("hole code is lexed only in a hole")
        };
        match bytes[i] {
            b'(' | b'[' | b'{' => *depth += 1,
            b')' | b']' if *depth == 0 => return None,
            b')' | b']' => *depth -= 1,
            b'}' if *depth > 0 => *depth -= 1,
            b'}' => {
                // The hole closes with as many braces as the string has `$`.
                let closing = dollars.max(1);
                if run(&bytes[i..], b'}') < closing {
                    return None;
                }
                self.pop();
                return Some(i + closing);
            }
            b':' if *depth == 0 => {
                // A format clause runs to the closing brace, on this line, and
                // is followed only when it is plain text. `::` is not followed.
                let clause = &line[i + 1..];
                let end = clause.find('}')?;
                if clause.starts_with(':') || clause[..end].contains(['"', '{', '\\', '\'']) {
                    return None;
                }
                return Some(i + 1 + end);
            }
            _ => {}
        }
        Some(i + 1)
    }

    /// Lex the start of a string at byte `i`: optional `@` and `$` prefixes,
    /// then quotes. A `@` without a quote starts a verbatim identifier.
    fn string(&mut self, line: &str, i: usize) -> Option<usize> {
        let bytes = line.as_bytes();
        let mut j = i;
        let mut verbatim = bytes[j] == b'@';
        j += usize::from(verbatim);
        let dollars = run(&bytes[j..], b'$');
        j += dollars;
        if !verbatim && bytes.get(j) == Some(&b'@') {
            verbatim = true;
            j += 1;
        }
        if bytes.get(j) != Some(&b'"') {
            // `@name` is an identifier; a `$` outside a string is not C#.
            return (dollars == 0 && j == i + 1).then_some(j);
        }
        let quotes = run(&bytes[j..], b'"');
        let quote = if verbatim {
            if dollars > 1 {
                return None;
            }
            j += 1;
            Quote::Verbatim
        } else if quotes < 3 {
            if dollars > 1 {
                return None;
            }
            if quotes == 2 {
                // An empty string.
                return Some(j + 2);
            }
            j += 1;
            Quote::Regular
        } else {
            j += quotes;
            let multi_line = line[j..].bytes().all(is_ascii_space);
            Quote::Raw { quotes, multi_line }
        };
        self.push(Mode::Text { quote, dollars });
        Some(j)
    }

    /// Lex literal text of a string at byte `i`; return where to continue.
    fn text(&mut self, line: &str, i: usize, quote: Quote, dollars: usize) -> Option<usize> {
        let bytes = line.as_bytes();
        let b = bytes[i];
        let next = bytes.get(i + 1).copied();
        match (quote, b) {
            (Quote::Regular, b'\\') => return Some(i + 2),
            (Quote::Regular, b'"') => {
                self.pop();
                return Some(i + 1);
            }
            (Quote::Verbatim, b'"') if next == Some(b'"') => return Some(i + 2),
            (Quote::Verbatim, b'"') => {
                self.pop();
                return Some(i + 1);
            }
            (Quote::Raw { quotes, multi_line }, b'"') => {
                let found = run(&bytes[i..], b'"');
                if found < quotes {
                    return Some(i + found);
                }
                // The closing quotes, exactly as many as opened, and on a line
                // of their own in a multi-line raw string.
                if found > quotes || multi_line && !line[..i].bytes().all(is_ascii_space) {
                    return None;
                }
                self.pop();
                return Some(i + found);
            }
            _ => {}
        }
        if dollars == 0 || !matches!(b, b'{' | b'}') {
            return Some(i + 1);
        }
        let found = run(&bytes[i..], b);
        if matches!(quote, Quote::Raw { .. }) {
            // Fewer braces than `$` are text; up to twice as many open a hole
            // after the extra ones; a run of closing braces is not followed.
            if found < dollars {
                return Some(i + found);
            }
            if b == b'}' || found >= 2 * dollars {
                return None;
            }
            self.push(Mode::Hole { depth: 0 });
            return Some(i + found);
        }
        // `{{` and `}}` are escaped braces; one `{` opens a hole.
        match (b, next) {
            (_, Some(n)) if n == b => Some(i + 2),
            (b'{', _) => {
                self.push(Mode::Hole { depth: 0 });
                Some(i + 1)
            }
            _ => None,
        }
    }
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
///
/// Every token takes at least one byte, so the loop runs at most once more
/// than `text.len()` times and there are at most `text.len()` tokens; an
/// expression not finished within that bound declines.
fn tokenize(text: &str) -> Option<Vec<Token<'_>>> {
    let mut tokens = Vec::new();
    let mut rest = text;
    for _ in 0..=text.len() {
        rest = rest.trim_start_matches(is_space);
        if rest.is_empty() || rest.starts_with("//") {
            return Some(tokens);
        }
        let first = rest.as_bytes()[0];
        let (token, len) = if first == b'_' || first.is_ascii_alphabetic() {
            let len = word_len(rest);
            let word = &rest[..len];
            // Non-ASCII and over-long identifiers decline (see `is_symbol`).
            if !word.is_ascii() || len > MAX_SYMBOL_LEN {
                return None;
            }
            // `bool.TryParse` in the compiler: any letter case.
            let token = if word.eq_ignore_ascii_case("true") {
                Token::Literal(true)
            } else if word.eq_ignore_ascii_case("false") {
                Token::Literal(false)
            } else {
                Token::Symbol(word)
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
    None
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

    // Each binary loop consumes an operator per iteration, so it runs at
    // most once per token; the bound declines instead of spinning.

    fn or(&mut self, depth: usize) -> Option<bool> {
        let mut value = self.and(depth)?;
        for _ in 0..self.tokens.len() {
            if !self.eat(&Token::Or) {
                return Some(value);
            }
            value |= self.and(depth)?;
        }
        None
    }

    fn and(&mut self, depth: usize) -> Option<bool> {
        let mut value = self.equality(depth)?;
        for _ in 0..self.tokens.len() {
            if !self.eat(&Token::And) {
                return Some(value);
            }
            value &= self.equality(depth)?;
        }
        None
    }

    fn equality(&mut self, depth: usize) -> Option<bool> {
        let mut value = self.unary(depth)?;
        for _ in 0..self.tokens.len() {
            if self.eat(&Token::Equal) {
                value = value == self.unary(depth)?;
            } else if self.eat(&Token::NotEqual) {
                value = value != self.unary(depth)?;
            } else {
                return Some(value);
            }
        }
        None
    }

    fn unary(&mut self, depth: usize) -> Option<bool> {
        if depth > MAX_EXPRESSION_DEPTH {
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
        let longest = "a".repeat(MAX_SYMBOL_LEN);
        for good in [
            "A",
            "_",
            "_x1",
            "Ab_9",
            "DEBUG",
            "NET8_0_OR_GREATER",
            "if",
            "Truex",
            "_true",
            "trues",
            &longest,
        ] {
            assert!(is_symbol(good), "{good}");
        }
        let too_long = "a".repeat(MAX_SYMBOL_LEN + 1);
        for bad in [
            "", "1A", "A-B", "A B", "@A", "A.B", " A", "A,B", "true", "false", "TRUE", "True",
            "tRuE", "FALSE", "False",
            "fAlSe", // Not ASCII: declined rather than approximated.
            "é", "Aé", "éA", "ःA", "A\u{301}", "\u{ff21}", &too_long,
        ] {
            assert!(!is_symbol(bad), "{bad}");
        }
        assert_eq!(MAX_SYMBOL_LEN, 128);
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
            // `true` and `false` are literals in every letter case.
            ("TRUE", &[], true),
            ("True", &[], true),
            ("tRuE", &[], true),
            ("FALSE", &[], false),
            ("False", &["False"], false),
            ("fAlSe", &[], false),
            ("!FALSE", &[], true),
            ("A == TRUE", &["A"], true),
            ("FALSE || A", &["A"], true),
            ("TRUE && FALSE", &[], false),
            ("(True)", &[], true),
            ("Truex", &["Truex"], true),
            ("Truex", &[], false),
            ("_TRUE", &["_TRUE"], true),
            ("A // TRUE", &["A"], true),
        ];
        for (text, names, expected) in cases {
            assert_eq!(eval(text, names), Some(*expected), "{text} with {names:?}");
        }
    }

    #[test]
    fn malformed_expressions_are_rejected() {
        for text in [
            "", "// x", "A B", "A &&", "&& A", "(A", "A)", "()", "&A", "A = B", "A | B", "!", "1",
            "A.B", "A ==", "A !", "#A", "@true", "@A",
        ] {
            assert_eq!(eval(text, &["A", "B"]), None, "{text:?}");
        }
        // Non-ASCII identifiers and identifiers over 128 characters decline,
        // even when the same text is among the symbols.
        let long = "a".repeat(MAX_SYMBOL_LEN);
        let too_long = format!("{long}a");
        assert_eq!(eval(&long, &[&long]), Some(true));
        assert_eq!(eval(&long, &[]), Some(false));
        assert_eq!(
            eval(&format!("!{long} && ({long} || A)"), &["A"]),
            Some(true)
        );
        assert_eq!(eval(&too_long, &[&too_long]), None);
        assert_eq!(eval(&format!("A && {too_long}"), &["A"]), None);
        for text in [
            "ःA",
            "A && ःA",
            "é",
            "Aé",
            "A\u{301}",
            "\u{ff21}",
            "A || é",
            "ःtrue",
            "trueé",
        ] {
            assert_eq!(eval(text, &["A", "ःA", "é", "Aé"]), None, "{text:?}");
        }
    }

    #[test]
    fn expression_depth_is_bounded() {
        let nested = |n: usize| format!("{}A{}", "(".repeat(n), ")".repeat(n));
        assert_eq!(MAX_EXPRESSION_DEPTH, 256);
        assert_eq!(eval(&nested(MAX_EXPRESSION_DEPTH), &["A"]), Some(true));
        assert_eq!(eval(&nested(MAX_EXPRESSION_DEPTH + 1), &["A"]), None);
        let nots = |n: usize| format!("{}A", "!".repeat(n));
        assert_eq!(eval(&nots(MAX_EXPRESSION_DEPTH - 1), &[]), Some(true));
        assert_eq!(eval(&nots(MAX_EXPRESSION_DEPTH), &[]), Some(false));
        assert_eq!(eval(&nots(MAX_EXPRESSION_DEPTH + 1), &[]), None);
        assert_eq!(eval(&nested(100_000), &["A"]), None);
        // Flat expressions of any length fit the per-token loop bounds.
        let flat = |op: &str, n: usize| vec!["A"; n].join(op);
        assert_eq!(eval(&flat("||", 5000), &["A"]), Some(true));
        assert_eq!(eval(&flat(" && ", 5000), &["A"]), Some(true));
        assert_eq!(eval(&flat("==", 5000), &[]), Some(true));
        assert_eq!(eval(&flat("!=", 5000), &[]), Some(false));
        assert_eq!(eval("A==B!=A", &["A"]), Some(true));
        assert_eq!(tokenize(&"!".repeat(5000)).unwrap().len(), 5000);
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

    #[test]
    fn literals_ignore_letter_case() {
        assert_eq!(
            tokenize("TRUE True tRuE FALSE False fAlSe true false").unwrap(),
            [
                Token::Literal(true),
                Token::Literal(true),
                Token::Literal(true),
                Token::Literal(false),
                Token::Literal(false),
                Token::Literal(false),
                Token::Literal(true),
                Token::Literal(false),
            ]
        );
        assert_eq!(tokenize("Truex").unwrap(), [Token::Symbol("Truex")]);
    }

    /// `#if` and `#elif` with every casing of `true` and `false` take the
    /// branch the literal selects, with or without symbols.
    #[test]
    fn if_and_elif_literals_ignore_letter_case() {
        for t in ["true", "TRUE", "True", "tRuE", "trUE"] {
            let in_if = format!("#if {t}\na\n#else\nb\n#endif\n");
            let in_elif = format!("#if false\na\n#elif {t}\nb\n#else\nc\n#endif\n");
            let negated = format!("#if !{t}\na\n#else\nb\n#endif\n");
            for names in [&[][..], &["A", "x"]] {
                assert_eq!(visible(&in_if, names), ["a"], "{t}");
                assert_eq!(visible(&in_elif, names), ["b"], "{t}");
                assert_eq!(visible(&negated, names), ["b"], "{t}");
            }
        }
        for f in ["false", "FALSE", "False", "fAlSe", "faLSE"] {
            let in_if = format!("#if {f}\na\n#else\nb\n#endif\n");
            let in_elif = format!("#if false\na\n#elif {f}\nb\n#else\nc\n#endif\n");
            let negated = format!("#if !{f}\na\n#else\nb\n#endif\n");
            for names in [&[][..], &["A", "x"]] {
                assert_eq!(visible(&in_if, names), ["b"], "{f}");
                assert_eq!(visible(&in_elif, names), ["c"], "{f}");
                assert_eq!(visible(&negated, names), ["a"], "{f}");
            }
        }
        // A symbol named like a literal is never consulted, and never defined.
        assert_eq!(visible("#if FALSE\na\n#else\nb\n#endif", &["FALSE"]), ["b"]);
        assert_eq!(visible("#if TRUE\na\n#else\nb\n#endif", &[]), ["a"]);
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
        // Inactive text is not lexed and its directives do not act, but they
        // are checked as the compiler checks them.
        let skipped = "#if false\n#if A && !B\nx\n#elif (C)\ny\n#endif\n#error e\n#define D\n\"\n/*\n#endif\nz";
        assert_eq!(visible(skipped, &[]), ["z"]);
        for bad in [
            "#if &&",
            "#elif ((",
            "#unknown",
            "#define 1",
            "#undef true",
            "#else x",
        ] {
            let text = format!("#if false\n#if A\n{bad}\n#endif\n#endif\nz");
            assert_eq!(resolve(&text, &set(&[])), None, "{bad}");
        }
    }

    #[test]
    fn define_and_undef_apply_in_order_in_active_text() {
        let text = "#define A // set\n#define B\n#undef A\n#if false\n#define C\n#undef B\n#endif\n#undef Z\n#if A\na\n#endif\n#if B\nb\n#endif\n#if C\nc\n#endif\n";
        assert_eq!(visible(text, &[]), ["b"]);
        // Comments, blank lines and app lines are not tokens.
        assert_eq!(
            visible(
                "// c\n/* d\n */\n#:sdk X\n\n#define A\n#if A\na\n#endif",
                &[]
            ),
            ["// c", "/* d", " */", "a"]
        );
        // Non-ASCII whitespace is not a token; a non-ASCII letter is.
        assert_eq!(
            visible("\u{a0}\u{3000}\n#define A\n#if A\na\n#endif", &[]),
            ["\u{a0}\u{3000}", "a"]
        );
        assert_eq!(resolve("é\n#define A\n", &set(&[])), None);
        // `#:` lines after a token are rejected only in active text.
        assert_eq!(visible("x\n#if false\n#:sdk X\n#endif\n", &[]), ["x"]);
        assert_eq!(
            visible("#undef A\n#if A\na\n#endif\n", &["A"]),
            Vec::<String>::new()
        );
        assert_eq!(visible("#  define\tA\n  #  if A\na\n #endif", &[]), ["a"]);
    }

    #[test]
    fn other_directives_and_app_lines_are_blanked() {
        let text = "#!/usr/bin/env dotnet\n#:sdk Microsoft.NET.Sdk\n  #:package A@1\n#region R\n#pragma warning disable 1\n#nullable enable\n#nullable restore warnings // w\n#line 5\n#line default\n#warning w\n#if false\n#error e\n#endif\n#endregion\nclass C { }\n#pragma warning restore 618";
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
        // Line breaks the fallback does not split on make it decline.
        for text in [
            "#if A\nä\rö\n#endif",
            "#if A\nx\n#endif\r",
            "#if A\nx\u{2028}\n#endif",
            "#if A\nx\u{2029}\n#endif",
            "#if A\nx\u{85}\n#endif",
        ] {
            assert_eq!(resolve(text, &set(&[])), None, "{text:?}");
        }
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
            "#define TRUE\n",
            "#define True\n",
            "#define A B\n",
            "#undef false\n",
            "#undef FALSE\n",
            "#undef fAlSe\n",
            "#define é\n",
            "#define ःA\n",
            "#undef ःA\n",
            "#define Aé\n",
            "#if ःA\n#endif\n",
            "#if é\n#endif\n",
            "#if A\n#elif ःA\n#endif\n",
            "#if false\n#define TRUE\n#endif\n",
            "#if false\n#undef true\n#endif\n",
            "#if false\n#define ःA\n#endif\n",
            "#unknown\n",
            "#\n",
            "#if A\n#endif\n#endif\n",
            // Stricter than tolerance: what the compiler rejects.
            "#if true\n#error stop\n#endif\n",
            "#error stop\n",
            "x\n#define B\n",
            "x\n#undef A\n",
            "x\n#if false\n#define B\n#endif\n",
            "x\n#:sdk X\n",
            "#region\n",
            "#endregion\n",
            "#region\n#endregion\n#endregion\n",
            "#region\n#if A\n#endregion\n#endif\n",
            "#if A\n#region\n#endif\n#endregion\n",
            "#if A\n#region\n#else\n#endregion\n#endif\n",
            "#if false\n#endregion\n#endif\n",
            "#nullable\n",
            "#nullable on\n",
            "#nullable enable all\n",
            "#nullable enable warnings x\n",
            "#if false\n#bogus\n#endif\n",
            "#if false\n#if (\n#endif\n#endif\n",
            "#if A\n#elif !\n#endif\n",
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

    /// `directive` on its own line before a member: resolved text keeps the
    /// member, or `None` when the fallback declines.
    fn resolves_with(directive: &str) -> bool {
        resolve(&format!("{directive}\nclass C {{ }}\n"), &set(&[])).is_some()
    }

    #[test]
    fn active_nullable_is_validated_and_inactive_is_not() {
        for ok in [
            "#nullable enable",
            "#nullable disable warnings",
            "#nullable restore annotations // c",
        ] {
            assert!(resolves_with(ok), "{ok}");
        }
        for bad in [
            "#nullable on",
            "#nullable",
            "#nullable enable all",
            "#nullable enable warnings x",
        ] {
            assert!(!resolves_with(bad), "{bad}");
            // The compiler skips it in inactive text, so it is accepted.
            let inactive = format!("#if false\n{bad}\n#endif\nclass C {{ }}\n");
            assert_eq!(visible(&inactive, &[]), ["class C { }"], "{bad}");
            let nested = format!("#if A\n#else\n{bad}\n#endif\nclass C {{ }}\n");
            assert_eq!(visible(&nested, &["A"]), ["class C { }"], "{bad}");
        }
    }

    #[test]
    fn active_line_directives_follow_the_compiler() {
        for ok in [
            "#line default",
            "#line hidden",
            "#line 1",
            "#line 5",
            "#line   7\t",
            "#line 007",
            "#line 2147483647",
            "#line 16707566",
            "#  line 5",
            "#line default // c",
            "#line hidden//c",
            "#line 5 // c",
            "#line 5//c",
            "#line 5 \"f.cs\"",
            "#line 5\t\"f.cs\"  ",
            "#line 5 \"\"",
            "#line 5 \"a\\b.cs\"",
            "#line 5 \"a // b.cs\" // c",
            "#line 5 \"ü 日本.cs\"",
        ] {
            assert!(resolves_with(ok), "{ok}");
        }
        for bad in [
            "#line",
            "#line abc",
            "#line \"f.cs\"",
            "#line 0",
            "#line 000",
            "#line -1",
            "#line +1",
            "#line 5 6",
            "#line 5 \"a\" extra",
            "#line 5 /*c*/",
            "#line 5\"f.cs\"",
            "#line 5 \"f.cs",
            "#line 5 \"f.cs\"\"",
            "#line 0x10",
            "#line 1_000",
            "#line 5x",
            "#line 5.0",
            "#line Default",
            "#line defaultx",
            "#line default x",
            "#line hidden \"f.cs\"",
            "#line hidden 5",
            "#line default /*c*/",
            // The span form is valid C# 11 but not followed.
            "#line (1, 1) - (2, 2) \"f.cs\"",
            "#line (1,1)-(2,2) 4 \"f.cs\"",
        ] {
            assert!(!resolves_with(bad), "{bad}");
            // Inactive `#line` is not checked.
            let inactive = format!("#if false\n{bad}\n#endif\nclass C {{ }}\n");
            assert_eq!(visible(&inactive, &[]), ["class C { }"], "{bad}");
        }
        // Numbers over `i32::MAX` decline when active. The compiler also
        // rejects them inactive, which the fallback does not check.
        for bad in ["#line 2147483648", "#line 99999999999999999999"] {
            assert!(!resolves_with(bad), "{bad}");
        }
        // The text after the directive is untouched and the line is blanked.
        assert_eq!(
            visible("#line 5 \"f.cs\"\nclass C { }\n", &[]),
            ["class C { }"]
        );
    }

    #[test]
    fn symbol_length_is_bounded_at_128() {
        let at = "a".repeat(MAX_SYMBOL_LEN);
        let over = format!("{at}a");
        let text = |symbol: &str| {
            format!("#define {symbol}\n#define {symbol}\n#undef {symbol}\n#define {symbol}\n#if {symbol}\nx\n#else\ny\n#endif\n")
        };
        assert_eq!(visible(&text(&at), &[]), ["x"]);
        assert_eq!(resolve(&text(&over), &set(&[])), None);
        for directive in ["#if", "#elif", "#define", "#undef"] {
            let text = format!("#if A\n#endif\n#if false\n#endif\n{directive} {over}\n#endif\n");
            assert_eq!(resolve(&text, &set(&[])), None, "{directive}");
        }
        // Over-long names decline in inactive text too.
        let inactive = format!("#if false\n#define {over}\n#endif\n");
        assert_eq!(resolve(&inactive, &set(&[])), None);
        let inactive = format!("#if false\n#if {over}\n#endif\n#endif\n");
        assert_eq!(resolve(&inactive, &set(&[])), None);
        // A 128-character `--define` is a symbol; one more character is not.
        let exact = format!("#if {at}\nx\n#else\ny\n#endif\n");
        assert_eq!(visible(&exact, &[&at]), ["x"]);
        assert_eq!(visible(&exact, &[]), ["y"]);
    }

    #[test]
    fn defines_and_undefs_of_literal_names_decline_in_any_case() {
        for name in ["true", "TRUE", "True", "false", "FALSE", "False", "fAlSe"] {
            for directive in ["#define", "#undef"] {
                let text = format!("{directive} {name}\nclass C {{ }}\n");
                assert_eq!(resolve(&text, &set(&[])), None, "{text:?}");
            }
        }
        // Names that merely contain one are symbols.
        let text = "#define Truex\n#define _FALSE\n#if Truex && _FALSE\nx\n#endif\n";
        assert_eq!(visible(text, &[]), ["x"]);
    }

    /// A deadline far beyond any honest run on a slow machine: a quadratic
    /// scan of the lexer stack at each line end takes minutes at this depth.
    const DEEP_LIMIT: std::time::Duration = std::time::Duration::from_secs(30);

    #[test]
    fn deeply_nested_interpolation_is_resolved_quickly() {
        let levels = 20_000;
        // Each level opens a verbatim interpolated string and its hole over
        // several lines; every line of a hole is checked at its end.
        let open = "var s = $@\"{\nvar a = 1;\nvar b = 2;\n".repeat(levels);
        let close = "}\"\n".repeat(levels);
        let text = format!("{open}x\n{close};\n#if A\na\n#else\nb\n#endif\n");
        let start = std::time::Instant::now();
        let out = resolve(&text, &set(&["A"]));
        assert!(start.elapsed() < DEEP_LIMIT, "{:?}", start.elapsed());
        let out = out.expect("nested verbatim holes resolve");
        assert_same_shape(&text, &out);
        let last = out.split('\n').map(str::trim).rfind(|l| !l.is_empty());
        assert_eq!(last, Some("a"));

        // A regular string must end on its line: the same nesting declines.
        let open = "var s = $\"{\nvar a = 1;\nvar b = 2;\n".repeat(levels);
        let text = format!("{open}x\n{close};\n#if A\na\n#endif\n");
        let start = std::time::Instant::now();
        assert_eq!(resolve(&text, &set(&["A"])), None);
        assert!(start.elapsed() < DEEP_LIMIT, "{:?}", start.elapsed());

        // Levels on one line, with a regular string deep inside, decline.
        let text = format!("{}\n#if A\n#endif\n", "$@\"{$\"{".repeat(levels));
        let start = std::time::Instant::now();
        assert_eq!(resolve(&text, &set(&["A"])), None);
        assert!(start.elapsed() < DEEP_LIMIT, "{:?}", start.elapsed());
    }

    #[test]
    fn open_regular_string_counter_matches_the_stack() {
        // Push and pop of every string form keep `line_bound` equal to a
        // scan of the stack, line by line.
        let text = "var a = \"x\"; var b = @\"y\nz\"; var c = $@\"{ \"q\" + $\"{1}\" }\n{2}\"; var d = \"\"\"\n  raw\n  \"\"\"; var e = \"\"\"r\"\"\"; /* c\n */ var f = $\"{1}\";\n";
        let mut lexer = Lexer::default();
        for line in text.split('\n') {
            lexer.line(line).expect("lexes");
            let scanned = lexer.stack.iter().filter(|m| ends_on_line(**m)).count();
            assert_eq!(lexer.line_bound, scanned, "{line:?}");
        }
        assert!(lexer.in_code());
        assert_eq!(lexer.line_bound, 0);
    }

    /// `#if A` ... `#else` ... `#endif` after `prefix`: `["a"]` when `A` is
    /// taken, `["b"]` otherwise, plus the visible lines of `prefix`.
    fn branch_after(prefix: &str, names: &[&str]) -> Vec<String> {
        let text = format!("{prefix}\n#if A\na\n#else\nb\n#endif\n");
        let prefix_lines = prefix.split('\n').filter(|l| !l.trim().is_empty()).count();
        visible(&text, names).split_off(prefix_lines)
    }

    #[test]
    fn hash_lines_inside_comments_and_strings_are_text() {
        let cases: &[(&str, usize)] = &[
            // Block comments, also several per line and nested-looking.
            ("/*\n#if A\n*/", 3),
            ("x /* a */ /*\n  #endif\n */ y", 3),
            ("/* /* */ z", 1),
            // Verbatim strings, with `""` escapes, plain and interpolated.
            ("var s = @\"\n#define A\n\";", 3),
            ("var s = @\"a\"\"\n#else\n\"\"\";", 3),
            ("var s = $@\"{x}\n#endif {{\n}}\";", 3),
            ("var s = @$\"\n#if B\n{y}\";", 3),
            // Raw strings, multi-line, single-line and interpolated.
            ("var s = \"\"\"\n  #if A\n  \"\"\";", 3),
            ("var s = \"\"\"\"\n  \"\"\" #endif\n  \"\"\"\";", 3),
            (
                "var s = $$\"\"\"\n  {#if} {{x}} {{{y}}}\n  #else\n  \"\"\";",
                4,
            ),
            ("var s = \"\"\"a \"\" b\"\"\"; var t = $\"\"\"{x}\"\"\";", 1),
            // Holes with strings, braces, parentheses and format clauses.
            (
                "var s = $@\"{F(\"}\")}\n#if A\n{(a ? b : c),5:N2} {new { }}\n#endif\";",
                4,
            ),
            ("var s = $@\"{ x\n  + y }\n#if A\n\";", 4),
            ("var s = $@\"{ /* } */ x }\n#endif\n\";", 3),
            // Regular strings and characters do not open anything.
            (
                "var s = \"/*\\\"\"; var c = '\"'; var d = '\\''; var e = '\\u0041';",
                1,
            ),
            ("var s = \"\"; var t = $\"{x}{{}}\"; // \"", 1),
            ("var @if = @class; var é = 'ü';", 1),
        ];
        for (prefix, lines) in cases {
            let text = format!("{prefix}\n#if A\na\n#else\nb\n#endif\n");
            let out = kept(&text, &[]);
            // The prefix is kept byte for byte, `#` lines included.
            assert!(out.starts_with(prefix), "{prefix:?} -> {out:?}");
            assert_eq!(prefix.split('\n').count(), *lines, "{prefix:?}");
            assert_eq!(branch_after(prefix, &[]), ["b"], "{prefix:?}");
            assert_eq!(branch_after(prefix, &["A"]), ["a"], "{prefix:?}");
        }
        // A `#define` in a string defines nothing, so `#if` sees no symbol.
        assert_eq!(branch_after("var s = @\"\n#define A\n\";", &[]), ["b"]);
    }

    #[test]
    fn lexer_steps_fit_the_line_length_bound() {
        // One-byte steps on every byte, and strings closing on the last byte.
        let dense = format!("x{}", "'a'\"b\"@\"c\"/**/".repeat(500));
        assert_eq!(branch_after(&dense, &["A"]), ["a"]);
        for prefix in ["x", "\"\"", "'a'", "/**/", "@\"\"", "\"\\\\\""] {
            assert_eq!(branch_after(prefix, &[]), ["b"], "{prefix:?}");
        }
    }

    #[test]
    fn unfollowed_lexical_forms_decline_the_fallback() {
        for prefix in [
            // Unterminated at the end of the file or of a line.
            "/* open",
            "var s = @\"open",
            "var s = \"\"\"\nopen",
            "var s = \"open",
            "var s = \"\"\"a\"\";",
            "var s = $\"{x}",
            "var c = 'ab';",
            "var c = '';",
            "var c = '",
            // A `#` line inside an interpolation hole.
            "var s = $@\"{\n#if A\nx}\";",
            "var s = $@\"{ /*\n#if A\n*/ x}\";",
            // Forms the lexer does not follow.
            "var s = $\"{x\n}\";",
            "var s = $@\"{x // c\n}\";",
            "var s = $@\"{x::y}\";",
            "var s = $@\"{x:\"q\"}\";",
            "var s = $@\"{x:{y}}\";",
            "var s = $@\"{x:a\nb}\";",
            "var s = $@\"{x)}\";",
            "var s = $@\"}\";",
            "var s = $\"}\";",
            "var s = $$\"x\";",
            "var s = $$@\"x\";",
            "var s = $$\"\";",
            "var s = $\"\"\"{{x}}\"\"\";",
            "var s = $\"\"\"x}\"\"\";",
            "var s = $$\"\"\"x}}\"\"\";",
            "var s = \"\"\"a\"\"\"\";",
            "var s = \"\"\"\n  x \"\"\";",
            "var s = $$\"\"\"{{x}\"\"\";",
            "var s = $x;",
            "var s = @$x;",
        ] {
            let text = format!("{prefix}\n#if A\na\n#else\nb\n#endif\n");
            assert_eq!(resolve(&text, &set(&["A"])), None, "{prefix:?}");
        }
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
            "#region",
            "#endregion",
            "/*",
            "*/",
            "@\"",
            "\"",
            "\"\"\"",
            "$@\"{",
            "}\"",
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
        // A lone `\r` always declines, so it is rare here.
        let breaks = ["\n", "\r\n", "\n", "\r\n", "\n", "\r", ""];
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
        let mut resolved = 0;
        for _ in 0..20_000 {
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

    #[test]
    fn escaped_character_literals_leave_the_rest_of_the_line_as_code() {
        // The closing quote of `'\n'` ends the character literal, so the
        // verbatim string after it opens and the `#if` line inside is text.
        let text = "var c = '\\n'; var s = @\"\n#if A\nx\";\n#if A\na\n#else\nb\n#endif\n";
        assert_eq!(
            visible(text, &["A"]),
            ["var c = '\\n'; var s = @\"", "#if A", "x\";", "a"]
        );
    }

    #[test]
    fn raw_string_closing_quotes_leave_following_code_intact() {
        // The single-line raw string closes before the verbatim string that
        // follows it, so the `#if` line inside that string is text.
        let text = "var s = \"\"\"x\"\"\"; var t = @\"\n#if A\nx\";\n#if A\na\n#else\nb\n#endif\n";
        assert_eq!(
            visible(text, &["A"]),
            ["var s = \"\"\"x\"\"\"; var t = @\"", "#if A", "x\";", "a"]
        );
    }

    #[test]
    fn format_clauses_ending_in_a_dollar_sign_are_followed() {
        // `$` is ordinary format-clause text up to the closing brace.
        let text = "var s = $\"{x:a$}\";\n#if A\na\n#else\nb\n#endif\n";
        assert_eq!(visible(text, &[]), ["var s = $\"{x:a$}\";", "b"]);
    }

    #[test]
    fn closing_brace_runs_in_raw_interpolated_strings_decline() {
        // A `}` run in a raw interpolated string is an unfollowed form.
        let text = "var s = $\"\"\"\na}b\n}\n\"\"\";\n#if A\nx\n#endif\n";
        assert_eq!(resolve(text, &set(&[])), None);
    }

    #[test]
    fn doubled_opening_braces_in_raw_interpolated_strings_decline() {
        // Two `{` with one `$` are an unfollowed opening-brace run.
        let text = "var s = $\"\"\"\na{{b\n}\n\"\"\";\n#if A\nx\n#endif\n";
        assert_eq!(resolve(text, &set(&[])), None);
    }

    #[test]
    fn doubled_closing_braces_in_interpolated_strings_are_text() {
        // `}}` is an escaped `}`, so the string closes and the branches work.
        let text = "var s = $\"x}}\";\n#if A\na\n#else\nb\n#endif\n";
        assert_eq!(visible(text, &[]), ["var s = $\"x}}\";", "b"]);
        assert_eq!(visible(text, &["A"]), ["var s = $\"x}}\";", "a"]);
    }
}
