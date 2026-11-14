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
//! # Ok::<(), dotnet_crap::Error>(())
//! ```
use crate::error::{fail, OptionExt, ResultExt};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};
use tree_sitter::Parser as TsParser;

/// Typed analysis of C# callables and their CRAP scores.
mod analysis;
/// Native coverage schema, reading and validation.
mod evidence;
/// Report model and text rendering.
mod report;
/// Input path resolution and source file discovery.
mod selection;
/// Source byte decoding.
mod source;

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
/// A report and the source identities captured during its scan.
#[derive(Debug)]
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
            if hex::encode(Sha256::digest(fs::read(path).kind(ErrorKind::Io)?)) != *hash {
                fail!(
                    SourceChanged,
                    "source changed during scan: {}",
                    path.display()
                );
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
            format!(
                "{}\n",
                serde_json::to_string_pretty(report).kind(ErrorKind::Render)?
            )
        } else {
            report::render_text(report, self.require_coverage)
        };
        Ok(output)
    }
}
pub(crate) use evidence::{Coverage, CoverageFile};
pub use report::{terminal_safe, Item, Report, ReportFormat};
pub(crate) use source::decode_source;
/// Strict Cobertura line-evidence importer.
pub mod cobertura;
/// Explicit trusted-process collection, separate from pure scanning.
pub mod collector;
/// Typed library errors.
pub mod error;
/// Safe publication of rendered scan reports.
pub mod output;
mod sarif;
pub use error::{Error, ErrorKind, Result};

/// Validate inputs and scan selected C# sources.
pub fn scan(args: &ScanOptions) -> Result<ScanResult> {
    if !args.threshold.is_finite() || args.threshold < 0.0 {
        fail!(InvalidInput, "threshold must be finite and nonnegative");
    }
    let root = args
        .root
        .canonicalize()
        .context(ErrorKind::InvalidInput, "invalid root")?;
    if !root.is_dir() {
        fail!(InvalidInput, "root must be a directory");
    }
    let coverage: Option<Coverage> = args
        .coverage
        .as_ref()
        .map(|p| evidence::read(p))
        .transpose()?;
    if let Some(c) = &coverage {
        evidence::validate(c)?;
    }
    let files = selection::select(&root, &args.paths)?;
    let mut parser = TsParser::new();
    parser
        .set_language(&tree_sitter_c_sharp::LANGUAGE.into())
        .kind(ErrorKind::Source)?;
    let kinds = analysis::KindTable::new(&tree_sitter_c_sharp::LANGUAGE.into());
    let mut items = Vec::new();
    let mut source_hashes = Vec::new();
    for path in &files {
        let bytes =
            fs::read(path).with_context(ErrorKind::Io, || format!("reading {}", path.display()))?;
        let hash = hex::encode(Sha256::digest(&bytes));
        let source = decode_source(&bytes)
            .with_context(ErrorKind::Source, || format!("decoding {}", path.display()))?;
        let relative = path
            .strip_prefix(&root)
            .kind(ErrorKind::InvalidInput)?
            .to_str()
            .context(ErrorKind::Source, "non-UTF-8 source path")?
            .to_owned();
        #[cfg(windows)]
        let relative = relative.replace('\\', "/");
        if coverage.is_some() && relative.contains(['\\', ':']) {
            fail!(
                Source,
                "source path contains characters unsupported by native coverage: {relative}"
            );
        }
        source_hashes.push((path.clone(), hash.clone()));
        let tree = parser
            .parse(&source, None)
            .context(ErrorKind::Source, "parser failed")?;
        if tree.root_node().has_error() {
            fail!(Syntax, "invalid C# syntax in {relative}");
        }
        let cov = coverage.as_ref().and_then(|c| c.files.get(&relative));
        if let Some(c) = cov {
            if !hash.eq_ignore_ascii_case(&c.sha256) {
                fail!(Coverage, "stale coverage for {relative}");
            }
            let line_count = source.lines().count();
            if c.lines
                .keys()
                .any(|l| l.parse::<usize>().unwrap_or(usize::MAX) > line_count)
            {
                fail!(Coverage, "coverage line outside {relative}");
            }
        }
        let points = cov.map(|c| {
            analysis::CoverageIndex::new(
                c.lines
                    .iter()
                    .map(|(line, &hits)| (line.parse::<usize>().expect("validated line"), hits)),
            )
        });
        analysis::collect(
            &kinds,
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
