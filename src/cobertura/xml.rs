//! Streaming XML reader for Cobertura reports.
//!
//! The grammar, character classes and acceptance rules follow roxmltree 0.21
//! with `allow_dtd: false` and no entity resolver, the configuration the
//! importer used when it built a DOM. [`check`] accepts a report exactly when
//! that parser would, without building a tree: it keeps the open-element stack,
//! the in-scope namespace declarations and a few child counts. A rejected report
//! is handed back to roxmltree only to render its diagnostic, so error messages
//! and positions are unchanged. [`tokenize`] then streams an accepted report a
//! second time for the importer.
//!
//! Only predefined and numeric character references exist: no DTD is accepted,
//! so every other entity reference is rejected, as roxmltree rejects it.

use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
};

/// The report is not one roxmltree accepts; the caller renders its message.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Malformed;

type Res<T> = Result<T, Malformed>;

const XML_PREFIX: &str = "xml";
const XMLNS: &str = "xmlns";
const XML_URI: &str = "http://www.w3.org/XML/1998/namespace";
const XMLNS_URI: &str = "http://www.w3.org/2000/xmlns/";
/// roxmltree refuses to intern more distinct (prefix, URI) pairs than this,
/// counting its predeclared `xml` binding.
const MAX_NAMESPACES: usize = u16::MAX as usize;

/// One markup event. Values are raw slices of the report.
pub(super) enum Token<'a> {
    ProcessingInstruction,
    Comment,
    /// `<prefix:local`
    ElementStart(&'a str, &'a str),
    /// `prefix:local="raw value"`
    Attribute(&'a str, &'a str, &'a str),
    ElementEnd(End<'a>),
    /// Character data between markup, before reference expansion.
    Text(&'a str),
    /// `<![CDATA[...]]>` content.
    Cdata(&'a str),
}

/// How a start tag ended, or a close tag.
pub(super) enum End<'a> {
    /// `>`
    Open,
    /// `/>`
    Empty,
    /// `</prefix:local>`
    Close(&'a str, &'a str),
}

/// Receiver of [`tokenize`] events.
pub(super) trait Events<'a> {
    type Error: From<Malformed>;
    fn token(&mut self, token: Token<'a>) -> Result<(), Self::Error>;
}

/// The importer-relevant place of an element in the report.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(super) enum Role {
    /// The document element.
    Root,
    /// The first `<sources>` child of the root.
    Sources,
    /// A `<source>` child of [`Role::Sources`].
    Source,
    /// The first `<packages>` child of the root.
    Packages,
    /// A `<package>` child of [`Role::Packages`].
    Package,
    /// The first `<classes>` child of a package.
    Classes,
    /// A `<class>` child of [`Role::Classes`].
    Class,
    /// The first `<lines>` child of a class.
    Lines,
    /// A `<line>` child of [`Role::Lines`].
    Line,
    #[default]
    Other,
}

/// An open element's role and which of its "first" children it has seen.
#[derive(Clone, Copy)]
pub(super) struct Level {
    pub(super) role: Role,
    first_taken: [bool; 2],
}

impl Level {
    pub(super) fn new(role: Role) -> Self {
        Self {
            role,
            first_taken: [false; 2],
        }
    }

    /// The role of a child element named `local`, by local name only, as the
    /// DOM importer matched `tag_name().name()`.
    pub(super) fn child(&mut self, local: &str) -> Role {
        let mut first = |slot: usize, role: Role| {
            if std::mem::replace(&mut self.first_taken[slot], true) {
                Role::Other
            } else {
                role
            }
        };
        match (self.role, local) {
            (Role::Root, "sources") => first(0, Role::Sources),
            (Role::Root, "packages") => first(1, Role::Packages),
            (Role::Sources, "source") => Role::Source,
            (Role::Packages, "package") => Role::Package,
            (Role::Package, "classes") => first(0, Role::Classes),
            (Role::Classes, "class") => Role::Class,
            (Role::Class, "lines") => first(0, Role::Lines),
            (Role::Lines, "line") => Role::Line,
            _ => Role::Other,
        }
    }
}

/// The role of a new element whose parent is `parent` (`None` for the root).
pub(super) fn role_of(parent: Option<&mut Level>, local: &str) -> Role {
    parent.map_or(Role::Root, |level| level.child(local))
}

/// Text children of one `<source>` element.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct SourceText {
    /// Whether the `<source>` has any element child.
    pub(super) has_element: bool,
    /// Its concatenated text and CDATA children, as roxmltree decoded them.
    pub(super) text: String,
}

