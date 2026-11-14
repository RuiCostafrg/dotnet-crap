//! Strict Cobertura line-evidence import.
//!
//! Only class-level `<lines>` records are evidence. Method-level copies and
//! summary percentages are deliberately ignored. Duplicate file/line records
//! across classes or reports merge by maximum hit count, so duplicate
//! instrumentation cannot inflate a score or overflow while merging.
//!
//! Paths must resolve exactly to a selected root-relative source or to a real
//! unselected file contained by the canonical scan root. Source prefixes from
//! `<sources>` are tried literally; basename/suffix guessing, SourceLink URLs
//! and deterministic `/_/` mappings are rejected rather than reinterpreted.

use crate::error::{fail, Error, ErrorKind, OptionExt, Result, ResultExt};
use roxmltree::{Document, Node, ParsingOptions};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs,
    io::Read,
    path::{Path, PathBuf},
};

/// Selected file lines mapped to their maximum observed hit count.
pub type LineEvidence = BTreeMap<String, BTreeMap<u32, u64>>;

/// Parse class-level Cobertura line evidence for selected sources.
///
/// `root` is the canonical source root used by a scan. `sources` is keyed by
/// exact slash-separated root-relative selected paths and contains the original
/// UTF-8 source text used to bound line numbers. Every report must contain
/// `coverage/packages/package/classes` and at least one class record overall.
/// Each class requires a `lines` container, whose line records may be empty.
/// Each package requires a `classes` container, which may be empty.
///
/// Reports may mention other real files inside `root`; their evidence is
/// ignored. Any path that does not resolve exactly, escapes the root, is a
/// foreign Windows root or any UNC root, or is an unmapped SourceLink/deterministic path is an
/// error. At least one report is required.
pub fn parse_reports(
    root: &Path,
    sources: &BTreeMap<String, String>,
    reports: &[PathBuf],
) -> Result<LineEvidence> {
    if reports.is_empty() {
        fail!(Cobertura, "no Cobertura reports to import");
    }
    let root = root
        .canonicalize()
        .context(ErrorKind::InvalidInput, "invalid coverage root")?;
    if !root.is_dir() {
        fail!(InvalidInput, "coverage root must be a directory");
    }
    let mut evidence = LineEvidence::new();
    // Line counts per selected source are file properties, not report
    // properties, so one lazy cache serves every report in this call.
    let mut line_counts = HashMap::new();
    for report in reports {
        // Keep the inner kind: an unreadable report is `Io`, rejected content is `Cobertura`.
        import_report(&root, sources, report, &mut evidence, &mut line_counts).map_err(
            |error| error.wrap(format!("invalid Cobertura report {}", report.display())),
        )?;
    }
    Ok(evidence)
}

/// Import one report, merging class-level lines into the shared evidence map.
///
/// `line_counts` caches the source line count per resolved selected source name
/// across reports. Resolution results are memoized per filename within this
/// report because `<sources>` prefixes are per report.
fn import_report(
    root: &Path,
    sources: &BTreeMap<String, String>,
    report: &Path,
    evidence: &mut LineEvidence,
    line_counts: &mut HashMap<String, usize>,
) -> Result<()> {
    const MAX_REPORT_BYTES: u64 = 16 * 1024 * 1024;
    let file = fs::File::open(report)
        .with_context(ErrorKind::Io, || format!("reading {}", report.display()))?;
    if file.metadata().kind(ErrorKind::Io)?.len() > MAX_REPORT_BYTES {
        fail!(Cobertura, "Cobertura report exceeds 16 MiB size limit");
    }
    let mut bytes = Vec::new();
    file.take(MAX_REPORT_BYTES + 1)
        .read_to_end(&mut bytes)
        .kind(ErrorKind::Io)?;
    if bytes.len() as u64 > MAX_REPORT_BYTES {
        fail!(Cobertura, "Cobertura report exceeds 16 MiB size limit");
    }
    let text = std::str::from_utf8(&bytes)
        .context(ErrorKind::Cobertura, "Cobertura report is not UTF-8")?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let document = parse_document(text)?;
    let coverage = document.root_element();
    if coverage.tag_name().name() != "coverage" {
        fail!(Cobertura, "root element must be <coverage>");
    }
    let prefixes = source_prefixes(coverage, root)?;
    let packages = only_child(coverage, "packages")?;
    let packages = child_elements(packages, "package");
    if packages.is_empty() {
        fail!(Cobertura, "<packages> contains no <package> elements");
    }
    let mut found_class = false;
    let mut resolutions: HashMap<&str, Option<String>> = HashMap::new();
    for package in packages {
        let classes = only_child(package, "classes")?;
        let classes = child_elements(classes, "class");
        for class in classes {
            found_class = true;
            import_class(
                class,
                root,
                sources,
                &prefixes,
                evidence,
                &mut resolutions,
                line_counts,
            )?;
        }
    }
    if !found_class {
        fail!(Cobertura, "Cobertura report contains no class records");
    }
    Ok(())
}

