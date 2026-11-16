//! Differential tests: the streaming reader against the DOM importer it replaced.
//!
//! Every case must give the same evidence, or the same error kind and complete
//! message chain. Generated cases use fixed seeds, so a failure reproduces.

use super::{dom_oracle, import_xml, xml, LineEvidence};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

/// A scan root with selected and unselected files.
struct Env {
    _root: tempfile::TempDir,
    canonical: PathBuf,
    sources: BTreeMap<String, usize>,
}

impl Env {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        for (relative, lines) in [
            ("src/A.cs", 5),
            ("src/B.cs", 3),
            ("src/sub/C.cs", 2),
            ("other/Unselected.cs", 4),
        ] {
            let path = root.path().join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, "x\n".repeat(lines)).unwrap();
        }
        let canonical = root.path().canonicalize().unwrap();
        let sources = BTreeMap::from([
            ("src/A.cs".to_owned(), 5),
            ("src/B.cs".to_owned(), 3),
            ("src/sub/C.cs".to_owned(), 2),
        ]);
        Self {
            _root: root,
            canonical,
            sources,
        }
    }

    fn root(&self) -> &Path {
        &self.canonical
    }

    /// Run one importer and render its outcome for comparison.
    fn outcome(
        &self,
        import: impl Fn(&Path, &BTreeMap<String, usize>, &str, &mut LineEvidence) -> crate::Result<()>,
        text: &str,
    ) -> String {
        let mut evidence = LineEvidence::new();
        match import(self.root(), &self.sources, text, &mut evidence) {
            Ok(()) => format!("Ok({evidence:?})"),
            Err(error) => format!("Err({:?}: {error:#})", error.kind()),
        }
    }

    /// Assert both importers agree on `text`; return the shared outcome.
    fn assert_same(&self, text: &str) -> String {
        let old = self.outcome(dom_oracle::import_xml, text);
        let new = self.outcome(import_xml, text);
        assert_eq!(new, old, "streaming and DOM importers disagree on {text:?}");
        assert_index_matches_dom(text);
        old
    }
}

/// For an accepted document, the first pass's index must equal what a tree walk
/// derives, and the streamed attribute values must equal the DOM's.
fn assert_index_matches_dom(text: &str) {
    let options = roxmltree::ParsingOptions {
        nodes_limit: super::NODES_LIMIT,
        allow_dtd: false,
        entity_resolver: None,
    };
    let dom = roxmltree::Document::parse_with_options(text, options);
    let checked = xml::check(text, super::NODES_LIMIT);
    assert_eq!(
        checked.is_ok(),
        dom.is_ok(),
        "acceptance differs on {text:?}: {:?}",
        dom.as_ref().err()
    );
    let (Ok(index), Ok(dom)) = (checked, dom) else {
        return;
    };
    assert_eq!(index, dom_index(&dom), "index differs on {text:?}");
    assert_eq!(
        streamed_elements(text),
        dom_elements(&dom),
        "elements differ on {text:?}"
    );
}

fn elements<'a, 'b>(
    node: roxmltree::Node<'a, 'b>,
    name: &'a str,
) -> impl Iterator<Item = roxmltree::Node<'a, 'b>> + 'a {
    node.children()
        .filter(move |child| child.is_element() && child.tag_name().name() == name)
}

/// The index a tree walk derives, in the same start-tag order.
fn dom_index<'a>(dom: &roxmltree::Document<'a>) -> xml::Index<'a> {
    let root = dom.root_element();
    let mut index = xml::Index {
        root: root.tag_name().name(),
        sources: elements(root, "sources").count(),
        packages: elements(root, "packages").count(),
        ..Default::default()
    };
    if let Some(sources) = elements(root, "sources").next() {
        for source in elements(sources, "source") {
            index.prefixes.push(xml::SourceText {
                has_element: source.children().any(|child| child.is_element()),
                text: source
                    .children()
                    .filter(|child| child.is_text())
                    .filter_map(|child| child.text())
                    .collect(),
            });
        }
    }
    if let Some(packages) = elements(root, "packages").next() {
        index.package = elements(packages, "package").count();
        for package in elements(packages, "package") {
            index.counts.push(elements(package, "classes").count());
            if let Some(classes) = elements(package, "classes").next() {
                for class in elements(classes, "class") {
                    index.counts.push(elements(class, "lines").count());
                }
            }
        }
    }
    index
}