/// What the importer needs before streaming classes, gathered by [`check`].
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Index<'a> {
    /// Local name of the document element.
    pub(super) root: &'a str,
    /// Number of `<sources>` children of the root.
    pub(super) sources: usize,
    /// Number of `<packages>` children of the root.
    pub(super) packages: usize,
    /// Number of `<package>` children of the first `<packages>`.
    pub(super) package: usize,
    /// The `<source>` children of the first `<sources>`, in order.
    pub(super) prefixes: Vec<SourceText>,
    /// In start-tag order: for every [`Role::Package`] its number of `<classes>`
    /// children, and for every [`Role::Class`] its number of `<lines>` children.
    pub(super) counts: Vec<usize>,
}

/// Accept `text` exactly when roxmltree accepts it with at most `nodes_limit`
/// nodes, and index it for the importer.
pub(super) fn check(text: &str, nodes_limit: u32) -> Res<Index<'_>> {
    let mut checker = Checker {
        nodes_limit,
        nodes: 1,
        in_text: false,
        stack: Vec::new(),
        tag: Pending::default(),
        attributes: Vec::new(),
        namespaces: Vec::new(),
        latest: HashMap::new(),
        own_namespaces: 0,
        interned: HashSet::from([(Some(XML_PREFIX), Cow::Borrowed(XML_URI))]),
        root_seen: false,
        index: Index::default(),
    };
    tokenize(text, &mut checker)?;
    if !checker.root_seen || !checker.stack.is_empty() {
        return Err(Malformed);
    }
    Ok(checker.index)
}

/// The start tag being read.
#[derive(Default)]
struct Pending<'a> {
    prefix: &'a str,
    local: &'a str,
    role: Role,
    slot: usize,
}

/// An open element.
struct Open<'a> {
    prefix: &'a str,
    local: &'a str,
    level: Level,
    /// Where its own namespace declarations start in `Checker::namespaces`.
    namespaces: usize,
    /// Its entry in `Index::counts` or `Index::prefixes`.
    slot: usize,
}

/// One in-scope namespace declaration.
struct Declaration<'a> {
    name: Option<&'a str>,
    uri: Cow<'a, str>,
    /// The declaration of the same prefix it shadows, restored when it goes out of scope.
    shadows: Option<usize>,
}

/// Well-formedness, namespace and resource checks of roxmltree's tree builder.
struct Checker<'a> {
    nodes_limit: u32,
    /// Nodes roxmltree would have allocated, starting with its document node.
    nodes: u32,
    /// Whether the last node was text, so new text or CDATA merges into it.
    in_text: bool,
    stack: Vec<Open<'a>>,
    tag: Pending<'a>,
    /// Ordinary attributes of the pending start tag.
    attributes: Vec<(&'a str, &'a str)>,
    /// In-scope declarations, outermost first; `None` is the default namespace.
    namespaces: Vec<Declaration<'a>>,
    /// The innermost live declaration of each prefix, as an index into
    /// `namespaces`, so a lookup does not scan every live declaration.
    latest: HashMap<&'a str, usize>,
    /// Where the pending start tag's own declarations start.
    own_namespaces: usize,
    /// Distinct declarations, as roxmltree interns them.
    interned: HashSet<(Option<&'a str>, Cow<'a, str>)>,
    root_seen: bool,
    index: Index<'a>,
}

impl<'a> Checker<'a> {
    fn add_node(&mut self) -> Res<()> {
        if self.nodes >= self.nodes_limit {
            return Err(Malformed);
        }
        self.nodes += 1;
        Ok(())
    }

    fn add_text(&mut self, text: &'a str, cdata: bool) -> Res<()> {
        if !cdata {
            validate_references(text)?;
        }
        if !self.in_text {
            self.add_node()?;
            self.in_text = true;
        }
        if let Some(open) = self.stack.last() {
            if open.level.role == Role::Source {
                let target = &mut self.index.prefixes[open.slot].text;
                if cdata {
                    decode_cdata(text, target);
                } else {
                    decode_text(text, target);
                }
            }
        }
        Ok(())
    }