/// Parse XML with DTD processing explicitly disabled and reject unsafe declarations.
fn parse_document(text: &str) -> Result<Document<'_>> {
    let [doctype, entity, stylesheet] =
        contains_ascii_ci(text, ["<!doctype", "<!entity", "<?xml-stylesheet"]);
    if doctype {
        fail!(
            Cobertura,
            "Cobertura report contains a forbidden DOCTYPE declaration"
        );
    }
    if entity {
        fail!(
            Cobertura,
            "Cobertura report contains a forbidden entity declaration"
        );
    }
    if stylesheet {
        fail!(
            Cobertura,
            "Cobertura report contains a forbidden external XML stylesheet"
        );
    }
    parse_with_node_limit(text, 500_000)
}

/// Report which lowercase ASCII `needles` occur in `text`, ignoring ASCII case,
/// in one pass and without copying the report. Every needle starts with `<`.
fn contains_ascii_ci<const N: usize>(text: &str, needles: [&str; N]) -> [bool; N] {
    let bytes = text.as_bytes();
    let mut found = [false; N];
    for (start, _) in bytes.iter().enumerate().filter(|(_, b)| **b == b'<') {
        let rest = &bytes[start..];
        for (flag, needle) in found.iter_mut().zip(needles) {
            *flag |= rest
                .get(..needle.len())
                .is_some_and(|window| window.eq_ignore_ascii_case(needle.as_bytes()));
        }
    }
    found
}

/// Bound DOM allocation independently of the byte limit.
fn parse_with_node_limit(text: &str, nodes_limit: u32) -> Result<Document<'_>> {
    // Keep every field explicit: `..Default::default()` would drop the node
    // limit, and no entity resolver preserves the no-external-entity contract.
    let options = ParsingOptions {
        nodes_limit,
        allow_dtd: false,
        entity_resolver: None,
    };
    Document::parse_with_options(text, options)
        .map_err(|error| Error::new(ErrorKind::Cobertura, format!("malformed XML: {error}")))
}

/// Read literal `<sources><source>` prefixes, anchoring relative prefixes at root.
fn source_prefixes(coverage: Node<'_, '_>, root: &Path) -> Result<Vec<PathBuf>> {
    let sources = child_elements(coverage, "sources");
    if sources.len() > 1 {
        fail!(Cobertura, "duplicate <sources> elements");
    }
    let Some(sources) = sources.into_iter().next() else {
        return Ok(Vec::new());
    };
    let mut prefixes = Vec::new();
    for source in child_elements(sources, "source") {
        if source.children().any(|child| child.is_element()) {
            fail!(
                Cobertura,
                "Cobertura source prefix must not contain elements"
            );
        }
        let text = source
            .children()
            .filter(|child| child.is_text())
            .filter_map(|child| child.text())
            .collect::<String>();
        let raw = text.as_str();
        if raw.trim().is_empty() {
            fail!(Cobertura, "empty Cobertura source prefix");
        }
        let text =
            normalize_path(raw).context(ErrorKind::Cobertura, "invalid Cobertura source prefix")?;
        let path = PathBuf::from(&text);
        prefixes.push(if is_host_absolute(&text) {
            path
        } else {
            root.join(path)
        });
    }
    Ok(prefixes)
}

