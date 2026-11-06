//! Scan C# sources and render source-bound CRAP reports.
//!
//! Scanning returns a typed report even when a quality policy fails. Invalid
//! input returns an error. Rendering builds a string; callers own output writes
//! and process exit policy. See `examples/scan.rs` for a runnable consumer.
//!
//! ```no_run
//! use dotnet_crap::{scan, ScanOptions, ReportFormat};
//!
//! let result = scan(&ScanOptions { root: "src".into(), ..Default::default() })?;
//! let json = result.render(ReportFormat::Json)?;
//! result.verify_sources()?;
//! println!("{json}");
//! # Ok::<(), anyhow::Error>(())
//! ```
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path, PathBuf},
};
use tree_sitter::{Node, Parser as TsParser};
use walkdir::WalkDir;

/// Inputs and policy for a scan. Relative source paths are resolved against `root`.
#[derive(Debug, Clone)]
pub struct ScanOptions {
    /// Root directory containing selected sources.
    pub root: PathBuf,
    /// Scores strictly greater than this value fail; equality passes.
    pub threshold: f64,
    /// Optional native coverage JSON path, resolved against the working directory.
    pub coverage: Option<PathBuf>,
    /// Fail the policy when any callable lacks coverage evidence.
    pub require_coverage: bool,
    /// Fail the policy when no callables are found.
    pub fail_on_empty: bool,
    /// Files or directories to select; an empty list selects the root.
    pub paths: Vec<PathBuf>,
}
impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            root: PathBuf::from("."),
            threshold: 10.0,
            coverage: None,
            require_coverage: false,
            fail_on_empty: false,
            paths: Vec::new(),
        }
    }
}
/// Supported report output formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportFormat {
    /// Human-readable terminal output.
    Text,
    /// Structured JSON report.
    Json,
    /// SARIF 2.1.0 report.
    Sarif,
}
/// A report and the source identities captured during its scan.
pub struct ScanResult {
    report: Report,
    root: PathBuf,
    input_paths: Vec<PathBuf>,
    source_hashes: Vec<(PathBuf, String)>,
    require_coverage: bool,
    fail_on_empty: bool,
}
impl ScanResult {
    /// Return the typed report.
    pub fn report(&self) -> &Report {
        &self.report
    }
    /// Return the canonical scan root.
    pub fn root(&self) -> &Path {
        &self.root
    }
    /// Return selected source paths and the coverage input, if present.
    pub fn input_paths(&self) -> &[PathBuf] {
        &self.input_paths
    }
    /// Verify that selected source bytes still match the scanned bytes.
    pub fn verify_sources(&self) -> Result<()> {
        for (path, hash) in &self.source_hashes {
            if hex::encode(Sha256::digest(fs::read(path)?)) != *hash {
                bail!("source changed during scan: {}", path.display());
            }
        }
        Ok(())
    }
    /// Render the report using the policies captured by the scan.
    pub fn render(&self, format: ReportFormat) -> Result<String> {
        let report = &self.report;
        let output = if format == ReportFormat::Sarif {
            format!(
                "{}\n",
                sarif::render(
                    report,
                    &self.root,
                    self.require_coverage,
                    self.fail_on_empty
                )?
            )
        } else if format == ReportFormat::Json {
            format!("{}\n", serde_json::to_string_pretty(report)?)
        } else {
            let mut s = String::new();
            if report.functions.is_empty() {
                s.push_str("No implemented C# callables found.\n");
            }
            if self.require_coverage && !report.coverage_complete {
                s.push_str("Coverage missing for one or more callables.\n");
            }
            for i in &report.functions {
                s.push_str(&format!(
                    "{}:{} {} complexity={} coverage={} CRAP={:.2}\n",
                    terminal_safe(&i.file),
                    i.line,
                    terminal_safe(&i.name),
                    i.complexity,
                    i.coverage
                        .map_or_else(|| "absent".into(), |v| format!("{:.1}%", v * 100.0)),
                    i.crap
                ));
            }
            s
        };
        Ok(output)
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Coverage {
    version: u32,
    #[serde(deserialize_with = "unique_map")]
    files: BTreeMap<String, CoverageFile>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CoverageFile {
    sha256: String,
    #[serde(deserialize_with = "unique_map")]
    lines: BTreeMap<String, u64>,
}
/// Deserialize coverage maps while rejecting duplicate keys instead of overwriting evidence.
fn unique_map<'de, D, V>(deserializer: D) -> std::result::Result<BTreeMap<String, V>, D::Error>
where
    D: serde::Deserializer<'de>,
    V: Deserialize<'de>,
{
    struct Unique<V>(std::marker::PhantomData<V>);
    impl<'de, V: Deserialize<'de>> serde::de::Visitor<'de> for Unique<V> {
        type Value = BTreeMap<String, V>;
        /// Describe the expected coverage map when deserialization receives another JSON type.
        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a map with unique keys")
        }
        /// Retain each key once and fail immediately when a duplicate would replace evidence.
        fn visit_map<M: serde::de::MapAccess<'de>>(
            self,
            mut map: M,
        ) -> std::result::Result<Self::Value, M::Error> {
            let mut entries = BTreeMap::new();
            while let Some((key, value)) = map.next_entry::<String, V>()? {
                if entries.insert(key, value).is_some() {
                    return Err(serde::de::Error::custom("duplicate coverage map key"));
                }
            }
            Ok(entries)
        }
    }
    deserializer.deserialize_map(Unique(std::marker::PhantomData))
}
/// A scored callable and its source location.
#[derive(Serialize)]
pub struct Item {
    /// Root-relative source path.
    pub file: String,
    /// Display name derived from the callable syntax.
    pub name: String,
    /// Syntax node kind.
    pub kind: String,
    /// One-based starting line.
    pub line: usize,
    /// One-based starting byte column.
    pub column: usize,
    /// One-based ending line.
    pub end_line: usize,
    /// Cyclomatic complexity score.
    pub complexity: u32,
    /// Fraction of supplied coverage points hit, or absent without evidence.
    pub coverage: Option<f64>,
    /// Calculated CRAP score.
    pub crap: f64,
}
/// Ordered scan results and the resulting policy decision.
#[derive(Serialize)]
pub struct Report {
    /// Score threshold used for the policy; equality passes.
    pub threshold: f64,
    /// Number of selected source files.
    pub files_scanned: usize,
    /// Whether every callable has coverage evidence.
    pub coverage_complete: bool,
    /// Callables in deterministic scan order.
    pub functions: Vec<Item>,
    /// Whether any configured policy failed.
    pub failed: bool,
}
mod sarif;
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
fn collect(
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
/// Decode UTF-8 or BOM-marked UTF-16 source, rejecting malformed bytes instead of replacing them.
fn decode_source(bytes: &[u8]) -> Result<String> {
    if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
        let le = bytes[0] == 0xff;
        let data = &bytes[2..];
        if !data.len().is_multiple_of(2) {
            bail!("invalid UTF-16 source");
        }
        let units: Vec<u16> = data
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| {
                if le {
                    u16::from_le_bytes([c[0], c[1]])
                } else {
                    u16::from_be_bytes([c[0], c[1]])
                }
            })
            .collect();
        Ok(String::from_utf16(&units).context("invalid UTF-16 source")?)
    } else {
        Ok(
            std::str::from_utf8(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes))
                .context("invalid UTF-8 source")?
                .to_owned(),
        )
    }
}
/// Escape control characters before writing untrusted source names or errors to a terminal.
pub fn terminal_safe(s: &str) -> String {
    let mut text = String::with_capacity(s.len());
    for character in s.chars() {
        if character.is_control() {
            text.extend(character.escape_default());
        } else {
            text.push(character);
        }
    }
    text
}