    fn intern(&mut self, name: Option<&'a str>, uri: Cow<'a, str>) -> Res<()> {
        let key = (name, uri);
        if !self.interned.contains(&key) {
            if self.interned.len() > MAX_NAMESPACES {
                return Err(Malformed);
            }
            self.interned.insert(key.clone());
        }
        let (name, uri) = key;
        let shadows = name.and_then(|name| self.latest.insert(name, self.namespaces.len()));
        self.namespaces.push(Declaration { name, uri, shadows });
        Ok(())
    }

    fn lookup(&self, prefix: &str) -> Option<&str> {
        self.latest
            .get(prefix)
            .map(|&index| self.namespaces[index].uri.as_ref())
    }

    /// Drop the declarations from `len` on, restoring the ones they shadowed.
    fn truncate_namespaces(&mut self, len: usize) {
        while self.namespaces.len() > len {
            let declaration = self.namespaces.pop().expect("longer than len");
            if let Some(name) = declaration.name {
                match declaration.shadows {
                    Some(previous) => self.latest.insert(name, previous),
                    None => self.latest.remove(name),
                };
            }
        }
    }

    fn element_start(&mut self, prefix: &'a str, local: &'a str) -> Res<()> {
        if prefix == XMLNS {
            return Err(Malformed);
        }
        let parent = self.stack.last_mut();
        let parent_role = parent.as_ref().map(|open| (open.level.role, open.slot));
        let role = role_of(parent.map(|open| &mut open.level), local);
        match parent_role {
            Some((Role::Root, _)) if local == "sources" => self.index.sources += 1,
            Some((Role::Root, _)) if local == "packages" => self.index.packages += 1,
            Some((Role::Packages, _)) if local == "package" => self.index.package += 1,
            Some((Role::Package, slot)) if local == "classes" => self.index.counts[slot] += 1,
            Some((Role::Class, slot)) if local == "lines" => self.index.counts[slot] += 1,
            Some((Role::Source, slot)) => self.index.prefixes[slot].has_element = true,
            _ => {}
        }
        let slot = match role {
            Role::Root => {
                self.index.root = local;
                0
            }
            Role::Package | Role::Class => {
                self.index.counts.push(0);
                self.index.counts.len() - 1
            }
            Role::Source => {
                self.index.prefixes.push(SourceText::default());
                self.index.prefixes.len() - 1
            }
            _ => 0,
        };
        self.tag = Pending {
            prefix,
            local,
            role,
            slot,
        };
        self.own_namespaces = self.namespaces.len();
        Ok(())
    }

    fn attribute(&mut self, prefix: &'a str, local: &'a str, raw: &'a str) -> Res<()> {
        let value = normalize_attribute(raw)?;
        if prefix == XMLNS {
            let xml_uri = value == XML_URI;
            if value == XMLNS_URI || (local == XML_PREFIX) != xml_uri {
                return Err(Malformed);
            }
            // The prefix's innermost declaration is this tag's own only if it was
            // declared at or after the tag's first declaration.
            if self
                .latest
                .get(local)
                .is_some_and(|&index| index >= self.own_namespaces)
            {
                return Err(Malformed);
            }
            if !xml_uri {
                self.intern(Some(local), value)?;
            }
        } else if local == XMLNS {
            if value == XML_URI || value == XMLNS_URI {
                return Err(Malformed);
            }
            self.intern(None, value)?;
        } else {
            self.attributes.push((prefix, local));
        }
        Ok(())
    }

    /// The namespace URI of an attribute prefix; unprefixed attributes have none.
    fn attribute_namespace(&self, prefix: &str) -> Res<Option<&str>> {
        match prefix {
            XML_PREFIX => Ok(Some(XML_URI)),
            "" => Ok(None),
            _ => self.lookup(prefix).map(Some).ok_or(Malformed),
        }
    }

