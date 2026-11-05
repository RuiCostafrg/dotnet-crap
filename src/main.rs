use anyhow::{bail, Context, Result};
use clap::Parser;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
};
use tree_sitter::{Node, Parser as TsParser};
use walkdir::WalkDir;

#[derive(Parser)]
#[command(
    version,
    about = "Score C# callables using cyclomatic complexity and source-bound coverage"
)]
struct Args {
    #[arg(long, default_value = ".")]
    root: PathBuf,
    #[arg(long, default_value_t = 10.0)]
    threshold: f64,
    #[arg(long, default_value = "text", value_parser = ["text", "json", "sarif"])]
    format: String,
    #[arg(long)]
    output: Option<PathBuf>,
    #[arg(long)]
    coverage: Option<PathBuf>,
    #[arg(long)]
    require_coverage: bool,
    #[arg(long)]
    fail_on_empty: bool,
    paths: Vec<PathBuf>,
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
fn unique_map<'de, D, V>(deserializer: D) -> std::result::Result<BTreeMap<String, V>, D::Error>
where
    D: serde::Deserializer<'de>,
    V: Deserialize<'de>,
{
    struct Unique<V>(std::marker::PhantomData<V>);
    impl<'de, V: Deserialize<'de>> serde::de::Visitor<'de> for Unique<V> {
        type Value = BTreeMap<String, V>;
        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a map with unique keys")
        }
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
#[derive(Serialize)]
struct Item {
    file: String,
    name: String,
    kind: String,
    line: usize,
    column: usize,
    end_line: usize,
    complexity: u32,
    coverage: Option<f64>,
    crap: f64,
}
#[derive(Serialize)]
struct Report {
    threshold: f64,
    files_scanned: usize,
    coverage_complete: bool,
    functions: Vec<Item>,
    failed: bool,
}
mod sarif;
fn callable(n: Node<'_>) -> bool {
    matches!(
        n.kind(),
        "method_declaration"
            | "constructor_declaration"
            | "local_function_statement"
            | "accessor_declaration"
            | "lambda_expression"
            | "anonymous_method_expression"
    ) && (n.child_by_field_name("body").is_some() || n.kind() == "anonymous_method_expression")
        || n.kind() == "property_declaration"
            && n.child_by_field_name("value")
                .is_some_and(|v| v.kind() == "arrow_expression_clause")
}
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
            let result = u32::from(n.children(&mut c).any(|child| child.kind() == "case"));
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
fn score(
    n: Node<'_>,
    source: &str,
    complexity: &mut u32,
    excluded: &mut BTreeSet<usize>,
    outer: bool,
) {
    if !outer && callable(n) {
        for line in n.start_position().row..=n.end_position().row {
            excluded.insert(line + 1);
        }
        return;
    }
    *complexity += decision(n, source);
    let mut c = n.walk();
    for child in n.named_children(&mut c) {
        score(child, source, complexity, excluded, false);
    }
}
fn collect(n: Node<'_>, source: &str, file: &str, cov: Option<&CoverageFile>, out: &mut Vec<Item>) {
    if callable(n) {
        let mut complexity = 1;
        let mut excluded = BTreeSet::new();
        score(n, source, &mut complexity, &mut excluded, true);
        let name = n
            .child_by_field_name("name")
            .and_then(|x| x.utf8_text(source.as_bytes()).ok())
            .unwrap_or(n.kind())
            .to_owned();
        let coverage = cov.and_then(|c| {
            let hits: Vec<u64> = c
                .lines
                .iter()
                .filter_map(|(line, hits)| {
                    let l = line.parse::<usize>().ok()?;
                    (l > n.start_position().row
                        && l <= n.end_position().row + 1
                        && !excluded.contains(&l))
                    .then_some(*hits)
                })
                .collect();
            (!hits.is_empty())
                .then(|| hits.iter().filter(|&&h| h > 0).count() as f64 / hits.len() as f64)
        });
        let missed = 1.0 - coverage.unwrap_or(0.0);
        let crap = f64::from(complexity).powi(2) * missed.powi(3) + f64::from(complexity);
        out.push(Item {
            file: file.to_owned(),
            name,
            kind: n.kind().to_owned(),
            line: n.start_position().row + 1,
            column: n.start_position().column + 1,
            end_line: n.end_position().row + 1,
            complexity,
            coverage,
            crap,
        });
    }
    let mut c = n.walk();
    for child in n.named_children(&mut c) {
        collect(child, source, file, cov, out);
    }
}
fn run(args: Args) -> Result<bool> {
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
            if e.file_type().is_file() && e.path().extension().is_some_and(|x| x == "cs") {
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
    for path in &files {
        let source =
            fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let relative = path
            .strip_prefix(&root)?
            .to_str()
            .context("non-UTF-8 source path")?
            .to_owned();
        #[cfg(windows)]
        let relative = relative.replace('\\', "/");
        if coverage.is_some() && relative.contains(['\\', ':']) {
            bail!("source path contains characters unsupported by native coverage");
        }
        let tree = parser.parse(&source, None).context("parser failed")?;
        if tree.root_node().has_error() {
            bail!("invalid C# syntax in {relative}");
        }
        let cov = coverage.as_ref().and_then(|c| c.files.get(&relative));
        if let Some(c) = cov {
            if !hex::encode(Sha256::digest(source.as_bytes())).eq_ignore_ascii_case(&c.sha256) {
                bail!("stale coverage for {relative}");
            }
            if c.lines
                .keys()
                .any(|l| l.parse::<usize>().unwrap_or(usize::MAX) > source.lines().count())
            {
                bail!("coverage line outside {relative}");
            }
        }
        collect(tree.root_node(), &source, &relative, cov, &mut items);
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
    let output = if args.format == "sarif" {
        format!(
            "{}\n",
            sarif::render(&report, &root, args.require_coverage, args.fail_on_empty)?
        )
    } else if args.format == "json" {
        format!("{}\n", serde_json::to_string_pretty(&report)?)
    } else {
        let mut s = String::new();
        if report.functions.is_empty() {
            s.push_str("No implemented C# callables found.\n");
        }
        if args.require_coverage && !coverage_complete {
            s.push_str("Coverage missing for one or more callables.\n");
        }
        for i in &report.functions {
            s.push_str(&format!(
                "{}:{} {} complexity={} coverage={} CRAP={:.2}\n",
                i.file,
                i.line,
                i.name,
                i.complexity,
                i.coverage
                    .map_or_else(|| "absent".into(), |v| format!("{:.1}%", v * 100.0)),
                i.crap
            ));
        }
        s
    };
    if let Some(path) = args.output {
        if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("cs"))
        {
            bail!("report output cannot overwrite C# source");
        }
        if fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
            bail!("report output cannot be a symbolic link");
        }
        if path.exists() {
            for input in files.iter().chain(args.coverage.iter()) {
                if same_file::is_same_file(&path, input)? {
                    bail!("report output aliases an input");
                }
            }
            // Also protect ignored/generated/unselected sources. Do not follow
            // directory links while looking for aliases.
            for entry in WalkDir::new(&root).follow_links(false) {
                let entry = entry?;
                if entry
                    .path()
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("cs"))
                    && entry.path().is_file()
                    && same_file::is_same_file(&path, entry.path())?
                {
                    bail!("report output aliases C# source");
                }
            }
        }
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut staged = tempfile::NamedTempFile::new_in(parent)?;
        staged.write_all(output.as_bytes())?;
        staged.as_file().sync_all()?;
        staged.persist(path)?;
    } else {
        std::io::stdout().lock().write_all(output.as_bytes())?;
    }
    Ok(failed)
}
fn main() {
    let code = match run(Args::parse()) {
        Ok(false) => 0,
        Ok(true) => 1,
        Err(e) => {
            eprintln!("error: {e:#}");
            2
        }
    };
    std::process::exit(code);
}