/// Resolve one class filename and import its class-level line records.
///
/// `resolutions` memoizes successful filename resolutions for this report;
/// failures return errors directly and are not cached. `line_counts` caches
/// the line count per resolved selected source so a dense file with many
/// classes is measured once instead of once per class.
fn import_class<'a>(
    class: Node<'a, 'a>,
    root: &Path,
    sources: &BTreeMap<String, String>,
    prefixes: &[PathBuf],
    evidence: &mut LineEvidence,
    resolutions: &mut HashMap<&'a str, Option<String>>,
    line_counts: &mut HashMap<String, usize>,
) -> Result<()> {
    let filename = class.attribute("filename").context(
        ErrorKind::Cobertura,
        "Cobertura <class> is missing filename",
    )?;
    let lines = only_child(class, "lines")?;
    let lines = child_elements(lines, "line");

    let resolved = match resolutions.get(filename) {
        Some(cached) => cached.clone(),
        None => {
            let resolved = resolve_filename(root, prefixes, filename, sources)?;
            resolutions.insert(filename, resolved.clone());
            resolved
        }
    };
    let source_line_count = match &resolved {
        Some(name) => Some(match line_counts.get(name) {
            Some(count) => *count,
            None => {
                let count = sources
                    .get(name)
                    .context(
                        ErrorKind::Cobertura,
                        "resolved selected source is missing from the source map",
                    )?
                    .lines()
                    .count();
                line_counts.insert(name.clone(), count);
                count
            }
        }),
        None => None,
    };
    let mut parsed = Vec::with_capacity(lines.len());
    for line in lines {
        let number = parse_line_number(line)?;
        let hits = parse_hits(line)?;
        if let Some(limit) = source_line_count {
            if number as usize > limit {
                fail!(
                    Cobertura,
                    "Cobertura line {number} is outside source {filename:?}"
                );
            }
        }
        parsed.push((number, hits));
    }
    let Some(name) = resolved else {
        return Ok(());
    };
    if parsed.is_empty() {
        return Ok(());
    }
    let entry = evidence.entry(name).or_default();
    for (line, hits) in parsed {
        entry
            .entry(line)
            .and_modify(|current| *current = (*current).max(hits))
            .or_insert(hits);
    }
    Ok(())
}

/// Parse a canonical positive line number.
fn parse_line_number(line: Node<'_, '_>) -> Result<u32> {
    let raw = line
        .attribute("number")
        .context(ErrorKind::Cobertura, "Cobertura <line> is missing number")?;
    let number: u32 = raw.parse().with_context(ErrorKind::Cobertura, || {
        format!("invalid Cobertura line number {raw:?}")
    })?;
    if number == 0 {
        fail!(Cobertura, "Cobertura line number must be positive: {raw:?}");
    }
    if number.to_string() != raw {
        fail!(Cobertura, "Cobertura line number is not canonical: {raw:?}");
    }
    Ok(number)
}

/// Parse a canonical unsigned hit count, including zero.
fn parse_hits(line: Node<'_, '_>) -> Result<u64> {
    let raw = line
        .attribute("hits")
        .context(ErrorKind::Cobertura, "Cobertura <line> is missing hits")?;
    let hits: u64 = raw.parse().with_context(ErrorKind::Cobertura, || {
        format!("invalid Cobertura hit count {raw:?}")
    })?;
    if hits.to_string() != raw {
        fail!(Cobertura, "Cobertura hit count is not canonical: {raw:?}");
    }
    Ok(hits)
}