    fn start_tag_end(&mut self, open: bool) -> Res<()> {
        // Reject two attributes with one expanded name, without allocating:
        // URIs are compared only when local names already match.
        for (index, &(prefix, local)) in self.attributes.iter().enumerate() {
            let uri = self.attribute_namespace(prefix)?;
            for &(seen_prefix, seen_local) in &self.attributes[..index] {
                if seen_local == local && self.attribute_namespace(seen_prefix)? == uri {
                    return Err(Malformed);
                }
            }
        }
        self.attributes.clear();
        let tag = std::mem::take(&mut self.tag);
        if !tag.prefix.is_empty() && self.lookup(tag.prefix).is_none() {
            return Err(Malformed);
        }
        self.add_node()?;
        self.root_seen = true;
        if open {
            self.stack.push(Open {
                prefix: tag.prefix,
                local: tag.local,
                level: Level::new(tag.role),
                namespaces: self.own_namespaces,
                slot: tag.slot,
            });
        } else {
            self.truncate_namespaces(self.own_namespaces);
        }
        Ok(())
    }

    fn close(&mut self, prefix: &str, local: &str) -> Res<()> {
        let open = self.stack.pop().ok_or(Malformed)?;
        if open.prefix != prefix || open.local != local {
            return Err(Malformed);
        }
        self.truncate_namespaces(open.namespaces);
        Ok(())
    }
}

impl<'a> Events<'a> for Checker<'a> {
    type Error = Malformed;

    fn token(&mut self, token: Token<'a>) -> Res<()> {
        match token {
            Token::ProcessingInstruction | Token::Comment => {
                self.in_text = false;
                self.add_node()
            }
            Token::ElementStart(prefix, local) => {
                self.in_text = false;
                self.element_start(prefix, local)
            }
            Token::Attribute(prefix, local, raw) => self.attribute(prefix, local, raw),
            Token::ElementEnd(end) => {
                self.in_text = false;
                match end {
                    End::Open => self.start_tag_end(true),
                    End::Empty => self.start_tag_end(false),
                    End::Close(prefix, local) => self.close(prefix, local),
                }
            }
            Token::Text(text) => self.add_text(text, false),
            Token::Cdata(text) => self.add_text(text, true),
        }
    }
}

/// Reject a reference other than a predefined or numeric character reference.
fn validate_references(text: &str) -> Res<()> {
    let mut stream = Stream::new(text);
    while let Some(offset) = text.as_bytes()[stream.pos..]
        .iter()
        .position(|&b| b == b'&')
    {
        stream.pos += offset;
        match stream.consume_reference() {
            Some(Reference::Char(_)) => {}
            _ => return Err(Malformed),
        }
    }
    Ok(())
}

/// Append text decoded as roxmltree decodes a text token without entities:
/// line ends are normalized, but a character reference's bytes are kept as is
/// and so is the raw byte after one.
fn decode_text(text: &str, target: &mut String) {
    if !text.bytes().any(|b| b == b'&' || b == b'\r') {
        target.push_str(text);
        return;
    }
    let mut buffer: Vec<u8> = Vec::with_capacity(text.len());
    let mut as_is = false;
    let mut stream = Stream::new(text);
    while !stream.at_end() {
        let byte = stream.curr_unchecked();
        if byte == b'&' {
            // References were validated before decoding.
            if let Some(Reference::Char(c)) = stream.consume_reference() {
                let mut utf8 = [0; 4];
                buffer.extend_from_slice(c.encode_utf8(&mut utf8).as_bytes());
                as_is = true;
            }
            continue;
        }
        stream.advance(1);
        if as_is {
            buffer.push(byte);
            as_is = false;
        } else {
            push_text_byte(&mut buffer, byte, stream.at_end());
        }
    }
    target.push_str(&String::from_utf8_lossy(&buffer));
}

/// Translate `\r\n` and a lone `\r` into `\n`, as roxmltree does byte by byte.
fn push_text_byte(buffer: &mut Vec<u8>, byte: u8, at_end: bool) {
    if let Some(last) = buffer.last_mut().filter(|last| **last == b'\r') {
        *last = b'\n';
        if at_end && byte == b'\r' {
            buffer.push(b'\n');
        } else if byte != b'\n' {
            buffer.push(byte);
        }
    } else if at_end && byte == b'\r' {
        buffer.push(b'\n');
    } else {
        buffer.push(byte);
    }
}

