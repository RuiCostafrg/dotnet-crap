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
    collections::{BTreeMap, BTreeSet},
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
    for report in reports {
        import_report(&root, sources, report, &mut evidence)
            .with_context(ErrorKind::Cobertura, || {
                format!("invalid Cobertura report {}", report.display())
            })?;
    }
    Ok(evidence)
}

/// Import one report, merging class-level lines into the shared evidence map.
fn import_report(
    root: &Path,
    sources: &BTreeMap<String, String>,
    report: &Path,
    evidence: &mut LineEvidence,
) -> Result<()> {
    const MAX_REPORT_BYTES: u64 = 16 * 1024 * 1024;
    let file = fs::File::open(report).with_context(ErrorKind::Cobertura, || {
        format!("reading {}", report.display())
    })?;
    if file.metadata().kind(ErrorKind::Cobertura)?.len() > MAX_REPORT_BYTES {
        fail!(Cobertura, "Cobertura report exceeds 16 MiB size limit");
    }
    let mut bytes = Vec::new();
    file.take(MAX_REPORT_BYTES + 1)
        .read_to_end(&mut bytes)
        .kind(ErrorKind::Cobertura)?;
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
    for package in packages {
        let classes = only_child(package, "classes")?;
        let classes = child_elements(classes, "class");
        for class in classes {
            found_class = true;
            import_class(class, root, sources, &prefixes, evidence)?;
        }
    }
    if !found_class {
        fail!(Cobertura, "Cobertura report contains no class records");
    }
    Ok(())
}

/// Parse XML with DTD processing explicitly disabled and reject unsafe declarations.
fn parse_document(text: &str) -> Result<Document<'_>> {
    let lower = text.to_ascii_lowercase();
    if lower.contains("<!doctype") {
        fail!(
            Cobertura,
            "Cobertura report contains a forbidden DOCTYPE declaration"
        );
    }
    if lower.contains("<!entity") {
        fail!(
            Cobertura,
            "Cobertura report contains a forbidden entity declaration"
        );
    }
    if lower.contains("<?xml-stylesheet") {
        fail!(
            Cobertura,
            "Cobertura report contains a forbidden external XML stylesheet"
        );
    }
    parse_with_node_limit(text, 500_000)
}

/// Bound DOM allocation independently of the byte limit.
fn parse_with_node_limit(text: &str, nodes_limit: u32) -> Result<Document<'_>> {
    let options = ParsingOptions {
        nodes_limit,
        allow_dtd: false,
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
fn import_class(
    class: Node<'_, '_>,
    root: &Path,
    sources: &BTreeMap<String, String>,
    prefixes: &[PathBuf],
    evidence: &mut LineEvidence,
) -> Result<()> {
    let filename = class.attribute("filename").context(
        ErrorKind::Cobertura,
        "Cobertura <class> is missing filename",
    )?;
    let lines = only_child(class, "lines")?;
    let lines = child_elements(lines, "line");

    let resolved = resolve_filename(root, prefixes, filename, sources)?;
    let source_line_count = match &resolved {
        Some(name) => Some(
            sources
                .get(name)
                .context(
                    ErrorKind::Cobertura,
                    "resolved selected source is missing from the source map",
                )?
                .lines()
                .count(),
        ),
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
        for prefix in prefixes {
            add_candidate(root, prefix, &path, sources, &mut candidates)?;
        }
        // Use the root as a fallback when there are no prefixes, when no prefix
        // produced an exact candidate, or when the filename is itself an exact
        // selected root-relative identity. A basename is never guessed.
        let prefix_candidate = !candidates.is_empty();
        if prefixes.is_empty() || !prefix_candidate || sources.contains_key(&path) {
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
            return Err(error).with_context(ErrorKind::Cobertura, || {
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
    #[test]
    fn node_limit_rejects_excess_nodes_without_requiring_a_large_allocation() {
        let text = "<coverage><a/></coverage>";
        assert!(super::parse_with_node_limit(text, 2).is_err());
        assert!(super::parse_with_node_limit(text, 3).is_ok());
    }
}