/// Resolve a report filename against literal source prefixes, then the root.
fn resolve_filename(
    root: &Path,
    prefixes: &[PathBuf],
    filename: &str,
    sources: &BTreeMap<String, String>,
) -> Result<Option<String>> {
    let path =
        normalize_path(filename).context(ErrorKind::Cobertura, "invalid Cobertura filename")?;
    let absolute = is_host_absolute(&path);
    validate_filename(&path, absolute)?;
    let mut candidates = BTreeSet::new();
    if absolute {
        add_candidate(root, Path::new(""), &path, sources, &mut candidates)?;
    } else {
        // An already-tried base joins to the same candidate path, so resolving
        // it again can neither add a candidate nor raise a new error. Coverlet
        // usually lists the project root itself as the only prefix.
        for prefix in first_occurrences(prefixes) {
            add_candidate(root, prefix, &path, sources, &mut candidates)?;
        }
        // Use the root as a fallback when there are no prefixes, when no prefix
        // produced an exact candidate, or when the filename is itself an exact
        // selected root-relative identity. A basename is never guessed.
        let prefix_candidate = !candidates.is_empty();
        let root_tried = prefixes.iter().any(|prefix| prefix == root);
        if !root_tried && (prefixes.is_empty() || !prefix_candidate || sources.contains_key(&path))
        {
            add_candidate(root, root, &path, sources, &mut candidates)?;
        }
    }
    if candidates.len() > 1 {
        fail!(
            Cobertura,
            "ambiguous Cobertura filename {filename:?} resolves to multiple files"
        );
    }
    let Some(candidate) = candidates.into_iter().next() else {
        fail!(
            Cobertura,
            "Cobertura filename {filename:?} does not resolve to a root-contained file"
        );
    };
    let relative = candidate
        .strip_prefix(root)
        .map_err(|_| {
            Error::new(
                ErrorKind::Cobertura,
                format!("Cobertura path is outside the coverage root: {filename:?}"),
            )
        })?
        .to_str()
        .context(ErrorKind::Cobertura, "non-UTF-8 Cobertura source path")?
        .replace('\\', "/");
    if relative.is_empty() {
        fail!(
            Cobertura,
            "Cobertura filename does not identify a file: {filename:?}"
        );
    }
    Ok(sources.contains_key(&relative).then_some(relative))
}

/// Each distinct prefix once, in its original order.
fn first_occurrences(prefixes: &[PathBuf]) -> impl Iterator<Item = &PathBuf> {
    prefixes
        .iter()
        .enumerate()
        .filter(|(index, prefix)| !prefixes[..*index].contains(prefix))
        .map(|(_, prefix)| prefix)
}

/// Add one exact candidate. Existing files are canonicalized and contained;
/// an absent lexical path is accepted only when it is an exact selected source.
fn add_candidate(
    root: &Path,
    base: &Path,
    filename: &str,
    sources: &BTreeMap<String, String>,
    candidates: &mut BTreeSet<PathBuf>,
) -> Result<()> {
    let candidate = base.join(filename);
    match candidate.canonicalize() {
        Ok(canonical) => {
            if canonical == root || !canonical.starts_with(root) {
                fail!(
                    Cobertura,
                    "Cobertura path is outside the coverage root: {}",
                    candidate.display()
                );
            }
            if !canonical.is_file() {
                fail!(
                    Cobertura,
                    "Cobertura path is not a file: {}",
                    candidate.display()
                );
            }
            candidates.insert(canonical);
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            // The parser receives source text rather than reading selected files.
            // A missing lexical path is still exact evidence when its
            // root-relative identity is an explicitly selected source.
            if let Ok(relative) = candidate.strip_prefix(root) {
                let relative = relative.to_string_lossy().replace('\\', "/");
                if sources.contains_key(&relative) {
                    candidates.insert(candidate);
                }
            }
        }
        Err(error) => {
            return Err(error).with_context(ErrorKind::Io, || {
                format!("resolving Cobertura path {}", candidate.display())
            });
        }
    }
    Ok(())
}