/// Append CDATA content with `\r\n` and lone `\r` translated into `\n`.
fn decode_cdata(text: &str, target: &mut String) {
    let mut rest = text;
    while let Some(index) = rest.find('\r') {
        target.push_str(&rest[..index]);
        target.push('\n');
        let after = &rest[index + 1..];
        rest = after.strip_prefix('\n').unwrap_or(after);
    }
    target.push_str(rest);
}

/// Decode an attribute value as roxmltree does: references are expanded,
/// `\r\n` becomes one space and every other `\t`, `\n` or `\r` a space, while a
/// character reference's bytes are kept as is.
pub(super) fn normalize_attribute(raw: &str) -> Res<Cow<'_, str>> {
    if !raw
        .bytes()
        .any(|b| matches!(b, b'&' | b'\t' | b'\n' | b'\r'))
    {
        return Ok(Cow::Borrowed(raw));
    }
    let mut buffer = String::with_capacity(raw.len());
    let mut stream = Stream::new(raw);
    while !stream.at_end() {
        let byte = stream.curr_unchecked();
        if byte == b'&' {
            match stream.consume_reference() {
                Some(Reference::Char(c)) => buffer.push(c),
                _ => return Err(Malformed),
            }
            continue;
        }
        let c = stream.chars().next().ok_or(Malformed)?;
        stream.advance(c.len_utf8());
        match c {
            '\r' if stream.curr().ok() == Some(b'\n') => {}
            '\t' | '\n' | '\r' => buffer.push(' '),
            _ => buffer.push(c),
        }
    }
    Ok(Cow::Owned(buffer))
}

/// Emit the tokens of `text`, rejecting what roxmltree's tokenizer rejects.
pub(super) fn tokenize<'a, E: Events<'a>>(text: &'a str, events: &mut E) -> Result<(), E::Error> {
    let s = &mut Stream::new(text);
    parse_declaration(s)?;
    parse_misc(s, events)?;
    s.skip_spaces();
    if s.starts_with(b"<!DOCTYPE") {
        return Err(Malformed.into());
    }
    if s.curr().ok() == Some(b'<') && parse_start_tag(s, events)? {
        parse_content(s, events)?;
    }
    parse_misc(s, events)?;
    if !s.at_end() {
        return Err(Malformed.into());
    }
    Ok(())
}

/// `Misc ::= Comment | PI | S`
fn parse_misc<'a, E: Events<'a>>(s: &mut Stream<'a>, events: &mut E) -> Result<(), E::Error> {
    while !s.at_end() {
        s.skip_spaces();
        if s.starts_with(b"<!--") {
            parse_comment(s, events)?;
        } else if s.starts_with(b"<?") {
            parse_pi(s, events)?;
        } else {
            break;
        }
    }
    Ok(())
}

/// `XMLDecl ::= '<?xml' VersionInfo EncodingDecl? SDDecl? S? '?>'`, validated only.
fn parse_declaration(s: &mut Stream<'_>) -> Res<()> {
    if s.starts_with("\u{feff}".as_bytes()) {
        s.advance(3);
    }
    if !s.starts_with(b"<?xml ") {
        return Ok(());
    }
    fn spaces(s: &mut Stream<'_>) -> Res<()> {
        if s.starts_with_space() {
            s.skip_spaces();
        } else if !s.starts_with(b"?>") && !s.at_end() {
            return Err(Malformed);
        }
        Ok(())
    }
    s.advance(5);
    spaces(s)?;
    if !s.starts_with(b"version") {
        return Err(Malformed);
    }
    parse_declaration_attribute(s)?;
    spaces(s)?;
    if s.starts_with(b"encoding") {
        parse_declaration_attribute(s)?;
        spaces(s)?;
    }
    if s.starts_with(b"standalone") {
        parse_declaration_attribute(s)?;
    }
    s.skip_spaces();
    s.skip_string(b"?>")
}

/// `Attribute ::= Name Eq AttValue` inside the XML declaration.
fn parse_declaration_attribute(s: &mut Stream<'_>) -> Res<()> {
    s.consume_qname()?;
    s.consume_eq()?;
    let quote = s.consume_quote()? as char;
    s.skip_chars(|_, c| c != quote && c != '<')?;
    s.consume_byte(quote as u8)
}

