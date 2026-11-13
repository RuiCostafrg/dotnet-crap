//! Execute an explicitly trusted test command and publish fresh native evidence.
use crate::error::{fail, ErrorKind, OptionExt, Result, ResultExt};
use crate::{
    output::{destination_parent, is_csharp_path},
    scan, ScanOptions,
};
use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, ExitStatus},
    time::{Duration, Instant},
};
use walkdir::WalkDir;

/// Collection policy. The caller is responsible for trusting the entire command
/// and pinning its Coverlet dependency. No shell is inserted by this API.
#[derive(Debug, Clone)]
pub struct CollectOptions {
    /// Root containing selected sources. Collection does not apply scoring gates.
    pub root: PathBuf,
    /// Selected files/directories, relative to root; empty selects the root.
    pub paths: Vec<PathBuf>,
    /// New native v1 evidence destination. Existing destinations are rejected.
    pub output: PathBuf,
    /// Executable and argv, containing exactly one standalone `{results}` token.
    /// Use it as the value of `dotnet test --results-directory` and explicitly
    /// select XPlat Code Coverage with Cobertura format.
    pub command: Vec<OsString>,
    /// Exact expected report-file count (not project identity); must be positive.
    pub expected_reports: usize,
    /// Deadline for the immediate trusted test process; must be nonzero.
    pub timeout: Duration,
}

/// Snapshot selected bytes, run trusted tests in the canonical source root, then
/// validate all fresh Cobertura reports before atomically creating `output`.
/// Errors leave no new coverage artifact. This operation never runs during scan.
pub fn collect(options: &CollectOptions) -> Result<()> {
    validate_options(options)?;
    validate_destination(&options.output)?;
    let selection = ScanOptions {
        root: options.root.clone(),
        paths: options.paths.clone(),
        ..Default::default()
    };
    let snapshot = scan(&selection)?;
    let mut sources = BTreeMap::new();
    let mut hashes = BTreeMap::new();
    for (path, hash) in &snapshot.source_hashes {
        let relative = path
            .strip_prefix(snapshot.root())
            .kind(ErrorKind::InvalidInput)?
            .to_str()
            .context(ErrorKind::Source, "non-UTF-8 source path")?
            .replace('\\', "/");
        if relative.contains(':') || relative.contains('\\') {
            fail!(Source, "unsupported source path for native coverage");
        }
        let bytes = fs::read(path).kind(ErrorKind::Io)?;
        sources.insert(relative.clone(), crate::decode_source(&bytes)?);
        hashes.insert(relative, hash.clone());
    }
    if sources.is_empty() {
        fail!(InvalidInput, "collection requires selected sources");
    }
    snapshot.verify_sources()?;
    let mut results_builder = tempfile::Builder::new();
    results_builder.prefix("dotnet-crap-coverlet-");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        results_builder.permissions(fs::Permissions::from_mode(0o700));
    }
    let results = results_builder.tempdir().kind(ErrorKind::Io)?;
    let mut command = Command::new(&options.command[0]);
    command
        .args(results_args(&options.command, results.path()))
        .current_dir(snapshot.root());
    let status = run_command(&mut command, options.timeout)?;
    if !status.success() {
        fail!(Collection, "trusted test command failed: {status}");
    }
    snapshot.verify_sources()?;
    let reports = find_reports(results.path(), options.expected_reports)?;
    let lines = crate::cobertura::parse_reports(snapshot.root(), &sources, &reports)?;
    let evidence = build_evidence(lines, &hashes);
    let parent = destination_parent(&options.output);
    let mut stage = tempfile::NamedTempFile::new_in(parent).kind(ErrorKind::Output)?;
    write_evidence(&mut stage, &evidence)?;
    stage.as_file().sync_all().kind(ErrorKind::Output)?;
    // Exercise the native validator and scoring on exactly the staged evidence.
    let mut verify = selection;
    verify.coverage = Some(stage.path().to_owned());
    scan(&verify)?;
    snapshot.verify_sources()?;
    validate_destination(&options.output)?;
    stage
        .persist_noclobber(&options.output)
        .context(ErrorKind::Output, "publishing native coverage")?;
    Ok(())
}