/// Normalize separators and reject URI, Windows, deterministic and parent paths.
fn normalize_path(raw: &str) -> Result<String> {
    let path = raw.replace('\\', "/");
    if path.is_empty() {
        fail!(Cobertura, "empty path");
    }
    if path.contains('\0') {
        fail!(Cobertura, "path contains NUL");
    }
    if let Some(colon) = path.find(':') {
        let scheme = &path[..colon];
        let scheme_like = !scheme.is_empty()
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
        let drive_absolute = scheme.len() == 1
            && scheme.as_bytes()[0].is_ascii_alphabetic()
            && path.as_bytes().get(colon + 1) == Some(&b'/');
        if scheme_like && drive_absolute {
            #[cfg(not(windows))]
            fail!(Cobertura, "foreign Windows root is not supported: {raw}");
        } else {
            fail!(Cobertura, "URI or colon path is not supported: {raw}");
        }
    }
    if path.starts_with("//") {
        fail!(Cobertura, "Windows/UNC roots are not supported: {raw}");
    }
    if path.starts_with("/_/") || path == "/_" {
        fail!(
            Cobertura,
            "unmapped deterministic/SourceLink path is not supported: {raw}"
        );
    }
    if path.split('/').any(|part| part == "..") {
        fail!(Cobertura, "parent path component is not supported: {raw}");
    }
    Ok(path)
}

/// Enforce exact file-path components without allowing lexical ambiguity.
fn validate_filename(path: &str, absolute: bool) -> Result<()> {
    if path.ends_with('/') {
        fail!(Cobertura, "Cobertura filename must name a file: {path:?}");
    }
    for (index, part) in path.split('/').enumerate() {
        if part.is_empty() {
            if absolute && index == 0 {
                continue;
            }
            fail!(
                Cobertura,
                "Cobertura filename has an empty component: {path:?}"
            );
        }
        if part == "." || part == ".." {
            fail!(
                Cobertura,
                "Cobertura filename is not an exact path: {path:?}"
            );
        }
    }
    Ok(())
}

/// Whether a normalized path is absolute for the host platform.
fn is_host_absolute(path: &str) -> bool {
    #[cfg(windows)]
    {
        Path::new(path).is_absolute()
    }
    #[cfg(not(windows))]
    {
        path.starts_with('/')
    }
}

/// Collect direct element children with the requested local name.
fn child_elements<'a, 'input: 'a>(node: Node<'a, 'input>, name: &str) -> Vec<Node<'a, 'input>> {
    node.children()
        .filter(|child| child.is_element() && child.tag_name().name() == name)
        .collect()
}

/// Require exactly one direct element child with the requested local name.
fn only_child<'a, 'input: 'a>(node: Node<'a, 'input>, name: &str) -> Result<Node<'a, 'input>> {
    let mut children = child_elements(node, name).into_iter();
    let child = children
        .next()
        .ok_or_else(|| Error::new(ErrorKind::Cobertura, format!("missing <{name}> element")))?;
    if children.next().is_some() {
        fail!(Cobertura, "duplicate <{name}> elements");
    }
    Ok(child)
}

#[cfg(test)]
mod resource_tests {
    const NEEDLES: [&str; 3] = ["<!doctype", "<!entity", "<?xml-stylesheet"];

    /// The copy-free scan must agree with the lower-cased copy it replaces.
    fn assert_scan_matches_lowercase(text: &str, expected: [bool; 3]) {
        let lower = text.to_ascii_lowercase();
        let oracle = NEEDLES.map(|needle| lower.contains(needle));
        assert_eq!(oracle, expected, "oracle for {text:?}");
        assert_eq!(
            super::contains_ascii_ci(text, NEEDLES),
            oracle,
            "scan for {text:?}"
        );
    }