/// `'<!--' ((Char - '-') | ('-' (Char - '-')))* '-->'`
fn parse_comment<'a, E: Events<'a>>(s: &mut Stream<'a>, events: &mut E) -> Result<(), E::Error> {
    s.advance(4);
    let text = s.consume_chars(|s, c| !(c == '-' && s.starts_with(b"-->")))?;
    s.skip_string(b"-->")?;
    if text.contains("--") || text.ends_with('-') {
        return Err(Malformed.into());
    }
    events.token(Token::Comment)
}

/// `PI ::= '<?' PITarget (S (Char* - (Char* '?>' Char*)))? '?>'`
fn parse_pi<'a, E: Events<'a>>(s: &mut Stream<'a>, events: &mut E) -> Result<(), E::Error> {
    if s.starts_with(b"<?xml ") {
        return Err(Malformed.into());
    }
    s.advance(2);
    s.consume_name()?;
    s.skip_spaces();
    s.consume_chars(|s, c| !(c == '?' && s.starts_with(b"?>")))?;
    s.skip_string(b"?>")?;
    events.token(Token::ProcessingInstruction)
}

/// `'<' Name (S Attribute)* S? ('/>' | '>')`; `true` when content follows.
fn parse_start_tag<'a, E: Events<'a>>(
    s: &mut Stream<'a>,
    events: &mut E,
) -> Result<bool, E::Error> {
    s.advance(1);
    let (prefix, local) = s.consume_qname()?;
    events.token(Token::ElementStart(prefix, local))?;
    while !s.at_end() {
        let has_space = s.starts_with_space();
        s.skip_spaces();
        match s.curr()? {
            b'/' => {
                s.advance(1);
                s.consume_byte(b'>')?;
                events.token(Token::ElementEnd(End::Empty))?;
                return Ok(false);
            }
            b'>' => {
                s.advance(1);
                events.token(Token::ElementEnd(End::Open))?;
                return Ok(true);
            }
            _ => {
                if !has_space {
                    return Err(Malformed.into());
                }
                let (prefix, local) = s.consume_qname()?;
                s.consume_eq()?;
                let quote = s.consume_quote()?;
                let start = s.pos;
                let length = s
                    .bytes()
                    .iter()
                    .position(|&b| b == quote || b == b'<')
                    .ok_or(Malformed)?;
                s.advance(length);
                let value = &s.text[start..s.pos];
                if !is_xml_str(value) {
                    return Err(Malformed.into());
                }
                s.consume_byte(quote)?;
                events.token(Token::Attribute(prefix, local, value))?;
            }
        }
    }
    Ok(false)
}

/// `content ::= CharData? ((element | Reference | CDSect | PI | Comment) CharData?)*`,
/// iterative so deep nesting cannot overflow the stack. Ends after the close tag
/// of the element whose content this is.
fn parse_content<'a, E: Events<'a>>(s: &mut Stream<'a>, events: &mut E) -> Result<(), E::Error> {
    let mut depth: u32 = 0;
    while !s.at_end() {
        if s.curr_unchecked() != b'<' {
            parse_text(s, events)?;
            continue;
        }
        match s.next_byte()? {
            b'!' if s.starts_with(b"<!--") => parse_comment(s, events)?,
            b'!' if s.starts_with(b"<![CDATA[") => parse_cdata(s, events)?,
            b'!' => return Err(Malformed.into()),
            b'?' => parse_pi(s, events)?,
            b'/' => {
                parse_close_element(s, events)?;
                if depth == 0 {
                    break;
                }
                depth -= 1;
            }
            _ => {
                if parse_start_tag(s, events)? {
                    depth = depth.checked_add(1).ok_or(Malformed)?;
                }
            }
        }
    }
    Ok(())
}

/// `'<![CDATA[' (Char* - (Char* ']]>' Char*)) ']]>'`
fn parse_cdata<'a, E: Events<'a>>(s: &mut Stream<'a>, events: &mut E) -> Result<(), E::Error> {
    s.advance(9);
    let text = s.consume_chars(|s, c| !(c == ']' && s.starts_with(b"]]>")))?;
    s.skip_string(b"]]>")?;
    events.token(Token::Cdata(text))
}

/// `'</' Name S? '>'`
fn parse_close_element<'a, E: Events<'a>>(
    s: &mut Stream<'a>,
    events: &mut E,
) -> Result<(), E::Error> {
    s.advance(2);
    let (prefix, local) = s.consume_qname()?;
    s.skip_spaces();
    s.consume_byte(b'>')?;
    events.token(Token::ElementEnd(End::Close(prefix, local)))
}