/// Every element's local name with its attributes' local names and values, in
/// document order.
type Elements = Vec<(String, Vec<(String, String)>)>;

fn dom_elements(dom: &roxmltree::Document<'_>) -> Elements {
    dom.descendants()
        .filter(|node| node.is_element())
        .map(|node| {
            let attributes = node
                .attributes()
                .map(|attribute| (attribute.name().to_owned(), attribute.value().to_owned()))
                .collect();
            (node.tag_name().name().to_owned(), attributes)
        })
        .collect()
}

fn streamed_elements(text: &str) -> Elements {
    struct Collect(Elements);
    impl<'a> xml::Events<'a> for Collect {
        type Error = xml::Malformed;
        fn token(&mut self, token: xml::Token<'a>) -> Result<(), xml::Malformed> {
            match token {
                xml::Token::ElementStart(_, local) => self.0.push((local.to_owned(), Vec::new())),
                xml::Token::Attribute(prefix, local, raw)
                    if prefix != "xmlns" && local != "xmlns" =>
                {
                    let value = xml::normalize_attribute(raw)?.into_owned();
                    self.0.last_mut().unwrap().1.push((local.to_owned(), value));
                }
                _ => {}
            }
            Ok(())
        }
    }
    let mut collect = Collect(Vec::new());
    xml::tokenize(text, &mut collect).unwrap();
    collect.0
}

/// SplitMix64: small, deterministic and good enough to pick test inputs.
struct Rng(u64, bool);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn chance(&mut self, percent: usize) -> bool {
        self.below(100) < percent
    }

    /// A chance of a deliberate fault, only in reports drawn as faulty.
    fn fault(&mut self, percent: usize) -> bool {
        self.1 && self.chance(percent)
    }

    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[self.below(items.len())]
    }
}

const FILENAMES: &[&str] = &[
    "src/A.cs",
    "src/B.cs",
    "src\\sub\\C.cs",
    "sub/C.cs",
    "A.cs",
    "other/Unselected.cs",
    "src/Missing.cs",
    "src/A.cs&#10;",
    "src/&#65;.cs",
    "src/A.cs\t",
    "src/A&amp;.cs",
    "&bogus;",
    "",
    "/_/src/A.cs",
    "C:/x/A.cs",
    "../A.cs",
    "src//A.cs",
    "src/",
    "http://x/A.cs",
];
const NUMBERS: &[&str] = &[
    "1",
    "2",
    "3",
    "4",
    "5",
    "6",
    "0",
    "01",
    "-1",
    " 1",
    "&#49;",
    "1&#10;",
    "1\r\n",
    "+1",
    "4294967295",
    "4294967296",
    "x",
];
const HITS: &[&str] = &[
    "0",
    "1",
    "7",
    "00",
    "18446744073709551615",
    "18446744073709551616",
    "&#50;",
    "-0",
    "1.0",
    "",
];
const PREFIXES: &[&str] = &[
    "{root}",
    "{root}/",
    "{root}/src",
    "src",
    "src/sub",
    "  ",
    "",
    "<![CDATA[src]]>",
    "s<!-- c -->rc",
    "s<?p?>rc",
    "src&#13;&#10;",
    "sr&#x63;",
    "{root}\r\n",
    "<x/>",
    "C:/foo",
    "/_/",
    "..",
];
const NAMES: &[&str] = &[
    "a", "methods", "method", "lines", "line", "class", "classes", "x:class", "p:lines",
];
const NOISE: &[&str] = &[
    " ",
    "\n",
    "\r\n",
    "\t",
    "<!-- note -->",
    "<?pi data?>",
    "<![CDATA[x\r\ny]]>",
    "&amp;",
    "&#13;",
    "text",
];
const MUTATIONS: &[&str] = &[
    "<",
    ">",
    "/>",
    "&",
    "&lt;",
    "&#0;",
    "&#xD800;",
    "&#x110000;",
    "&nbsp;",
    "\"",
    "'",
    "=",
    ":",
    "</a>",
    "<a>",
    "<!--",
    "--",
    "-->",
    "<![CDATA[",
    "]]>",
    "<?xml version='1.0'?>",
    "<?xml ",
    "<?pi?>",
    "<!DOCTYPE",
    "<!ELEMENT",
    "\u{feff}",
    "\u{0}",
    "\u{1}",
    "\u{fffe}",
    "é",
    "\u{b7}",
    " xmlns:x='u'",
    " xmlns:p='u'",
    " xmlns='u'",
    " xmlns:xml='http://www.w3.org/XML/1998/namespace'",
    " xmlns:y='http://www.w3.org/XML/1998/namespace'",
    " xmlns:z='http://www.w3.org/2000/xmlns/'",
    " x:a='1' p:a='2'",
    " a='1' a='2'",
    " xml:lang='en'",
    "<xmlns:a/>",
    "<xml:a/>",
    "\r",
    "\n",
];

