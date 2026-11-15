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
    num::NonZeroUsize,
    path::{Path, PathBuf},
};
use tree_sitter::Parser as TsParser;

/// Typed analysis of C# callables and their CRAP scores.
mod analysis;
/// Native coverage schema, reading and validation.
mod evidence;
/// Deterministic ordered worker pool for per-file work.
mod par;
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
    let coverage: Option<Coverage> = args
        .coverage
        .as_ref()
        .map(|p| evidence::read(p))
        .transpose()?;
    if let Some(c) = &coverage {
        evidence::validate(c)?;
    }
    let files: Vec<PathBuf> = selection::select(&root, &args.paths)?.into_iter().collect();
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
        want_lines: want_sources,
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
    let mut source_hashes = Vec::with_capacity(scanned.len());
    let mut sources = Vec::new();
    for mut file in scanned {
        items.append(&mut file.items);
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
    let mut input_paths = files;
    input_paths.extend(args.coverage.iter().cloned());
    let result = ScanResult {
        report,
        root,
        input_paths,
        source_hashes,
        require_coverage: args.require_coverage,
        fail_on_empty: args.fail_on_empty,
        jobs,
    };
    result.verify_sources()?;
    Ok((result, sources))
}

/// Inputs shared by every per-file scan, read-only across workers.
struct FileContext<'a> {
    root: &'a Path,
    coverage: Option<&'a Coverage>,
    kinds: &'a analysis::KindTable,
    want_lines: bool,
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
        let source = decode_source(&bytes)
            .with_context(ErrorKind::Source, || format!("decoding {}", path.display()))?;
        drop(bytes);
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
        if tree.root_node().has_error() {
            fail!(Syntax, "invalid C# syntax in {relative}");
        }
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
        Ok(FileScan {
            path: path.to_path_buf(),
            hash,
            line_count,
            items,
        })
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
}