/// Character data up to the next `<`, which must not contain `]]>`.
fn parse_text<'a, E: Events<'a>>(s: &mut Stream<'a>, events: &mut E) -> Result<(), E::Error> {
    let length = s
        .bytes()
        .iter()
        .position(|&b| b == b'<')
        .unwrap_or(s.bytes().len());
    let text = &s.text[s.pos..s.pos + length];
    if !is_xml_str(text) {
        return Err(Malformed.into());
    }
    s.advance(length);
    if text.contains("]]>") {
        return Err(Malformed.into());
    }
    events.token(Token::Text(text))
}

/// A reference that roxmltree resolves without a DTD, or a named one it cannot.
enum Reference {
    Char(char),
    /// A named entity; none can be declared, so it is never resolvable.
    Entity,
}

/// A cursor over the report.
struct Stream<'a> {
    text: &'a str,
    pos: usize,
}

impl<'a> Stream<'a> {
    fn new(text: &'a str) -> Self {
        Self { text, pos: 0 }
    }

    fn at_end(&self) -> bool {
        self.pos >= self.text.len()
    }

    fn bytes(&self) -> &'a [u8] {
        &self.text.as_bytes()[self.pos..]
    }

    fn chars(&self) -> std::str::Chars<'a> {
        self.text[self.pos..].chars()
    }

    fn curr(&self) -> Res<u8> {
        self.bytes().first().copied().ok_or(Malformed)
    }

    fn curr_unchecked(&self) -> u8 {
        self.text.as_bytes()[self.pos]
    }

    fn next_byte(&self) -> Res<u8> {
        self.bytes().get(1).copied().ok_or(Malformed)
    }

    fn advance(&mut self, n: usize) {
        self.pos += n;
    }

    fn starts_with(&self, prefix: &[u8]) -> bool {
        self.bytes().starts_with(prefix)
    }

    fn consume_byte(&mut self, byte: u8) -> Res<()> {
        if self.curr()? != byte {
            return Err(Malformed);
        }
        self.advance(1);
        Ok(())
    }

    fn try_consume_byte(&mut self, byte: u8) -> bool {
        let found = self.curr() == Ok(byte);
        if found {
            self.advance(1);
        }
        found
    }

    fn skip_string(&mut self, text: &[u8]) -> Res<()> {
        if !self.starts_with(text) {
            return Err(Malformed);
        }
        self.advance(text.len());
        Ok(())
    }

    fn consume_bytes(&mut self, keep: impl Fn(u8) -> bool) -> &'a str {
        let start = self.pos;
        let length = self.bytes().iter().take_while(|&&b| keep(b)).count();
        self.advance(length);
        &self.text[start..self.pos]
    }

    /// Advance over XML characters while `keep` holds; a non-XML character
    /// before that point is an error.
    fn skip_chars(&mut self, keep: impl Fn(&Self, char) -> bool) -> Res<()> {
        for c in self.chars() {
            if !is_xml_char(c) {
                return Err(Malformed);
            }
            if !keep(self, c) {
                break;
            }
            self.advance(c.len_utf8());
        }
        Ok(())
    }

    fn consume_chars(&mut self, keep: impl Fn(&Self, char) -> bool) -> Res<&'a str> {
        let start = self.pos;
        self.skip_chars(keep)?;
        Ok(&self.text[start..self.pos])
    }

    fn starts_with_space(&self) -> bool {
        self.curr().is_ok_and(is_xml_space)
    }

    fn skip_spaces(&mut self) {
        while self.starts_with_space() {
            self.advance(1);
        }
    }

    /// `Reference ::= EntityRef | CharRef`; `None` when malformed.
    fn consume_reference(&mut self) -> Option<Reference> {
        if !self.try_consume_byte(b'&') {
            return None;
        }
        let reference = if self.try_consume_byte(b'#') {
            let (digits, radix) = if self.try_consume_byte(b'x') {
                (self.consume_bytes(|b| b.is_ascii_hexdigit()), 16)
            } else {
                (self.consume_bytes(|b| b.is_ascii_digit()), 10)
            };
            let value = u32::from_str_radix(digits, radix).ok()?;
            let c = char::from_u32(value).unwrap_or('\u{FFFD}');
            if !is_xml_char(c) {
                return None;
            }
            Reference::Char(c)
        } else {
            match self.consume_name().ok()? {
                "quot" => Reference::Char('"'),
                "amp" => Reference::Char('&'),
                "apos" => Reference::Char('\''),
                "lt" => Reference::Char('<'),
                "gt" => Reference::Char('>'),
                _ => Reference::Entity,
            }
        };
        self.consume_byte(b';').ok()?;
        Some(reference)
    }

    /// `Name`; an empty name is an error.
    fn consume_name(&mut self) -> Res<&'a str> {
        let start = self.pos;
        let mut chars = self.chars();
        if let Some(c) = chars.next() {
            if !is_name_start_char(c) {
                return Err(Malformed);
            }
            self.advance(c.len_utf8());
        }
        for c in chars.take_while(|&c| is_name_char(c)) {
            self.advance(c.len_utf8());
        }
        if self.pos == start {
            return Err(Malformed);
        }
        Ok(&self.text[start..self.pos])
    }

    /// `QName`, returned as `(prefix, local)` with an empty prefix when absent.
    fn consume_qname(&mut self) -> Res<(&'a str, &'a str)> {
        let start = self.pos;
        let mut splitter = None;
        while let Some(&byte) = self.bytes().first() {
            let width = if byte.is_ascii() {
                if byte == b':' {
                    if splitter.is_some() {
                        return Err(Malformed);
                    }
                    splitter = Some(self.pos);
                } else if !is_name_char(byte as char) {
                    break;
                }
                1
            } else {
                match self.chars().next() {
                    Some(c) if is_name_char(c) => c.len_utf8(),
                    _ => break,
                }
            };
            self.advance(width);
        }
        let (prefix, local) = match splitter {
            Some(colon) => (&self.text[start..colon], &self.text[colon + 1..self.pos]),
            None => ("", &self.text[start..self.pos]),
        };
        let starts_name = |name: &str| name.chars().next().is_some_and(is_name_start_char);
        if (!prefix.is_empty() && !starts_name(prefix)) || !starts_name(local) {
            return Err(Malformed);
        }
        Ok((prefix, local))
    }

    fn consume_eq(&mut self) -> Res<()> {
        self.skip_spaces();
        self.consume_byte(b'=')?;
        self.skip_spaces();
        Ok(())
    }

    fn consume_quote(&mut self) -> Res<u8> {
        let quote = self.curr()?;
        if quote != b'\'' && quote != b'"' {
            return Err(Malformed);
        }
        self.advance(1);
        Ok(quote)
    }
}