/// A Cobertura-shaped report built from `rng`, mostly valid.
fn generate(rng: &mut Rng, root: &str) -> String {
    rng.1 = rng.chance(60);
    let mut xml = String::new();
    if rng.chance(30) {
        xml.push_str(rng.pick(&[
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n",
            "<?xml version='1.0' standalone='yes' ?>",
            "\u{feff}<?xml version=\"1.0\"?>",
            "<?xml version=\"1.0\"encoding=\"x\"?>",
            "<!-- head -->\n",
            "<?xml-ish?>",
        ]));
    }
    let coverage = rng.pick(&["coverage", "coverage", "coverage", "x:coverage", "report"]);
    xml.push_str(&format!("<{coverage}"));
    if coverage.starts_with("x:") || rng.chance(10) {
        xml.push_str(" xmlns:x=\"urn:x\"");
    }
    if rng.chance(20) {
        xml.push_str(" xmlns:p='urn:p' line-rate=\"1\"");
    }
    xml.push('>');
    noise(rng, &mut xml);
    let sources = if rng.fault(15) { 2 } else { rng.below(2) };
    for _ in 0..sources {
        xml.push_str("<sources>");
        for _ in 0..rng.below(3) {
            let prefix = if rng.fault(50) {
                rng.pick(PREFIXES)
            } else {
                rng.pick(&["{root}", "{root}/", "{root}/src"])
            };
            let prefix = prefix.replace("{root}", root);
            xml.push_str(&format!("<source>{prefix}</source>"));
        }
        xml.push_str("</sources>");
        noise(rng, &mut xml);
    }
    let packages = if rng.fault(8) { rng.below(3) } else { 1 };
    for _ in 0..packages {
        xml.push_str("<packages>");
        for _ in 0..rng.below(3) + usize::from(!rng.fault(10)) {
            xml.push_str("<package name=\"P\">");
            noise(rng, &mut xml);
            let classes = if rng.fault(8) { rng.below(3) } else { 1 };
            for _ in 0..classes {
                xml.push_str("<classes>");
                for _ in 0..rng.below(4) {
                    class(rng, &mut xml);
                }
                xml.push_str("</classes>");
            }
            xml.push_str("</package>");
        }
        xml.push_str("</packages>");
    }
    noise(rng, &mut xml);
    xml.push_str(&format!("</{coverage}>"));
    if rng.chance(20) {
        xml.push_str(rng.pick(&["\n", "<!-- tail -->", "<?tail?>", " <a/>", "x"]));
    }
    xml
}