/// Validate inputs and scan selected C# sources.
pub fn scan(args: &ScanOptions) -> Result<ScanResult> {
    if !args.threshold.is_finite() || args.threshold < 0.0 {
        bail!("threshold must be finite and nonnegative");
    }
    let root = args.root.canonicalize().context("invalid root")?;
    if !root.is_dir() {
        bail!("root must be a directory");
    }
    let coverage: Option<Coverage> = args
        .coverage
        .as_ref()
        .map(|p| -> Result<_> {
            serde_json::from_slice(&fs::read(p).context("reading coverage")?)
                .context("invalid coverage JSON")
        })
        .transpose()?;
    if let Some(c) = &coverage {
        if c.version != 1 {
            bail!("unsupported coverage version");
        }
        for (path, f) in &c.files {
            if path.contains(['\\', ':'])
                || Path::new(path)
                    .components()
                    .any(|x| !matches!(x, Component::Normal(_)))
                || path.is_empty()
                || path.split('/').any(|part| part == "." || part == "..")
                || path.contains("//")
                || path.ends_with('/')
                || f.sha256.len() != 64
                || !f.sha256.bytes().all(|b| b.is_ascii_hexdigit())
            {
                bail!("invalid coverage entry: {path}");
            }
            for l in f.lines.keys() {
                if l.parse::<usize>()
                    .ok()
                    .filter(|v| *v > 0 && v.to_string() == *l)
                    .is_none()
                {
                    bail!("invalid coverage line: {l}");
                }
            }
        }
    }
    let mut files = BTreeSet::new();
    for path in if args.paths.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        args.paths.clone()
    } {
        let candidate = if path.is_absolute() {
            path
        } else {
            root.join(path)
        };
        let candidate = candidate.canonicalize().context("invalid input path")?;
        if !candidate.starts_with(&root) {
            bail!("input path escapes root");
        }
        if candidate
            .strip_prefix(&root)?
            .components()
            .any(|part| matches!(part.as_os_str().to_str(), Some("bin" | "obj" | ".git")))
        {
            continue;
        }
        for entry in WalkDir::new(candidate)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| {
                let n = e.file_name().to_string_lossy();
                e.depth() == 0
                    || !(e.file_type().is_symlink()
                        || e.file_type().is_dir() && matches!(n.as_ref(), "bin" | "obj" | ".git"))
            })
        {
            let e = entry?;
            if e.file_type().is_file()
                && e.path()
                    .extension()
                    .is_some_and(|x| x.eq_ignore_ascii_case("cs"))
            {
                let name = e.file_name().to_string_lossy().to_ascii_lowercase();
                if !name.ends_with(".g.cs")
                    && !name.ends_with(".generated.cs")
                    && !name.ends_with(".designer.cs")
                {
                    files.insert(e.path().to_path_buf());
                }
            }
        }
    }
    let mut parser = TsParser::new();
    parser.set_language(&tree_sitter_c_sharp::LANGUAGE.into())?;
    let mut items = Vec::new();
    let mut source_hashes = Vec::new();
    for path in &files {
        let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        let hash = hex::encode(Sha256::digest(&bytes));
        let source =
            decode_source(&bytes).with_context(|| format!("decoding {}", path.display()))?;
        let relative = path
            .strip_prefix(&root)?
            .to_str()
            .context("non-UTF-8 source path")?
            .to_owned();
        #[cfg(windows)]
        let relative = relative.replace('\\', "/");
        if coverage.is_some() && relative.contains(['\\', ':']) {
            bail!("source path contains characters unsupported by native coverage: {relative}");
        }
        source_hashes.push((path.clone(), hash.clone()));
        let tree = parser.parse(&source, None).context("parser failed")?;
        if tree.root_node().has_error() {
            bail!("invalid C# syntax in {relative}");
        }
        let cov = coverage.as_ref().and_then(|c| c.files.get(&relative));
        if let Some(c) = cov {
            if !hash.eq_ignore_ascii_case(&c.sha256) {
                bail!("stale coverage for {relative}");
            }
            let line_count = source.lines().count();
            if c.lines
                .keys()
                .any(|l| l.parse::<usize>().unwrap_or(usize::MAX) > line_count)
            {
                bail!("coverage line outside {relative}");
            }
        }
        let points = cov.map(|c| {
            c.lines
                .iter()
                .map(|(line, &hits)| (line.parse::<usize>().expect("validated line"), hits))
                .collect()
        });
        collect(
            tree.root_node(),
            &source,
            &relative,
            points.as_ref(),
            &mut items,
            1_000_000,
            1_000_000,
        )?;
    }
    let coverage_complete = items.iter().all(|i| i.coverage.is_some());
    let failed = items.iter().any(|i| i.crap > args.threshold)
        || args.require_coverage && !coverage_complete
        || args.fail_on_empty && items.is_empty();
    let report = Report {
        threshold: args.threshold,
        files_scanned: files.len(),
        coverage_complete,
        functions: items,
        failed,
    };
    let mut input_paths: Vec<PathBuf> = files.into_iter().collect();
    input_paths.extend(args.coverage.iter().cloned());
    let result = ScanResult {
        report,
        root,
        input_paths,
        source_hashes,
        require_coverage: args.require_coverage,
        fail_on_empty: args.fail_on_empty,
    };
    result.verify_sources()?;
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;

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
