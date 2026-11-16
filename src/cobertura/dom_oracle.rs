//! The DOM importer this module replaced, kept as the differential oracle.
//!
//! It is the previous `import_report` body after UTF-8 decoding, unchanged
//! except that line attributes go through the shared canonical-number parsers.

use super::{
    contains_ascii_ci, is_host_absolute, normalize_path, parse_hits, parse_line_number,
    resolve_filename, LineEvidence,
};
use crate::error::{fail, Error, ErrorKind, OptionExt, Result, ResultExt};
use roxmltree::{Document, Node, ParsingOptions};
use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
};

pub(super) fn import_xml(
    root: &Path,
    sources: &BTreeMap<String, usize>,
    text: &str,
    evidence: &mut LineEvidence,
) -> Result<()> {
    let document = parse_document(text, 500_000)?;
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
            import_class(class, root, sources, &prefixes, evidence, &mut resolutions)?;
        }
    }
    if !found_class {
        fail!(Cobertura, "Cobertura report contains no class records");
    }
    Ok(())
}

pub(super) fn parse_document(text: &str, nodes_limit: u32) -> Result<Document<'_>> {
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
    let options = ParsingOptions {
        nodes_limit,
        allow_dtd: false,
        entity_resolver: None,
    };
    Document::parse_with_options(text, options)
        .map_err(|error| Error::new(ErrorKind::Cobertura, format!("malformed XML: {error}")))
}

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

fn import_class<'a>(
    class: Node<'a, 'a>,
    root: &Path,
    sources: &BTreeMap<String, usize>,
    prefixes: &[PathBuf],
    evidence: &mut LineEvidence,
    resolutions: &mut HashMap<&'a str, Option<String>>,
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
        Some(name) => Some(*sources.get(name).context(
            ErrorKind::Cobertura,
            "resolved selected source is missing from the source map",
        )?),
        None => None,
    };
    let mut parsed = Vec::with_capacity(lines.len());
    for line in lines {
        let number = parse_line_number(line.attribute("number"))?;
        let hits = parse_hits(line.attribute("hits"))?;
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

fn child_elements<'a, 'input: 'a>(node: Node<'a, 'input>, name: &str) -> Vec<Node<'a, 'input>> {
    node.children()
        .filter(|child| child.is_element() && child.tag_name().name() == name)
        .collect()
}

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