fn class(rng: &mut Rng, xml: &mut String) {
    xml.push_str("<class name=\"C\"");
    if !rng.fault(5) {
        let quote = rng.pick(&["\"", "'"]);
        let attribute = rng.pick(&["filename", "filename", "filename", "x:filename"]);
        let filename = if rng.fault(30) {
            rng.pick(FILENAMES)
        } else {
            rng.pick(&[
                "src/A.cs",
                "src/B.cs",
                "src/sub/C.cs",
                "other/Unselected.cs",
            ])
        };
        xml.push_str(&format!(" {attribute}={quote}{filename}{quote}"));
    }
    xml.push('>');
    if rng.chance(30) {
        xml.push_str("<methods><method name=\"M\"><lines><line number=\"99\" hits=\"x\"/></lines></method></methods>");
    }
    let lines = if rng.fault(8) { rng.below(3) } else { 1 };
    for _ in 0..lines {
        xml.push_str("<lines>");
        for _ in 0..rng.below(5) {
            let number = if !rng.fault(15) {
                format!("{}", rng.below(2) + 1)
            } else {
                rng.pick(NUMBERS).to_owned()
            };
            let hits = if !rng.fault(15) {
                format!("{}", rng.below(4))
            } else {
                rng.pick(HITS).to_owned()
            };
            let name = if !rng.fault(5) {
                "line"
            } else {
                rng.pick(NAMES)
            };
            xml.push_str(&format!("<{name}"));
            if !rng.fault(3) {
                xml.push_str(&format!(" number=\"{number}\""));
            }
            if !rng.fault(3) {
                xml.push_str(&format!(" hits=\"{hits}\""));
            }
            if rng.chance(5) {
                xml.push_str(" branch=\"false\" x:number=\"9\"");
            }
            if name.contains(':') {
                xml.push_str(" xmlns:x='u' xmlns:p='v'");
            }
            xml.push_str("/>");
        }
        xml.push_str("</lines>");
    }
    xml.push_str("</class>");
}

fn noise(rng: &mut Rng, xml: &mut String) {
    if rng.chance(25) {
        xml.push_str(rng.pick(NOISE));
    }
    if rng.fault(5) {
        let name = rng.pick(NAMES);
        xml.push_str(&format!("<{name} /><other a='1'>t</other>"));
    }
}

/// Apply up to three random edits at character boundaries.
fn mutate(rng: &mut Rng, text: &str) -> String {
    let mut chars: Vec<char> = text.chars().collect();
    for _ in 0..=rng.below(3) {
        let at = rng.below(chars.len() + 1);
        match rng.below(4) {
            0 => {
                let insert: Vec<char> = rng.pick(MUTATIONS).chars().collect();
                chars.splice(at..at, insert);
            }
            1 => {
                let end = (at + rng.below(8)).min(chars.len());
                chars.drain(at..end);
            }
            2 => {
                let end = (at + rng.below(12)).min(chars.len());
                let copy: Vec<char> = chars[at..end].to_vec();
                let to = rng.below(chars.len() + 1);
                chars.splice(to..to, copy);
            }
            _ => chars.truncate(at),
        }
    }
    chars.into_iter().collect()
}

#[test]
fn generated_reports_import_identically() {
    let env = Env::new();
    let root = env.root().display().to_string();
    let mut rng = Rng(0x00C0_BE27_0001, false);
    let mut accepted = 0;
    for _ in 0..1500 {
        let text = generate(&mut rng, &root);
        if env.assert_same(&text).starts_with("Ok(") {
            accepted += 1;
        }
    }
    // The generator must reach evidence, not only errors.
    assert!(accepted > 100, "only {accepted} generated reports imported");
}

#[test]
fn mutated_reports_fail_identically() {
    let env = Env::new();
    let root = env.root().display().to_string();
    let mut rng = Rng(0x00C0_BE27_0002, false);
    let mut malformed = 0;
    let mut accepted = 0;
    for _ in 0..2500 {
        let report = generate(&mut rng, &root);
        let text = mutate(&mut rng, &report);
        let outcome = env.assert_same(&text);
        malformed += usize::from(outcome.contains("malformed XML"));
        accepted += usize::from(outcome.starts_with("Ok("));
    }
    assert!(
        malformed > 500,
        "only {malformed} mutated reports were malformed"
    );
    // Mutations rarely leave a report valid, and how often depends on the
    // temporary root's length (5 of 2500 on Windows). One proves the path runs.
    assert!(accepted > 0, "no mutated report imported");
}