    #[test]
    fn declaration_scan_matches_ascii_lowercase_contains_on_edge_cases() {
        let padded_end = format!("{}<!entity", "x".repeat(8191));
        let padded_start = format!("<?xml-stylesheet{}", "x".repeat(8191));
        let padded_middle = format!("{}<!DOCTYPE{}", "<".repeat(4093), ">".repeat(4096));
        for (text, expected) in [
            ("", [false, false, false]),
            ("<", [false, false, false]),
            ("<coverage/>", [false, false, false]),
            // Needle at the very start, at the very end, and the whole input.
            ("<!doctype", [true, false, false]),
            ("<!doctype x>", [true, false, false]),
            ("a<!entity", [false, true, false]),
            ("<?xml-stylesheet", [false, false, true]),
            // One byte short of a needle at the end of input.
            ("<!doctyp", [false, false, false]),
            ("x<!entit", [false, false, false]),
            // Mixed case, including non-needle letters in odd positions.
            ("<!DocType x>", [true, false, false]),
            ("<!EnTiTy foo>", [false, true, false]),
            ("<?XML-Stylesheet?>", [false, false, true]),
            // Overlapping partial matches before the real one.
            ("<!doc<!doctype", [true, false, false]),
            ("<<!entity", [false, true, false]),
            ("<!<!entity", [false, true, false]),
            ("<!doctype <!doctyp<!entity", [true, true, false]),
            ("<<<?xml-style<?xml-stylesheet", [false, false, true]),
            // Near misses: wrong or missing `<`/`!`/`?`, or a split needle.
            ("!doctype <doctype", [false, false, false]),
            ("<! doctype", [false, false, false]),
            ("<!doc type", [false, false, false]),
            ("<?xml_stylesheet", [false, false, false]),
            // The same needle twice, and all three together.
            ("<!entity><!entity>", [false, true, false]),
            ("<!ENTITY><!doctype><?xml-stylesheet", [true, true, true]),
            // Non-ASCII bytes around, and inside look-alikes of, a needle.
            ("é<!doctype>é", [true, false, false]),
            ("日本<!ENTITY>語", [false, true, false]),
            ("<!doctypé", [false, false, false]),
            ("<!ſtylesheet <?xml-ſtylesheet", [false, false, false]),
            // Needles that straddle where a chunked scan could split the input.
            (padded_end.as_str(), [false, true, false]),
            (padded_start.as_str(), [false, false, true]),
            (padded_middle.as_str(), [true, false, false]),
        ] {
            assert_scan_matches_lowercase(text, expected);
        }
    }

    #[test]
    fn repeated_prefixes_are_tried_once_in_original_order() {
        let paths = |names: &[&str]| {
            names
                .iter()
                .map(std::path::PathBuf::from)
                .collect::<Vec<_>>()
        };
        let prefixes = paths(&["/r", "/a", "/r", "/b", "/a", "/r"]);
        let distinct: Vec<_> = super::first_occurrences(&prefixes).collect();
        assert_eq!(
            distinct,
            paths(&["/r", "/a", "/b"]).iter().collect::<Vec<_>>()
        );
        assert_eq!(super::first_occurrences(&[]).count(), 0);
    }

    #[test]
    fn declarations_are_rejected_in_the_documented_order() {
        let message = |text: &str| super::parse_document(text).unwrap_err().to_string();
        let doctype = "Cobertura report contains a forbidden DOCTYPE declaration";
        let entity = "Cobertura report contains a forbidden entity declaration";
        let stylesheet = "Cobertura report contains a forbidden external XML stylesheet";
        assert_eq!(message("<?xml-stylesheet?><!ENTITY><!DOCTYPE>"), doctype);
        assert_eq!(message("<?xml-stylesheet?><!ENTITY>"), entity);
        assert_eq!(message("<?XML-STYLESHEET?><a/>"), stylesheet);
        assert!(super::parse_document("<a/>").is_ok());
    }

    #[test]
    fn node_limit_rejects_excess_nodes_without_requiring_a_large_allocation() {
        let text = "<coverage><a/></coverage>";
        assert!(super::parse_with_node_limit(text, 2).is_err());
        assert!(super::parse_with_node_limit(text, 3).is_ok());
    }
}

#[cfg(test)]
mod dense_class_regression_tests {
    //! Many `<class>` records name the same dense source file, so per-class
    //! resolution and per-class line counting must be cached without changing
    //! merge, range or resolution semantics for later classes.