/// Write pretty evidence JSON and a trailing newline through a buffer, since
/// unbuffered pretty JSON issues one `write(2)` per token. The explicit flush
/// reports a deferred write failure as an output error instead of losing it
/// when the buffer is dropped.
fn write_evidence(sink: impl Write, evidence: &crate::Coverage) -> Result<()> {
    let mut writer = std::io::BufWriter::new(sink);
    serde_json::to_writer_pretty(&mut writer, evidence).kind(ErrorKind::Output)?;
    writer.write_all(b"\n").kind(ErrorKind::Output)?;
    writer.flush().kind(ErrorKind::Output)
}

/// Enforce collection policy before any filesystem or process work.
fn validate_options(options: &CollectOptions) -> Result<()> {
    if options.expected_reports == 0 {
        fail!(InvalidInput, "expected report count must be positive");
    }
    if options.timeout.is_zero() {
        fail!(InvalidInput, "test timeout must be positive");
    }
    if options.command.len() < 2
        || options.command[1..]
            .iter()
            .filter(|a| *a == "{results}")
            .count()
            != 1
    {
        fail!(
            InvalidInput,
            "test command requires exactly one standalone {{results}} argument"
        );
    }
    Ok(())
}

/// Substitute the single standalone `{results}` argument in the trusted command.
fn results_args<'a>(command: &'a [OsString], results: &'a Path) -> Vec<&'a OsStr> {
    command[1..]
        .iter()
        .map(|arg| {
            if arg == "{results}" {
                results.as_os_str()
            } else {
                arg.as_os_str()
            }
        })
        .collect()
}

/// Find and sort fresh Cobertura reports, rejecting links and count mismatches.
fn find_reports(results: &Path, expected: usize) -> Result<Vec<PathBuf>> {
    let mut reports = Vec::new();
    for entry in WalkDir::new(results).follow_links(false) {
        let entry = entry.kind(ErrorKind::Io)?;
        if entry.file_type().is_symlink() {
            fail!(
                Collection,
                "collector results cannot contain symbolic links"
            );
        }
        if entry.file_name() == "coverage.cobertura.xml" && entry.file_type().is_file() {
            reports.push(entry.into_path());
        }
    }
    reports.sort();
    if reports.len() != expected {
        fail!(
            Collection,
            "expected {} fresh coverage.cobertura.xml reports, found {}",
            expected,
            reports.len()
        );
    }
    Ok(reports)
}

/// Convert parsed line evidence and source hashes into native coverage.
fn build_evidence(
    lines: crate::cobertura::LineEvidence,
    hashes: &BTreeMap<String, String>,
) -> crate::Coverage {
    let files = lines
        .into_iter()
        .map(|(name, points)| {
            let sha256 = hashes[&name].clone();
            (
                name,
                crate::CoverageFile {
                    sha256,
                    lines: points
                        .into_iter()
                        .map(|(line, hits)| (line.to_string(), hits))
                        .collect(),
                },
            )
        })
        .collect();
    crate::Coverage { version: 1, files }
}

/// Refuse existing destinations (including hardlinks/symlinks) and C# names.
fn validate_destination(path: &std::path::Path) -> Result<()> {
    if is_csharp_path(path) {
        fail!(Output, "coverage output cannot overwrite C# source");
    }
    let parent = destination_parent(path);
    if !parent.is_dir() {
        fail!(
            Output,
            "coverage destination parent must be an existing directory"
        );
    }
    match fs::symlink_metadata(path) {
        Ok(_) => fail!(Output, "coverage destination must not already exist"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).kind(ErrorKind::Output),
    }
}