/// Token soup exercises the tokenizer away from Cobertura's shape.
#[test]
fn random_markup_is_accepted_and_rejected_identically() {
    let env = Env::new();
    let mut rng = Rng(0x00C0_BE27_0003, false);
    let pieces: Vec<&str> = MUTATIONS.iter().chain(NOISE).copied().collect();
    for _ in 0..3000 {
        let mut text = String::from(rng.pick(&["<coverage>", "<a>", "<x:a xmlns:x='u'>", ""]));
        for _ in 0..rng.below(8) {
            text.push_str(rng.pick(&pieces));
        }
        text.push_str(rng.pick(&["</coverage>", "</a>", "</x:a>", ""]));
        env.assert_same(&text);
    }
}

#[test]
fn hostile_inputs_fail_identically() {
    let env = Env::new();
    let ok = "<coverage><packages><package><classes><class filename=\"src/A.cs\"><lines><line number=\"1\" hits=\"2\"/></lines></class></classes></package></packages></coverage>";
    let deep = format!("{}{}", "<a>".repeat(3000), "</a>".repeat(3000));
    let wide = format!(
        "<a{}/>",
        (0..300).map(|i| format!(" a{i}='{i}'")).collect::<String>()
    );
    let laughs = format!("<coverage>{}</coverage>", "&amp;".repeat(20_000));
    let many_ns = format!(
        "<a>{}</a>",
        (0..70_000)
            .map(|i| format!("<b xmlns:p='u{i}'/>"))
            .collect::<String>()
    );
    let same_ns = format!(
        "<a>{}</a>",
        (0..70_000).map(|_| "<b xmlns:p='u'/>").collect::<String>()
    );
    for text in [
        ok,
        "",
        " ",
        "\u{feff}",
        "\u{feff}\u{feff}<coverage/>",
        "<coverage",
        "<coverage>",
        "<coverage></coverage",
        "<coverage/><coverage/>",
        "<coverage/>text",
        "text<coverage/>",
        "<coverage></Coverage>",
        "<x:coverage></coverage>",
        "<x:coverage xmlns:x='u'></x:coverage>",
        "<x:coverage xmlns:x='u'><packages/></x:coverage>",
        "<coverage xmlns='u'><packages/></coverage>",
        "<coverage xmlns='u' xmlns='v'><packages/></coverage>",
        "<coverage xmlns:p='u' xmlns:p='u'/>",
        "<coverage xmlns:xml='http://www.w3.org/XML/1998/namespace' xmlns:xml='http://www.w3.org/XML/1998/namespace'/>",
        "<coverage xmlns:xml='u'/>",
        "<coverage xmlns:p='http://www.w3.org/XML/1998/namespace'/>",
        "<coverage xmlns='http://www.w3.org/XML/1998/namespace'/>",
        "<coverage xmlns='http://www.w3.org/2000/xmlns/'/>",
        "<coverage xmlns:p='http://www.w3.org/2000/xmlns/'/>",
        "<coverage xmlns:xmlns='u'/>",
        "<coverage q:xmlns='u'/>",
        "<coverage xmlns:p=''/>",
        "<xmlns:coverage/>",
        "<xml:coverage/>",
        "<coverage xml:lang='en' xml:lang='fr'/>",
        "<coverage xmlns:p='u' xmlns:q='u' p:a='1' q:a='2'/>",
        "<coverage xmlns:p='u' xmlns:q='v' p:a='1' q:a='2'/>",
        "<coverage p:a='1'/>",
        "<coverage a='1' a='1'/>",
        "<coverage a='1'b='2'/>",
        "<coverage a = '1' />",
        "<coverage a=1/>",
        "<coverage a='<'/>",
        "<coverage a='&lt;'/>",
        "<coverage a='&#60;'/>",
        "<coverage a='&#0;'/>",
        "<coverage a='&#x;'/>",
        "<coverage a='&#xFFFFFFFFF;'/>",
        "<coverage a='&#55296;'/>",
        "<coverage a='&amp'/>",
        "<coverage a='&unknown;'/>",
        "<coverage a='\u{1}'/>",
        "<coverage a='\u{fffe}'/>",
        "<coverage>&#1;</coverage>",
        "<coverage>&#x10FFFF;</coverage>",
        "<coverage>]]></coverage>",
        "<coverage>]]</coverage>",
        "<coverage><![CDATA[]]></coverage>",
        "<coverage><![CDATA[]]]></coverage>",
        "<coverage><![CDATA[x</coverage>",
        "<coverage><!-- a -- b --></coverage>",
        "<coverage><!-- a- --></coverage>",
        "<coverage><!---></coverage>",
        "<coverage><!----></coverage>",
        "<coverage><!-x--></coverage>",
        "<coverage><?xml version='1.0'?></coverage>",
        "<coverage><?xml?></coverage>",
        "<coverage><?pi</coverage>",
        "<coverage><?1pi?></coverage>",
        "<coverage><!ELEMENT a></coverage>",
        "<?xml version='1.0'?><?xml version='1.0'?><coverage/>",
        "<?xml version='1.0' encoding='utf-8' standalone='no'?><coverage/>",
        "<?xml encoding='utf-8'?><coverage/>",
        "<?xml versionx='1'?><coverage/>",
        "<?xml version='1'standalone='no'?><coverage/>",
        "<?xml version='1' ?  ><coverage/>",
        " <?xml version='1.0'?><coverage/>",
        "<!DOCTYPE coverage><coverage/>",
        "<!doctype coverage><coverage/>",
        "<!ENTITY x 'y'><coverage/>",
        "<?xml-stylesheet href='x'?><coverage/>",
        "<?XML-STYLESHEET href='x'?><coverage/>",
        "<!DOCTYPE a [<!ENTITY a '&b;'><!ENTITY b '&a;'>]><a>&a;</a>",
        "<coverage>\0</coverage>",
        "<coverage>\r</coverage>",
        "<coverage></coverage >",
        "<coverage></coverage\n>",
        "< coverage/>",
        "<coverage/ >",
        "<1coverage/>",
        "<co:ver:age/>",
        "<:coverage/>",
        "<coverage:/>",
        "<éa/>",
        "<a\u{b7}b/>",
        "<\u{b7}a/>",
        "<a\u{80}/>",
        "<coverage><source>x</source></coverage>",
        "<coverage><sources/><sources/><packages/></coverage>",
        "<coverage><packages/><packages/></coverage>",
        "<coverage><packages/></coverage>",
        "<coverage><packages><package/></packages></coverage>",
        "<coverage><packages><package><classes/><classes/></package></packages></coverage>",
        "<coverage><packages><package><classes/></package></packages></coverage>",
        "<coverage><packages><package><classes><class/></classes></package></packages></coverage>",
        "<coverage><packages><package><classes><class filename='src/A.cs'/></classes></package></packages></coverage>",
        "<coverage><packages><package><classes><class filename='src/A.cs'><lines/><lines/></class></classes></package></packages></coverage>",
        "<coverage><packages><package><classes><class filename='src/A.cs'><lines><line hits='1'/></lines></class></classes></package></packages></coverage>",
        "<coverage><packages><package><classes><class filename='src/A.cs'><lines><line number='1'/></lines></class></classes></package></packages></coverage>",
        "<coverage><packages><package><classes><class filename='src/A.cs'><lines><line number='9' hits='1'/></lines></class></classes></package></packages></coverage>",
        "<coverage><packages><package><classes><class filename='src/Missing.cs'><lines><line number='x' hits='1'/></lines></class></classes></package></packages></coverage>",
        "<coverage><packages><package><classes><class filename='src/A.cs'><lines><line number='1' hits='1'/></lines></class></classes></package><package/></packages></coverage>",
        "<coverage><packages><package><classes><class filename='src/A.cs'><lines><line number='1' hits='1'/></lines></class></classes></package></packages><sources/><sources/></coverage>",
        "<coverage><sources><source>src<x/></source></sources><packages/></coverage>",
        "<coverage><sources><source> </source></sources><packages/></coverage>",
        "<coverage><sources><source>a:b</source></sources><packages/></coverage>",
        "<coverage><sources><source>a&#13;&#10;b&#9;</source></sources><packages/></coverage>",
        "<coverage><sources><source>a\r\nb\rc</source></sources><packages/></coverage>",
        "<coverage><sources><source>a&#13;\nb</source></sources><packages/></coverage>",
        "<coverage><sources><source>a&#65;\r\nb</source></sources><packages/></coverage>",
        "<coverage><sources><source>a\r</source></sources><packages/></coverage>",
        "<coverage><sources><source><![CDATA[a\r\n\rb]]>c</source></sources><packages/></coverage>",
        "<coverage><packages><package><classes><class filename='src/A.cs&#9;'><lines/></class></classes></package></packages></coverage>",
        "<coverage><packages><package><classes><class filename='src/A.cs\r\n'><lines/></class></classes></package></packages></coverage>",
        "<coverage><packages><package><classes><class filename='src/A.cs\r'><lines/></class></classes></package></packages></coverage>",
        "<coverage><packages><package><classes><class filename='src/A.cs&#13;&#10;'><lines/></class></classes></package></packages></coverage>",
        &deep,
        &wide,
        &laughs,
        &many_ns,
        &same_ns,
    ] {
        env.assert_same(text);
    }
}