    use super::{parse_reports, LineEvidence};
    use std::{
        collections::BTreeMap,
        fs,
        path::{Path, PathBuf},
    };

    /// Minimal report fixture with selected source text and real files on disk.
    struct Fixture {
        root: tempfile::TempDir,
        sources: BTreeMap<String, String>,
        reports: Vec<PathBuf>,
    }

    impl Fixture {
        fn new() -> Self {
            Self {
                root: tempfile::tempdir().unwrap(),
                sources: BTreeMap::new(),
                reports: Vec::new(),
            }
        }

        fn root(&self) -> &Path {
            self.root.path()
        }

        fn source(&mut self, relative: &str, text: &str) {
            let path = self.root().join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, text).unwrap();
            self.sources.insert(relative.to_owned(), text.to_owned());
        }

        fn report(&mut self, prefixes: &[String], classes: &[String]) {
            let sources = if prefixes.is_empty() {
                String::new()
            } else {
                format!(
                    "<sources>{}</sources>",
                    prefixes
                        .iter()
                        .map(|prefix| format!("<source>{prefix}</source>"))
                        .collect::<String>()
                )
            };
            let xml = format!(
                "<coverage version=\"1.9\">{sources}<packages><package name=\"P\"><classes>{}</classes></package></packages></coverage>",
                classes.concat()
            );
            let path = self
                .root()
                .join(format!("report-{}.xml", self.reports.len()));
            fs::write(&path, xml).unwrap();
            self.reports.push(path);
        }