/// Bound the immediate child, reaping it on timeout and on wait failures.
fn run_command(command: &mut Command, timeout: Duration) -> Result<ExitStatus> {
    let mut child = command
        .spawn()
        .context(ErrorKind::Collection, "starting trusted test command")?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return observed_completion(status, start.elapsed(), timeout),
            Ok(None) => {}
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error)
                    .context(ErrorKind::Collection, "waiting for trusted test command");
            }
        }
        if start.elapsed() >= timeout {
            let _ = child.kill();
            child
                .wait()
                .context(ErrorKind::Collection, "reaping timed-out test command")?;
            fail!(Collection, "trusted test command timed out");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Apply the completion-observation deadline independently of process scheduling.
fn observed_completion(
    status: ExitStatus,
    elapsed: Duration,
    timeout: Duration,
) -> Result<ExitStatus> {
    if elapsed >= timeout {
        fail!(Collection, "trusted test command timed out");
    }
    Ok(status)
}

#[cfg(test)]
mod deadline_tests {
    /// Even a successful child cannot publish when completion is observed at/after the deadline.
    #[test]
    fn completed_status_must_be_observed_strictly_before_deadline() {
        let status = Command::new(std::env::current_exe().unwrap())
            .arg("--list")
            .output()
            .unwrap()
            .status;
        assert!(status.success());
        let deadline = Duration::from_millis(10);
        for elapsed in [Duration::ZERO, Duration::from_millis(9)] {
            assert!(observed_completion(status, elapsed, deadline)
                .unwrap()
                .success());
        }
        for elapsed in [Duration::from_millis(10), Duration::from_millis(11)] {
            let error = observed_completion(status, elapsed, deadline).unwrap_err();
            assert!(error.to_string().contains("timed out"));
        }
    }
    /// Valid baseline options; each test changes one field.
    fn options() -> CollectOptions {
        CollectOptions {
            root: PathBuf::from("."),
            paths: Vec::new(),
            output: PathBuf::from("coverage.json"),
            command: vec![OsString::from("dotnet"), OsString::from("{results}")],
            expected_reports: 1,
            timeout: Duration::from_secs(1),
        }
    }

    #[test]
    /// A zero report count is rejected before any process work.
    fn validate_options_rejects_zero_expected_reports() {
        let mut options = options();
        options.expected_reports = 0;
        let error = validate_options(&options).unwrap_err();
        assert_eq!(error.to_string(), "expected report count must be positive");
    }

    #[test]
    /// A zero deadline is rejected.
    fn validate_options_rejects_zero_timeout() {
        let mut options = options();
        options.timeout = Duration::ZERO;
        let error = validate_options(&options).unwrap_err();
        assert_eq!(error.to_string(), "test timeout must be positive");
    }

    #[test]
    /// The minimal valid command is accepted.
    fn validate_options_accepts_one_standalone_results_argument() {
        // Length two also proves the minimum-length check is not too strict.
        assert!(validate_options(&options()).is_ok());
    }

    #[test]
    /// Missing, duplicate and executable-position `{results}` tokens are rejected.
    fn validate_options_rejects_wrong_results_argument_forms() {
        for command in [
            vec!["dotnet"],
            vec!["dotnet", "test"],
            vec!["dotnet", "{results}", "{results}"],
            vec!["{results}", "test"],
        ] {
            let mut options = options();
            options.command = command.into_iter().map(OsString::from).collect();
            let error = validate_options(&options).unwrap_err();
            assert_eq!(
                error.to_string(),
                "test command requires exactly one standalone {results} argument"
            );
        }
    }

    #[test]
    /// Only the exact standalone token is substituted; the executable is excluded.
    fn results_args_replaces_only_standalone_exact_tokens() {
        let command: Vec<OsString> = [
            "dotnet",
            "test",
            "--x={results}",
            "{results}",
            "{results}x",
            "plain",
        ]
        .into_iter()
        .map(OsString::from)
        .collect();
        let results = Path::new("/tmp/dotnet-crap-results");
        assert_eq!(
            results_args(&command, results),
            vec![
                OsStr::new("test"),
                OsStr::new("--x={results}"),
                results.as_os_str(),
                OsStr::new("{results}x"),
                OsStr::new("plain"),
            ]
        );
    }

    #[test]
    /// An empty results directory reports the expected and found counts.
    fn find_reports_reports_count_mismatch() {
        let results = tempfile::tempdir().unwrap();
        let error = find_reports(results.path(), 1).unwrap_err();
        assert!(error.to_string().contains("found 0"));
        assert!(error.to_string().contains("expected 1 fresh"));
    }

    #[test]
    /// The count check compares exactly rather than requiring a report.
    fn find_reports_accepts_empty_results_when_zero_expected() {
        let results = tempfile::tempdir().unwrap();
        assert!(find_reports(results.path(), 0).unwrap().is_empty());
    }

    #[test]
    /// Nested reports are counted and returned in sorted order.
    fn find_reports_sorts_and_counts_reports() {
        let results = tempfile::tempdir().unwrap();
        let nested = results.path().join("a");
        std::fs::create_dir(&nested).unwrap();
        std::fs::write(
            results.path().join("coverage.cobertura.xml"),
            b"<coverage/>",
        )
        .unwrap();
        std::fs::write(nested.join("coverage.cobertura.xml"), b"<coverage/>").unwrap();
        assert_eq!(
            find_reports(results.path(), 2).unwrap(),
            vec![
                nested.join("coverage.cobertura.xml"),
                results.path().join("coverage.cobertura.xml"),
            ]
        );
    }

    /// Only regular files named `coverage.cobertura.xml` count; other files and
    /// a directory with the report name are ignored rather than miscounted.
    #[test]
    fn find_reports_counts_only_regular_report_files() {
        let results = tempfile::tempdir().unwrap();
        let report = results.path().join("coverage.cobertura.xml");
        std::fs::write(&report, b"<coverage/>").unwrap();
        std::fs::write(results.path().join("other.xml"), b"<coverage/>").unwrap();
        std::fs::create_dir_all(results.path().join("d").join("coverage.cobertura.xml")).unwrap();
        assert_eq!(find_reports(results.path(), 1).unwrap(), vec![report]);
    }

    #[cfg(unix)]
    #[test]
    /// Links in the results directory are rejected.
    fn find_reports_rejects_symbolic_links() {
        let results = tempfile::tempdir().unwrap();
        let target = results.path().join("coverage.cobertura.xml");
        std::fs::write(&target, b"<coverage/>").unwrap();
        std::os::unix::fs::symlink(&target, results.path().join("link.xml")).unwrap();
        let error = find_reports(results.path(), 1).unwrap_err();
        assert_eq!(
            error.to_string(),
            "collector results cannot contain symbolic links"
        );
    }

    /// Sink that accepts nothing, so a buffered write only fails at flush.
    struct FailingSink;
    impl Write for FailingSink {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("sink refused"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// One-file evidence for the writer tests.
    fn sample_evidence() -> crate::Coverage {
        let mut lines = crate::cobertura::LineEvidence::new();
        lines.insert("src/a.cs".to_owned(), BTreeMap::from([(5u32, 2u64)]));
        let hashes = BTreeMap::from([("src/a.cs".to_owned(), "abc123".to_owned())]);
        build_evidence(lines, &hashes)
    }

    #[test]
    /// Evidence goes through the buffer verbatim, with a trailing newline.
    fn write_evidence_matches_unbuffered_pretty_json() {
        let evidence = sample_evidence();
        let mut out = Vec::new();
        write_evidence(&mut out, &evidence).unwrap();
        let mut expected = serde_json::to_vec_pretty(&evidence).unwrap();
        expected.push(b'\n');
        assert_eq!(out, expected);
    }

    #[test]
    /// A small document stays in the buffer, so only the flush can report the failure.
    fn write_evidence_reports_a_flush_failure_as_an_output_error() {
        let error = write_evidence(FailingSink, &sample_evidence()).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Output);
    }

    #[test]
    /// Native evidence keeps hit counts, decimal line keys and the snapshot hash.
    fn build_evidence_stringifies_lines_and_copies_hashes() {
        let mut lines = crate::cobertura::LineEvidence::new();
        let mut points = BTreeMap::new();
        points.insert(5u32, 2u64);
        points.insert(12u32, 0u64);
        lines.insert("src/a.cs".to_owned(), points);
        let hashes = BTreeMap::from([("src/a.cs".to_owned(), "abc123".to_owned())]);
        let evidence = build_evidence(lines, &hashes);
        assert_eq!(evidence.version, 1);
        let file = &evidence.files["src/a.cs"];
        assert_eq!(file.sha256, "abc123");
        assert_eq!(file.lines.len(), 2);
        assert_eq!(file.lines.get("5"), Some(&2));
        assert_eq!(file.lines.get("12"), Some(&0));
    }

    use super::*;
}