/// The node limit is reached at exactly the same document size.
#[test]
fn node_limit_is_reached_identically() {
    let texts = [
        "<coverage><a/>t<!--c-->t<?p?><![CDATA[x]]>t<b>u<![CDATA[]]></b></coverage>",
        "<!--a--><?b?><coverage>x<![CDATA[y]]>z</coverage><!--c-->",
        "<coverage><packages><package><classes><class filename='src/A.cs'><lines><line number='1' hits='1'/></lines></class></classes></package></packages></coverage>",
    ];
    for text in texts {
        for limit in 0..24 {
            let render = |error: crate::Error| format!("{:?}: {error:#}", error.kind());
            let old = dom_oracle::parse_document(text, limit)
                .map(|_| ())
                .map_err(render);
            let new = super::check_document(text, limit)
                .map(|_| ())
                .map_err(render);
            assert_eq!(new, old, "{text} at limit {limit}");
        }
    }
}

/// Recorded Coverlet reports in the repository import identically.
#[test]
fn recorded_fixture_reports_import_identically() {
    let env = Env::new();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut seen = 0;
    for entry in walkdir::WalkDir::new(fixtures) {
        let entry = entry.unwrap();
        if entry.path().extension().is_some_and(|ext| ext == "xml") {
            let text = fs::read_to_string(entry.path()).unwrap();
            let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
            env.assert_same(text);
            seen += 1;
        }
    }
    assert!(seen > 0, "no fixture reports found");
}

/// A longer run of the generated, mutated and token-soup cases over many seeds:
/// `DOTNET_CRAP_DIFFERENTIAL_CASES=200000 cargo test --release -- --ignored`.
#[test]
#[ignore = "long-running; the fixed-seed tests above run by default"]
fn extended_differential_run() {
    let cases: u64 = std::env::var("DOTNET_CRAP_DIFFERENTIAL_CASES")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(100_000);
    let env = Env::new();
    let root = env.root().display().to_string();
    let pieces: Vec<&str> = MUTATIONS.iter().chain(NOISE).copied().collect();
    for seed in 0..cases {
        let mut rng = Rng(seed.wrapping_mul(0x2545_F491_4F6C_DD1D), false);
        let report = generate(&mut rng, &root);
        env.assert_same(&report);
        env.assert_same(&mutate(&mut rng, &report));
        let mut text = String::from(rng.pick(&["<coverage>", "<a>", "<x:a xmlns:x='u'>", ""]));
        for _ in 0..rng.below(12) {
            text.push_str(rng.pick(&pieces));
        }
        env.assert_same(&text);
    }
}