        fn parse(&self) -> crate::Result<LineEvidence> {
            parse_reports(self.root(), &self.sources, &self.reports)
        }
    }

    /// One class record with class-level lines only.
    fn class(filename: &str, lines: &[(u32, u64)]) -> String {
        let line_xml = lines
            .iter()
            .map(|(number, hits)| format!("<line number=\"{number}\" hits=\"{hits}\"/>"))
            .collect::<String>();
        format!("<class name=\"C\" filename=\"{filename}\"><lines>{line_xml}</lines></class>")
    }

    #[test]
    /// Conflicting hits across many classes for one dense file still merge by maximum.
    fn many_classes_for_one_file_merge_by_maximum_hits() {
        let mut fixture = Fixture::new();
        let text: String = (1..=40).map(|index| format!("line {index}\n")).collect();
        fixture.source("src/Dense.cs", &text);
        let mut classes = Vec::new();
        let mut expected = BTreeMap::new();
        for index in 0..60u32 {
            let base = (index % 20) + 1;
            classes.push(class(
                "src/Dense.cs",
                &[
                    (base, 1 + (index % 7) as u64),
                    (base + 1, (index % 5) as u64),
                ],
            ));
            let current = expected.entry(base).or_insert(0);
            *current = (*current).max(1 + (index % 7) as u64);
            let current = expected.entry(base + 1).or_insert(0);
            *current = (*current).max((index % 5) as u64);
        }
        fixture.report(&[], &classes);

        let parsed = fixture.parse().unwrap();
        assert_eq!(parsed.get("src/Dense.cs").unwrap(), &expected);
    }

    #[test]
    /// A cached line count for an earlier class must still bound later classes.
    fn out_of_range_line_in_later_class_is_rejected_with_the_same_message() {
        let mut fixture = Fixture::new();
        fixture.source("src/Dense.cs", "one\ntwo\nthree\n");
        fixture.report(
            &[],
            &[
                class("src/Dense.cs", &[(1, 1), (2, 2)]),
                class("src/Dense.cs", &[(4, 1)]),
            ],
        );

        let error = format!("{:#}", fixture.parse().unwrap_err());
        assert!(
            error.contains(r#"Cobertura line 4 is outside source "src/Dense.cs""#),
            "{error}"
        );
    }

    #[test]
    /// Line counts are cached per resolved source, never across different files.
    fn cached_line_count_of_one_source_does_not_bound_another() {
        let mut fixture = Fixture::new();
        fixture.source("src/Short.cs", "one\n");
        fixture.source("src/Long.cs", "one\ntwo\nthree\nfour\nfive\n");
        fixture.report(
            &[],
            &[
                class("src/Short.cs", &[(1, 1)]),
                class("src/Long.cs", &[(5, 2)]),
            ],
        );
        let parsed = fixture.parse().unwrap();
        assert_eq!(
            parsed.get("src/Long.cs").unwrap(),
            &BTreeMap::from([(5, 2)])
        );

        let mut fixture = Fixture::new();
        fixture.source("src/Short.cs", "one\n");
        fixture.source("src/Long.cs", "one\ntwo\nthree\nfour\nfive\n");
        fixture.report(
            &[],
            &[
                class("src/Short.cs", &[(1, 1)]),
                class("src/Short.cs", &[(2, 1)]),
            ],
        );
        let error = format!("{:#}", fixture.parse().unwrap_err());
        assert!(
            error.contains(r#"Cobertura line 2 is outside source "src/Short.cs""#),
            "{error}"
        );
    }

    #[test]
    /// A resolution failure on a later class with a different filename still surfaces.
    fn resolution_error_on_later_class_with_different_filename_still_surfaces() {
        let mut fixture = Fixture::new();
        fixture.source("src/A.cs", "one\ntwo\n");
        fixture.report(
            &[],
            &[
                class("src/A.cs", &[(1, 1)]),
                class("src/Missing.cs", &[(1, 1)]),
            ],
        );

        let error = format!("{:#}", fixture.parse().unwrap_err());
        assert!(
            error.contains(
                r#"Cobertura filename "src/Missing.cs" does not resolve to a root-contained file"#
            ),
            "{error}"
        );
    }

    #[test]
    /// The root, spelled once or repeated, resolves a class exactly as no prefix does,
    /// and a miss under it keeps the same message.
    fn root_and_repeated_prefixes_resolve_like_a_single_prefix() {
        let mut fixture = Fixture::new();
        fixture.source("src/A.cs", "one\ntwo\n");
        let root = fixture.root().display().to_string();
        let found = [class("src/A.cs", &[(1, 1)])];
        fixture.report(std::slice::from_ref(&root), &found);
        fixture.report(&[root.clone(), root.clone(), format!("{root}/")], &found);
        fixture.report(
            &[format!("{root}/src"), format!("{root}/src")],
            &[class("A.cs", &[(2, 4)])],
        );
        let parsed = fixture.parse().unwrap();
        assert_eq!(
            parsed.get("src/A.cs").unwrap(),
            &BTreeMap::from([(1, 1), (2, 4)])
        );

        let mut fixture = Fixture::new();
        fixture.source("src/A.cs", "one\ntwo\n");
        let root = fixture.root().display().to_string();
        fixture.report(&[root.clone(), root], &[class("src/Missing.cs", &[(1, 1)])]);
        let error = format!("{:#}", fixture.parse().unwrap_err());
        assert!(
            error.contains(
                r#"Cobertura filename "src/Missing.cs" does not resolve to a root-contained file"#
            ),
            "{error}"
        );
    }

    #[test]
    /// Resolution results are memoized per report because prefixes are per report.
    fn resolution_memoization_does_not_leak_across_reports() {
        let mut fixture = Fixture::new();
        fixture.source("Alpha/A.cs", "one\ntwo\n");
        fixture.source("Beta/A.cs", "one\ntwo\n");
        let root = fixture.root().display().to_string();
        fixture.report(&[format!("{root}/Alpha/")], &[class("A.cs", &[(1, 1)])]);
        fixture.report(&[format!("{root}/Beta/")], &[class("A.cs", &[(2, 3)])]);

        let parsed = fixture.parse().unwrap();
        assert_eq!(parsed.get("Alpha/A.cs").unwrap(), &BTreeMap::from([(1, 1)]));
        assert_eq!(parsed.get("Beta/A.cs").unwrap(), &BTreeMap::from([(2, 3)]));
    }
}