fn is_xml_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r')
}

/// Whether every character of `text` is an XML character.
fn is_xml_str(text: &str) -> bool {
    if text.is_ascii() {
        text.bytes().all(|b| b >= 0x20 || is_xml_space(b))
    } else {
        text.chars().all(is_xml_char)
    }
}

/// <https://www.w3.org/TR/xml/#NT-Char>; surrogates cannot occur in `str`.
fn is_xml_char(c: char) -> bool {
    match c as u32 {
        0..=0x1F => is_xml_space(c as u8),
        0xFFFE | 0xFFFF => false,
        _ => true,
    }
}

/// <https://www.w3.org/TR/xml/#NT-NameStartChar>
fn is_name_start_char(c: char) -> bool {
    matches!(c as u32,
        0x3A | 0x41..=0x5A | 0x5F | 0x61..=0x7A
        | 0xC0..=0xD6 | 0xD8..=0xF6 | 0xF8..=0x2FF | 0x370..=0x37D
        | 0x37F..=0x1FFF | 0x200C..=0x200D | 0x2070..=0x218F | 0x2C00..=0x2FEF
        | 0x3001..=0xD7FF | 0xF900..=0xFDCF | 0xFDF0..=0xFFFD | 0x10000..=0xEFFFF)
}

/// <https://www.w3.org/TR/xml/#NT-NameChar>
fn is_name_char(c: char) -> bool {
    is_name_start_char(c)
        || matches!(c as u32,
            0x2D | 0x2E | 0x30..=0x39 | 0xB7 | 0x300..=0x36F | 0x203F..=0x2040)
}
