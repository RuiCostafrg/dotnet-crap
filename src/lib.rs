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
    collections::BTreeSet,
    fs,
    num::NonZeroUsize,
    path::{Path, PathBuf},
};
use tree_sitter::Parser as TsParser;

/// Typed analysis of C# callables and their CRAP scores.
mod analysis;
/// Regression baselines and move-aware matching.
pub mod baseline;
/// Native coverage schema, reading and validation.
mod evidence;
/// Deterministic ordered worker pool for per-file work.
mod par;
/// Partial scoring of files that do not parse.
mod partial;
/// Directives the grammar accepts in positions the 0.23.5 grammar rejected.
mod placement;
/// Conditional-compilation fallback for sources that do not parse as written.
mod preprocess;
/// Report model and text rendering.
mod report;
/// Input path resolution and source file discovery.
mod selection;
/// Source byte decoding.
mod source;
/// Test-only per-file work counters.
#[cfg(test)]
mod work;

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
    /// Worker threads for per-file scanning and source verification; `None`
    /// uses the available parallelism. The report is identical for any value.
    pub jobs: Option<NonZeroUsize>,
    /// Conditional compilation symbols, as the CLI's `--define`. They are used
    /// only for a file that does not parse as written: its `#if` groups are
    /// then resolved with these symbols and the file is parsed again. Each
    /// must match `[A-Za-z_][A-Za-z0-9_]*`, be at most 128 characters and not
    /// be `true` or `false` in any letter case. Empty by default (no symbol
    /// defined).
    pub defines: Vec<String>,
    /// Skip selected files whose final failure is a syntax error instead of
    /// failing the whole scan. Skipped files contribute no callables and are
    /// listed in `Report::skipped`, in scan order. Other failures (I/O,
    /// decoding, resource limits) still fail the scan. Defaults to `false`,
    /// which fails on the first syntax error as before.
    pub skip_unparsed: bool,
    /// Score the intact callables of a selected file whose final failure is a
    /// syntax error, instead of failing the scan or skipping the whole file.
    /// A callable is scored only when its own syntax, and that of any
    /// enclosing callable, has no error. Such files are listed in
    /// `Report::partial` with the number of callables left unscored. Takes
    /// precedence over `skip_unparsed`. Defaults to `false`.
    pub partial_unparsed: bool,
    /// Honor `.gitignore` and `.ignore` files at the root and below while
    /// walking directories. Files named explicitly in `paths` are still
    /// selected. Defaults to `true`; `false` disables ignore files entirely.
    pub respect_ignore_files: bool,
    /// Gitignore-style patterns relative to the root. When non-empty, a
    /// selected file must match at least one. Applies to explicit files too.
    pub include: Vec<String>,
    /// Gitignore-style patterns relative to the root; a selected file matching
    /// any of them is dropped. Applies to explicit files too.
    pub exclude: Vec<String>,
    /// A baseline file, resolved against the working directory, to compare
    /// with. The score policy then fails only for callables that are new or
    /// worse than the baseline; `require_coverage` and `fail_on_empty` are
    /// unaffected. Its threshold must equal `threshold`. Defaults to `None`.
    pub baseline: Option<PathBuf>,
    /// Root-relative `(old, new)` file paths: baseline entries for `old` are
    /// matched as if they were in `new`. Requires `baseline`.
    pub baseline_moves: Vec<(String, String)>,
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
            jobs: None,
            defines: Vec::new(),
            skip_unparsed: false,
            partial_unparsed: false,
            respect_ignore_files: true,
            include: Vec::new(),
            exclude: Vec::new(),
            baseline: None,
            baseline_moves: Vec::new(),
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
    jobs: usize,
    /// Baseline entries that match no failing callable now; empty without a baseline.
    resolved: Vec<baseline::Entry>,
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
    /// Whether `require_coverage` or `fail_on_empty` fails for this scan,
    /// whatever any baseline accepts.
    pub fn completeness_failed(&self) -> bool {
        completeness_failed(&self.report, self.require_coverage, self.fail_on_empty)
    }
    /// Treat every score violation as accepted, as when a baseline is being
    /// written: `report().failed` then reflects only the completeness
    /// policies, and renderings show it.
    pub fn accept_scores(&mut self) {
        self.report.failed = self.completeness_failed();
    }
    /// Write the callables that fail the threshold as a baseline at `path`,
    /// atomically, and return how many entries it holds. An existing file is
    /// an error unless `overwrite` is set. The path cannot be a C# source,
    /// a symbolic link or an input of this scan.
    pub fn write_baseline(&self, path: &Path, overwrite: bool) -> Result<usize> {
        let baseline =
            baseline::Baseline::from_items(&self.report.functions, self.report.threshold);
        output::publish(
            self,
            &baseline.render()?,
            path,
            "baseline output",
            overwrite,
        )?;
        Ok(baseline.entries.len())
    }
    /// Verify that selected source bytes still match the scanned bytes.
    ///
    /// Uses the scan's worker count. The first failing source in scan order
    /// is reported, as by a sequential check.
    pub fn verify_sources(&self) -> Result<()> {
        let files = self.source_hashes.len();
        let workers = vec![(); par::workers_for(files, self.jobs)];
        par::ordered_map(
            files,
            workers,
            |_| 0,
            |(), i| {
                let (path, hash) = &self.source_hashes[i];
                #[cfg(test)]
                work::count(work::Unit::Read);
                #[cfg(test)]
                work::count(work::Unit::Hash);
                if hex::encode(Sha256::digest(fs::read(path).kind(ErrorKind::Io)?)) != *hash {
                    fail!(
                        SourceChanged,
                        "source changed during scan: {}",
                        path.display()
                    );
                }
                Ok(())
            },
        )
        .map(drop)
    }
    /// Render the report using the policies captured by the scan.
    pub fn render(&self, format: ReportFormat) -> Result<String> {
        let report = &self.report;
        let output = if format == ReportFormat::Sarif {
            format!(
                "{}\n",
                sarif::render(
                    report,
                    &self.resolved,
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
pub use report::{
    terminal_safe, BaselineState, BaselineSummary, Item, PartialFile, Report, ReportFormat,
    SkippedFile,
};
pub(crate) use source::decode_source;
/// Strict Cobertura line-evidence importer.
pub mod cobertura;
/// Explicit trusted-process collection, separate from pure scanning.
pub mod collector;
/// Safe publication of rendered scan reports.
pub mod config;
/// Typed library errors.
pub mod error;
pub mod output;
mod sarif;
pub use error::{Error, ErrorKind, Result};

/// Validate inputs and scan selected C# sources.
pub fn scan(args: &ScanOptions) -> Result<ScanResult> {
    let budget = par::ByteBudget::new(par::MIN_BYTES_IN_FLIGHT);
    Ok(scan_impl(args, false, &budget)?.0)
}

/// Facts about one source, kept together to avoid positional hash/count pairing.
pub(crate) struct ScannedSource {
    pub(crate) path: PathBuf,
    pub(crate) hash: String,
    pub(crate) line_count: usize,
}

/// [`scan`] that also returns source facts from exactly the bytes hashed and
/// parsed, in scan order. `collect` uses them without re-reading source text.
pub(crate) fn scan_with_line_counts(
    args: &ScanOptions,
) -> Result<(ScanResult, Vec<ScannedSource>)> {
    let budget = par::ByteBudget::new(par::MIN_BYTES_IN_FLIGHT);
    scan_impl(args, true, &budget)
}

/// One scanned source: its identity, its line count when requested, and its callables.
struct FileScan {
    path: PathBuf,
    hash: String,
    line_count: Option<usize>,
    items: Vec<Item>,
    /// Set when the file did not parse and skipping is enabled.
    skipped: Option<SkippedFile>,
    /// Set when the file did not parse and only its intact callables were scored.
    partial: Option<PartialFile>,
}

/// Shared [`scan`] body; with `want_sources`, also returns facts for each
/// source. `budget` bounds the source bytes parsed at once.
fn scan_impl(
    args: &ScanOptions,
    want_sources: bool,
    budget: &par::ByteBudget,
) -> Result<(ScanResult, Vec<ScannedSource>)> {
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
    let baseline = args
        .baseline
        .as_deref()
        .map(|path| load_baseline(path, args.threshold))
        .transpose()?;
    if baseline.is_none() && !args.baseline_moves.is_empty() {
        fail!(InvalidInput, "baseline moves require a baseline");
    }
    let coverage: Option<Coverage> = args
        .coverage
        .as_ref()
        .map(|p| evidence::read(p))
        .transpose()?;
    if let Some(c) = &coverage {
        evidence::validate(c)?;
    }
    let files: Vec<PathBuf> = selection::select(
        &root,
        &args.paths,
        &selection::Filters {
            respect_ignore_files: args.respect_ignore_files,
            include: &args.include,
            exclude: &args.exclude,
        },
    )?
    .into_iter()
    .collect();
    // Checked after the root, coverage and paths, so those errors keep their
    // precedence over a symbol that only matters to the fallback.
    if let Some(symbol) = args.defines.iter().find(|s| !preprocess::is_symbol(s)) {
        fail!(
            InvalidInput,
            "invalid conditional compilation symbol {symbol:?}: use an ASCII letter or `_` followed by ASCII letters, digits or `_`, at most 128 characters, other than `true` and `false` in any letter case"
        );
    }
    let defines: BTreeSet<String> = args.defines.iter().cloned().collect();
    let jobs = par::resolve(args.jobs);
    let parsers = (0..par::workers_for(files.len(), jobs))
        .map(|_| {
            let mut parser = TsParser::new();
            parser
                .set_language(&tree_sitter_c_sharp::LANGUAGE.into())
                .kind(ErrorKind::Source)?;
            Ok(parser)
        })
        .collect::<Result<Vec<_>>>()?;
    let kinds = analysis::KindTable::new(&tree_sitter_c_sharp::LANGUAGE.into());
    let context = FileContext {
        root: &root,
        coverage: coverage.as_ref(),
        kinds: &kinds,
        defines: &defines,
        want_lines: want_sources,
        skip_unparsed: args.skip_unparsed,
        partial_unparsed: args.partial_unparsed,
        budget,
    };
    let scanned = par::ordered_map(
        files.len(),
        parsers,
        // Large files first, so one big file does not become the tail.
        |i| fs::metadata(&files[i]).map_or(0, |m| m.len()),
        |parser, i| context.scan_file(parser, &files[i]),
    )?;
    let mut items = Vec::new();
    let mut skipped = Vec::new();
    let mut partial = Vec::new();
    let mut source_hashes = Vec::with_capacity(scanned.len());
    let mut sources = Vec::new();
    for mut file in scanned {
        items.append(&mut file.items);
        skipped.extend(file.skipped.take());
        partial.extend(file.partial.take());
        if want_sources {
            sources.push(ScannedSource {
                path: file.path.clone(),
                hash: file.hash.clone(),
                line_count: file.line_count.expect("line count requested"),
            });
        }
        source_hashes.push((file.path, file.hash));
    }
    let coverage_complete = items.iter().all(|i| i.coverage.is_some());
    let mut summary = None;
    let mut resolved = Vec::new();
    if let Some(baseline) = &baseline {
        let comparison = baseline::compare(
            &baseline.entries,
            &items,
            args.threshold,
            &args.baseline_moves,
        );
        for (item, class) in items.iter_mut().zip(&comparison.items) {
            if let Some(class) = class {
                item.baseline = Some(class.state);
                item.baseline_crap = class.was;
            }
        }
        summary = Some(comparison.summary());
        resolved = comparison
            .resolved
            .iter()
            .map(|&e| baseline.entries[e].clone())
            .collect();
    }
    let mut report = Report {
        version: 1,
        threshold: args.threshold,
        files_scanned: files.len(),
        skipped,
        partial,
        coverage_complete,
        functions: items,
        failed: false,
        baseline: summary,
    };
    // With a baseline only new or worse callables fail the score policy.
    let scores_failed = match &report.baseline {
        Some(summary) => summary.new + summary.worse > 0,
        None => report.functions.iter().any(|i| i.crap > args.threshold),
    };
    report.failed =
        scores_failed || completeness_failed(&report, args.require_coverage, args.fail_on_empty);
    let mut input_paths = files;
    input_paths.extend(args.coverage.iter().cloned());
    input_paths.extend(args.baseline.iter().cloned());
    let result = ScanResult {
        report,
        root,
        input_paths,
        source_hashes,
        require_coverage: args.require_coverage,
        fail_on_empty: args.fail_on_empty,
        jobs,
        resolved,
    };
    result.verify_sources()?;
    Ok((result, sources))
}

/// Whether a completeness policy fails: required coverage is missing, or an
/// empty selection is not allowed. A baseline never suppresses these.
fn completeness_failed(report: &Report, require_coverage: bool, fail_on_empty: bool) -> bool {
    require_coverage && !report.coverage_complete || fail_on_empty && report.functions.is_empty()
}

/// Read a baseline and check it was written under the scan's threshold.
fn load_baseline(path: &Path, threshold: f64) -> Result<baseline::Baseline> {
    if output::is_csharp_path(path) {
        fail!(InvalidInput, "baseline cannot be a C# source file");
    }
    let baseline = baseline::read(path)?;
    baseline::check_threshold(&baseline, threshold)?;
    Ok(baseline)
}

/// Inputs shared by every per-file scan, read-only across workers.
struct FileContext<'a> {
    root: &'a Path,
    coverage: Option<&'a Coverage>,
    kinds: &'a analysis::KindTable,
    defines: &'a BTreeSet<String>,
    want_lines: bool,
    skip_unparsed: bool,
    partial_unparsed: bool,
    budget: &'a par::ByteBudget,
}

impl FileContext<'_> {
    /// Read, hash, decode, parse and score one source. The check order defines
    /// which error a file reports and matches the sequential scan exactly.
    fn scan_file(&self, parser: &mut TsParser, path: &Path) -> Result<FileScan> {
        // Admit the file before reading it, so its raw bytes, decoded text and
        // tree all count against the budget. Admission cannot fail; a file that
        // cannot be stat'ed is admitted as empty and its read reports the error.
        // Declared first, so it is dropped last, after the tree and the text.
        let size_hint =
            fs::metadata(path).map_or(0, |m| usize::try_from(m.len()).unwrap_or(usize::MAX));
        let _permit = self.budget.acquire(size_hint);
        let bytes =
            fs::read(path).with_context(ErrorKind::Io, || format!("reading {}", path.display()))?;
        #[cfg(test)]
        work::count(work::Unit::Read);
        let hash = hex::encode(Sha256::digest(&bytes));
        #[cfg(test)]
        work::count(work::Unit::Hash);
        let source = decode_source(bytes)
            .with_context(ErrorKind::Source, || format!("decoding {}", path.display()))?;
        let relative = path
            .strip_prefix(self.root)
            .kind(ErrorKind::InvalidInput)?
            .to_str()
            .context(ErrorKind::Source, "non-UTF-8 source path")?
            .to_owned();
        #[cfg(windows)]
        let relative = relative.replace('\\', "/");
        if self.coverage.is_some() && relative.contains(['\\', ':']) {
            fail!(
                Source,
                "source path contains characters unsupported by native coverage: {relative}"
            );
        }
        let tree = parser
            .parse(&source, None)
            .context(ErrorKind::Source, "parser failed")?;
        #[cfg(test)]
        work::count(work::Unit::Parse);
        // Files that parse as written are scored from their own text. Only a
        // file whose parse failed, or whose tree holds a conditional directive
        // in a position the 0.23.5 grammar rejected (an `#if` that splits a
        // declaration, which grammar 0.24.x accepts as a body-less method),
        // pays for conditional-compilation resolution and a second parse; the
        // resolved text keeps every byte offset and line. While resolving, the
        // decoded and resolved texts are both held, so a fallback file briefly
        // uses twice its text against one admission.
        let mut partial = None;
        let (source, tree) = if tree.root_node().has_error()
            || placement::misplaced_directive(tree.root_node(), &source)
        {
            // Partial scoring reads the original tree of a file with errors.
            let original = (self.partial_unparsed && tree.root_node().has_error()).then_some(tree);
            match self.reparse_resolved(parser, &source) {
                Some(resolved) => {
                    drop(original);
                    drop(source);
                    resolved
                }
                // With partial scoring, the original tree is scored and only
                // its intact callables are kept below.
                None if original.is_some() => {
                    partial = Some(format!("invalid C# syntax in {relative}"));
                    (source, original.expect("kept for partial scoring"))
                }
                // A file whose final failure is a syntax error is skipped when
                // enabled. It binds coverage like a selected file with no
                // callables, and contributes no items.
                None if self.skip_unparsed => {
                    let cov = self.coverage.and_then(|c| c.files.get(&relative));
                    let line_count =
                        (cov.is_some() || self.want_lines).then(|| source.lines().count());
                    if let Some(c) = cov {
                        evidence::check_binding(c, &hash, line_count.expect("counted"), &relative)?;
                    }
                    return Ok(FileScan {
                        path: path.to_path_buf(),
                        hash,
                        line_count,
                        items: Vec::new(),
                        skipped: Some(SkippedFile {
                            path: relative.clone(),
                            message: format!("invalid C# syntax in {relative}"),
                        }),
                        partial: None,
                    });
                }
                None => fail!(Syntax, "invalid C# syntax in {relative}"),
            }
        } else {
            (source, tree)
        };
        let cov = self.coverage.and_then(|c| c.files.get(&relative));
        let line_count = (cov.is_some() || self.want_lines).then(|| source.lines().count());
        if let Some(c) = cov {
            evidence::check_binding(c, &hash, line_count.expect("counted"), &relative)?;
        }
        let points = cov.map(|c| {
            analysis::CoverageIndex::new(
                c.lines
                    .iter()
                    .map(|(line, &hits)| (line.parse::<usize>().expect("validated line"), hits)),
            )
        });
        let mut items = Vec::new();
        analysis::collect(
            self.kinds,
            tree.root_node(),
            &source,
            &relative,
            points.as_ref(),
            &mut items,
            1_000_000,
            1_000_000,
        )?;
        let partial = partial.map(|message| {
            let (kept, unscored) =
                partial::keep_intact(tree.root_node(), &source, items.split_off(0));
            items = kept;
            PartialFile {
                path: relative.clone(),
                message,
                unscored,
            }
        });
        Ok(FileScan {
            path: path.to_path_buf(),
            hash,
            line_count,
            items,
            skipped: None,
            partial,
        })
    }

    /// Resolve conditional compilation in a source whose tree has errors and
    /// parse it again. `None` means the original syntax error stands.
    fn reparse_resolved(
        &self,
        parser: &mut TsParser,
        source: &str,
    ) -> Option<(String, tree_sitter::Tree)> {
        let resolved = preprocess::resolve(source, self.defines)?;
        // A parser that returns no tree leaves the original error standing.
        let tree = parser.parse(&resolved, None)?;
        #[cfg(test)]
        work::count(work::Unit::Parse);
        (!tree.root_node().has_error()).then_some((resolved, tree))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OK: &str = "class C { int F(int x) { return x > 0 ? 1 : 2; } }";

    fn options(root: &Path, jobs: usize) -> ScanOptions {
        ScanOptions {
            root: root.to_path_buf(),
            threshold: 1e12,
            jobs: NonZeroUsize::new(jobs),
            ..Default::default()
        }
    }

    /// A class with `methods` small branching methods, about 45 bytes each.
    fn class_with(name: &str, methods: usize) -> String {
        let mut text = format!("class {name} {{\n");
        for i in 0..methods {
            text.push_str(&format!("int M{i}(int x) {{ return x > {i} ? 1 : 2; }}\n"));
        }
        text.push_str("}\n");
        text
    }

    /// Many valid files of mixed sizes in nested directories.
    fn valid_tree() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..40 {
            let sub = dir.path().join(format!("p{:02}", i % 7));
            fs::create_dir_all(&sub).unwrap();
            // Every seventh file is much larger, so claim order differs from index order.
            let methods = if i % 7 == 3 { 300 } else { 1 + i % 5 };
            fs::write(
                sub.join(format!("f{i:02}.cs")),
                class_with(&format!("C{i}"), methods),
            )
            .unwrap();
        }
        dir
    }

    fn json(result: &ScanResult) -> String {
        result.render(ReportFormat::Json).unwrap()
    }

    #[test]
    fn report_hashes_and_sources_are_identical_for_one_to_eight_workers() {
        let dir = valid_tree();
        let (base, base_sources) = scan_with_line_counts(&options(dir.path(), 1)).unwrap();
        assert_eq!(base.report().files_scanned, 40);
        for jobs in 1..=8 {
            let (other, sources) = scan_with_line_counts(&options(dir.path(), jobs)).unwrap();
            assert_eq!(other.jobs, jobs);
            assert_eq!(json(&other), json(&base), "jobs {jobs}");
            assert_eq!(
                other.render(ReportFormat::Sarif).unwrap(),
                base.render(ReportFormat::Sarif).unwrap()
            );
            assert_eq!(other.source_hashes, base.source_hashes, "jobs {jobs}");
            assert_eq!(other.input_paths(), base.input_paths());
            let facts = |s: &[ScannedSource]| {
                s.iter()
                    .map(|s| (s.path.clone(), s.hash.clone(), s.line_count))
                    .collect::<Vec<_>>()
            };
            assert_eq!(facts(&sources), facts(&base_sources), "jobs {jobs}");
            other.verify_sources().unwrap();
        }
        let default = scan(&ScanOptions {
            jobs: None,
            ..options(dir.path(), 1)
        })
        .unwrap();
        assert_eq!(default.jobs, par::resolve(None));
        assert_eq!(json(&default), json(&base));
    }

    /// Valid files, then a slow-to-parse syntax failure first in sorted order,
    /// then files that fail differently and immediately.
    fn failing_tree() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..30 {
            fs::write(dir.path().join(format!("a{i:02}.cs")), OK).unwrap();
        }
        let mut big = class_with("Big", 4000);
        big.truncate(big.len() - 2);
        big.push_str("int Broken( {\n");
        fs::write(dir.path().join("b_slow_syntax.cs"), big).unwrap();
        fs::write(dir.path().join("c_bad_utf8.cs"), [0xffu8, 0xfe, 0x41]).unwrap();
        fs::write(dir.path().join("d_syntax.cs"), "class {").unwrap();
        dir
    }

    fn assert_same_error(sequential: &Error, other: &Error, jobs: usize) {
        assert_eq!(other.kind(), sequential.kind(), "jobs {jobs}");
        assert_eq!(
            format!("{other:#}"),
            format!("{sequential:#}"),
            "jobs {jobs}"
        );
    }

    #[test]
    fn the_first_failing_file_in_scan_order_wins_for_any_worker_count() {
        let dir = failing_tree();
        let sequential = scan(&options(dir.path(), 1)).unwrap_err();
        assert_eq!(sequential.kind(), ErrorKind::Syntax);
        assert!(sequential.to_string().ends_with("b_slow_syntax.cs"));
        for _ in 0..10 {
            for jobs in 2..=8 {
                let error = scan(&options(dir.path(), jobs)).unwrap_err();
                assert_same_error(&sequential, &error, jobs);
            }
        }
    }

    #[test]
    fn stale_coverage_precedes_later_decode_and_syntax_failures() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("src");
        fs::create_dir(&root).unwrap();
        for i in 0..12 {
            fs::write(root.join(format!("a{i:02}.cs")), OK).unwrap();
        }
        // The heaviest files fail last in order, so they are claimed and fail first.
        fs::write(root.join("b_stale.cs"), OK).unwrap();
        let mut bad = vec![0xffu8, 0xfe, 0x41];
        bad.resize(200_001, 0x41); // odd length: invalid UTF-16
        fs::write(root.join("c_bad_utf16.cs"), bad).unwrap();
        fs::write(root.join("d_syntax.cs"), class_with("D", 3000) + "class {").unwrap();
        let coverage = dir.path().join("coverage.json");
        fs::write(
            &coverage,
            format!(
                r#"{{"version":1,"files":{{"b_stale.cs":{{"sha256":"{}","lines":{{"1":1}}}}}}}}"#,
                "0".repeat(64)
            ),
        )
        .unwrap();
        let with_coverage = |jobs| ScanOptions {
            coverage: Some(coverage.clone()),
            ..options(&root, jobs)
        };
        let sequential = scan(&with_coverage(1)).unwrap_err();
        assert_eq!(sequential.kind(), ErrorKind::Coverage);
        assert_eq!(sequential.to_string(), "stale coverage for b_stale.cs");
        for jobs in [2, 3, 4, 8] {
            let error = scan(&with_coverage(jobs)).unwrap_err();
            assert_same_error(&sequential, &error, jobs);
        }
    }

    #[test]
    fn the_check_order_within_a_file_is_kept() {
        // A decode failure wins over the next file's syntax failure.
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.cs"), [0xffu8, 0xfe, 0x41]).unwrap();
        fs::write(dir.path().join("b.cs"), "class {").unwrap();
        for jobs in [1, 2, 4] {
            let error = scan(&options(dir.path(), jobs)).unwrap_err();
            assert_eq!(error.kind(), ErrorKind::Source, "jobs {jobs}");
            assert_eq!(
                format!("{error:#}"),
                format!(
                    "decoding {}: invalid UTF-16 source",
                    dir.path().canonicalize().unwrap().join("a.cs").display()
                )
            );
        }
    }

    #[test]
    fn skipped_files_keep_scan_order_for_any_worker_count() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..8 {
            fs::write(dir.path().join(format!("a{i:02}.cs")), OK).unwrap();
            fs::write(dir.path().join(format!("b{i:02}.cs")), "class {").unwrap();
        }
        let expected: Vec<String> = (0..8).map(|i| format!("b{i:02}.cs")).collect();
        let sequential = scan(&ScanOptions {
            skip_unparsed: true,
            ..options(dir.path(), 1)
        })
        .unwrap();
        for jobs in 1..=4 {
            let result = scan(&ScanOptions {
                skip_unparsed: true,
                ..options(dir.path(), jobs)
            })
            .unwrap();
            let report = result.report();
            assert_eq!(report.files_scanned, 16);
            assert_eq!(report.functions.len(), 8);
            let paths: Vec<&str> = report.skipped.iter().map(|s| s.path.as_str()).collect();
            assert_eq!(paths, expected, "jobs {jobs}");
            assert!(report
                .skipped
                .iter()
                .all(|s| s.message == format!("invalid C# syntax in {}", s.path)));
            assert_eq!(json(&result), json(&sequential), "jobs {jobs}");
        }
    }

    #[test]
    fn verification_reports_the_first_changed_file_in_order() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..20 {
            fs::write(dir.path().join(format!("f{i:02}.cs")), OK).unwrap();
        }
        let results: Vec<ScanResult> = [1, 2, 8]
            .into_iter()
            .map(|jobs| scan(&options(dir.path(), jobs)).unwrap())
            .collect();
        fs::write(dir.path().join("f05.cs"), format!("{OK} ")).unwrap();
        fs::write(dir.path().join("f12.cs"), format!("{OK} ")).unwrap();
        fs::remove_file(dir.path().join("f03.cs")).unwrap();
        for result in &results {
            let error = result.verify_sources().unwrap_err();
            assert_eq!(error.kind(), ErrorKind::Io, "jobs {}", result.jobs);
        }
        fs::write(dir.path().join("f03.cs"), OK).unwrap();
        for result in &results {
            let error = result.verify_sources().unwrap_err();
            assert_eq!(error.kind(), ErrorKind::SourceChanged);
            assert!(
                error.to_string().ends_with("f05.cs"),
                "jobs {}",
                result.jobs
            );
        }
    }

    #[test]
    fn bytes_in_flight_stay_within_the_bound_with_several_large_files() {
        let dir = tempfile::tempdir().unwrap();
        let mut largest = 0;
        for i in 0..6 {
            let text = class_with(&format!("L{i}"), 200 + 40 * i);
            largest = largest.max(text.len());
            fs::write(dir.path().join(format!("large{i}.cs")), text).unwrap();
        }
        for i in 0..20 {
            fs::write(dir.path().join(format!("small{i:02}.cs")), OK).unwrap();
        }
        // Every large file exceeds this floor, so each one must parse alone.
        let floor = 4096;
        assert!(largest > floor && 200 * 45 > floor);
        let sequential = scan(&options(dir.path(), 1)).unwrap();
        for jobs in [2, 4, 8] {
            let budget = par::ByteBudget::new(floor);
            let (result, _) = scan_impl(&options(dir.path(), jobs), false, &budget).unwrap();
            assert_eq!(budget.peak(), largest, "jobs {jobs}");
            assert_eq!(json(&result), json(&sequential));
        }
        // The production floor admits this whole tree, so the bound never exceeds it.
        let budget = par::ByteBudget::new(par::MIN_BYTES_IN_FLIGHT);
        scan_impl(&options(dir.path(), 8), false, &budget).unwrap();
        assert!((largest..=par::MIN_BYTES_IN_FLIGHT).contains(&budget.peak()));
    }

    #[test]
    fn each_worker_gets_its_own_parser_and_reports_its_work() {
        let dir = valid_tree();
        work::take();
        scan(&options(dir.path(), 4)).unwrap();
        // One read, hash, decode and parse per file, plus the verification's read and hash.
        assert_eq!(work::take(), [80, 80, 40, 40]);
    }

    #[test]
    fn only_files_that_fail_to_parse_are_resolved_and_parsed_again() {
        let dir = tempfile::tempdir().unwrap();
        // Parses as written, `#if` included: one parse, whatever is defined.
        fs::write(
            dir.path().join("a.cs"),
            "class A {\n#if X\n int F() { return 1; }\n#endif\n}\n",
        )
        .unwrap();
        // Fails as written, parses once resolved: two parses.
        fs::write(
            dir.path().join("b.cs"),
            "class B {\n int G()\n#if X\n { return 1; }\n#else\n ;\n#endif\n}\n",
        )
        .unwrap();
        let mut hashes = Vec::new();
        for defines in [vec![], vec!["X".to_owned()], vec!["Y".to_owned()]] {
            work::take();
            let result = scan(&ScanOptions {
                defines,
                ..options(dir.path(), 1)
            })
            .unwrap();
            assert_eq!(work::take(), [4, 4, 2, 3]);
            assert_eq!(result.report().functions[0].name, "F");
            hashes.push(result.source_hashes);
        }
        // Source hashes are over the original bytes, whatever is defined.
        let expected: Vec<(PathBuf, String)> = ["a.cs", "b.cs"]
            .iter()
            .map(|name| {
                let path = dir.path().canonicalize().unwrap().join(name);
                let hash = hex::encode(Sha256::digest(fs::read(&path).unwrap()));
                (path, hash)
            })
            .collect();
        assert!(hashes.iter().all(|h| *h == expected), "{hashes:?}");
        // Fails with nothing to resolve: no second parse, the original error.
        fs::write(dir.path().join("c.cs"), "class {\n").unwrap();
        work::take();
        let error = scan(&options(dir.path(), 1)).unwrap_err();
        assert_eq!(error.to_string(), "invalid C# syntax in c.cs");
        assert_eq!(work::take(), [3, 3, 3, 4]);
    }
}
