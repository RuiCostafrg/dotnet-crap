use dotnet_crap::{
    cobertura::parse_reports,
    collector::{collect, CollectOptions},
    output::write_report,
    scan, Error, ErrorKind, ScanOptions,
};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, ffi::OsString, fs, path::Path, path::PathBuf, time::Duration};

/// A valid one-method C# source used as the default selected file.
const CLASS: &[u8] = b"class C { int F(int n) { return n; } }\n";

/// Create a temporary root containing the given relative files.
fn root_with(files: &[(&str, &[u8])]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (name, bytes) in files {
        fs::write(dir.path().join(name), bytes).unwrap();
    }
    dir
}

/// Build scan options for a root with default policy.
fn options(root: &Path) -> ScanOptions {
    ScanOptions {
        root: root.to_owned(),
        ..ScanOptions::default()
    }
}

/// Build collection options with an explicit argv and output destination.
fn collect_options(
    root: &Path,
    output: PathBuf,
    command: &[&str],
    expected_reports: usize,
) -> CollectOptions {
    CollectOptions {
        root: root.to_owned(),
        paths: Vec::new(),
        output,
        command: command.iter().map(OsString::from).collect(),
        expected_reports,
        timeout: Duration::from_secs(60),
    }
}

/// Scan that must fail, returning the typed error.
fn scan_error(options: &ScanOptions) -> Error {
    scan(options).expect_err("scan must fail")
}

/// Assert the broad kind and a stable fragment of the outer message.
fn assert_kind(error: &Error, kind: ErrorKind, text: &str) {
    assert_eq!(error.kind(), kind, "wrong kind for {error:#}");
    assert!(
        error.to_string().contains(text),
        "message {error} does not contain {text:?}"
    );
}

/// Native v1 coverage JSON for `C.cs` with the given version, hash and line key.
fn coverage_json(version: u32, sha: &str, line: &str) -> String {
    format!(
        r#"{{"version":{version},"files":{{"C.cs":{{"sha256":"{sha}","lines":{{"{line}":1}}}}}}}}"#
    )
}

/// Compile-time check that the public error is thread-safe and a std error.
fn assert_error_traits<T: std::error::Error + Send + Sync + 'static>() {}

#[test]
/// Negative and non-finite thresholds are rejected as caller input before scanning.
fn invalid_threshold_is_invalid_input() {
    let dir = root_with(&[("C.cs", CLASS)]);
    for threshold in [-1.0, f64::NAN] {
        let opts = ScanOptions {
            threshold,
            ..options(dir.path())
        };
        let error = scan_error(&opts);
        assert_kind(
            &error,
            ErrorKind::InvalidInput,
            "threshold must be finite and nonnegative",
        );
    }
}

#[test]
/// A root that does not exist is invalid input, not an I/O failure.
fn nonexistent_root_is_invalid_input() {
    let dir = tempfile::tempdir().unwrap();
    let error = scan_error(&options(&dir.path().join("missing")));
    assert_kind(&error, ErrorKind::InvalidInput, "invalid root");
}

#[test]
/// An absolute selection outside the root is rejected even when it names a real sibling file.
fn input_path_escaping_root_is_invalid_input() {
    let base = tempfile::tempdir().unwrap();
    let root = base.path().join("root");
    let sibling = base.path().join("sibling");
    fs::create_dir(&root).unwrap();
    fs::create_dir(&sibling).unwrap();
    fs::write(sibling.join("S.cs"), CLASS).unwrap();
    let opts = ScanOptions {
        paths: vec![sibling.join("S.cs")],
        ..options(&root)
    };
    let error = scan_error(&opts);
    assert_kind(&error, ErrorKind::InvalidInput, "input path escapes root");
}

#[test]
/// Collection requires a positive expected report count before any process runs.
fn zero_expected_reports_is_invalid_input() {
    let dir = root_with(&[("C.cs", CLASS)]);
    let opts = collect_options(
        dir.path(),
        dir.path().join("native.json"),
        &["never-run", "{results}"],
        0,
    );
    let error = collect(&opts).expect_err("zero reports must fail");
    assert_kind(
        &error,
        ErrorKind::InvalidInput,
        "expected report count must be positive",
    );
}

#[test]
/// A test command without a standalone `{results}` argument is rejected before execution.
fn collect_without_results_argument_is_invalid_input() {
    let dir = root_with(&[("C.cs", CLASS)]);
    let opts = collect_options(
        dir.path(),
        dir.path().join("native.json"),
        &["never-run", "--no-results"],
        1,
    );
    let error = collect(&opts).expect_err("missing {results} must fail");
    assert_kind(
        &error,
        ErrorKind::InvalidInput,
        "requires exactly one standalone {results} argument",
    );
}

#[test]
/// A selected file that parses with errors is a syntax error naming the file.
fn invalid_csharp_is_syntax_error() {
    let dir = root_with(&[("C.cs", b"class {")]);
    let error = scan_error(&options(dir.path()));
    assert_kind(&error, ErrorKind::Syntax, "invalid C# syntax in C.cs");
}

#[test]
/// Malformed UTF-8 bytes are rejected rather than replaced, as a source error.
fn invalid_utf8_source_is_source_error() {
    let dir = root_with(&[("Bad.cs", b"class C { \xff }")]);
    let error = scan_error(&options(dir.path()));
    assert_kind(&error, ErrorKind::Source, "decoding ");
}

#[test]
/// Coverage version 2 is unsupported by the native v1 reader.
fn coverage_version_two_is_coverage_error() {
    let dir = root_with(&[("C.cs", CLASS)]);
    let sha = hex::encode(Sha256::digest(CLASS));
    let cov = dir.path().join("cov.json");
    fs::write(&cov, coverage_json(2, &sha, "1")).unwrap();
    let opts = ScanOptions {
        coverage: Some(cov),
        ..options(dir.path())
    };
    let error = scan_error(&opts);
    assert_kind(&error, ErrorKind::Coverage, "unsupported coverage version");
}

#[test]
/// Coverage that is not JSON is a coverage error with the parser failure as its cause.
fn invalid_coverage_json_is_coverage_error() {
    let dir = root_with(&[("C.cs", CLASS)]);
    let cov = dir.path().join("cov.json");
    fs::write(&cov, "{").unwrap();
    let opts = ScanOptions {
        coverage: Some(cov),
        ..options(dir.path())
    };
    let error = scan_error(&opts);
    assert_kind(&error, ErrorKind::Coverage, "invalid coverage JSON");
}

#[test]
/// A well-formed but wrong SHA-256 binds coverage to other bytes and is stale.
fn stale_coverage_hash_is_coverage_error() {
    let dir = root_with(&[("C.cs", CLASS)]);
    let cov = dir.path().join("cov.json");
    fs::write(&cov, coverage_json(1, &"0".repeat(64), "1")).unwrap();
    let opts = ScanOptions {
        coverage: Some(cov),
        ..options(dir.path())
    };
    let error = scan_error(&opts);
    assert_kind(&error, ErrorKind::Coverage, "stale coverage for C.cs");
}

#[test]
/// A line number beyond the source's last line is rejected even when the hash matches.
fn coverage_line_outside_source_is_coverage_error() {
    let dir = root_with(&[("C.cs", CLASS)]);
    let sha = hex::encode(Sha256::digest(CLASS));
    let cov = dir.path().join("cov.json");
    fs::write(&cov, coverage_json(1, &sha, "99")).unwrap();
    let opts = ScanOptions {
        coverage: Some(cov),
        ..options(dir.path())
    };
    let error = scan_error(&opts);
    assert_kind(&error, ErrorKind::Coverage, "coverage line outside C.cs");
}

#[test]
/// A coverage path that does not exist is an I/O error with context.
fn missing_coverage_file_is_io_error() {
    let dir = root_with(&[("C.cs", CLASS)]);
    let opts = ScanOptions {
        coverage: Some(dir.path().join("absent.json")),
        ..options(dir.path())
    };
    let error = scan_error(&opts);
    assert_kind(&error, ErrorKind::Io, "reading coverage");
}

#[test]
/// Writing a report over a C# name is rejected even though the destination is new-looking.
fn report_to_csharp_path_is_output_error() {
    let dir = root_with(&[("C.cs", CLASS)]);
    let result = scan(&options(dir.path())).unwrap();
    let error = write_report(&result, "report", &dir.path().join("Other.cs"))
        .expect_err("C# destination must fail");
    assert_kind(&error, ErrorKind::Output, "cannot overwrite C# source");
}

#[test]
/// Writing a report over the coverage input is rejected as an alias of a scan input.
fn report_over_coverage_input_is_output_error() {
    let dir = root_with(&[("C.cs", CLASS)]);
    let sha = hex::encode(Sha256::digest(CLASS));
    let cov = dir.path().join("cov.json");
    fs::write(&cov, coverage_json(1, &sha, "1")).unwrap();
    let opts = ScanOptions {
        coverage: Some(cov.clone()),
        ..options(dir.path())
    };
    let result = scan(&opts).unwrap();
    let error = write_report(&result, "report", &cov).expect_err("coverage input must fail");
    assert_kind(&error, ErrorKind::Output, "report output aliases an input");
}

#[test]
/// Collection refuses to replace an existing evidence destination.
fn existing_collection_output_is_output_error() {
    let dir = root_with(&[("C.cs", CLASS)]);
    let output = dir.path().join("native.json");
    fs::write(&output, "{}").unwrap();
    let opts = collect_options(dir.path(), output, &["never-run", "{results}"], 1);
    let error = collect(&opts).expect_err("existing output must fail");
    assert_kind(&error, ErrorKind::Output, "must not already exist");
}

#[cfg(unix)]
#[test]
/// A trusted command that exits non-zero publishes nothing and reports a collection error.
fn failing_test_command_is_collection_error() {
    let dir = root_with(&[("C.cs", CLASS)]);
    let out = tempfile::tempdir().unwrap();
    let opts = collect_options(
        dir.path(),
        out.path().join("native.json"),
        &["/bin/sh", "-c", "exit 3", "{results}"],
        1,
    );
    let error = collect(&opts).expect_err("failing command must fail");
    assert_kind(&error, ErrorKind::Collection, "trusted test command failed");
    assert!(!out.path().join("native.json").exists());
}

#[test]
/// Importing zero Cobertura reports is a Cobertura error, not an empty success.
fn empty_cobertura_list_is_cobertura_error() {
    let dir = root_with(&[("C.cs", CLASS)]);
    let error = parse_reports(dir.path(), &BTreeMap::new(), &[]).unwrap_err();
    assert_kind(
        &error,
        ErrorKind::Cobertura,
        "no Cobertura reports to import",
    );
}

#[test]
/// A report with a DOCTYPE is rejected by the strict importer.
fn cobertura_doctype_is_cobertura_error() {
    let dir = root_with(&[("C.cs", CLASS)]);
    let report = dir.path().join("report.xml");
    fs::write(&report, "<!DOCTYPE coverage><coverage/>").unwrap();
    let error = parse_reports(dir.path(), &BTreeMap::new(), &[report]).unwrap_err();
    assert_kind(&error, ErrorKind::Cobertura, "invalid Cobertura report ");
    assert!(format!("{error:#}").contains("forbidden DOCTYPE declaration"));
}

#[test]
/// A malformed XML report keeps its path in the outer message and the parser cause in the chain.
fn malformed_cobertura_is_cobertura_error_with_context() {
    let dir = root_with(&[("C.cs", CLASS)]);
    let report = dir.path().join("report.xml");
    fs::write(&report, "<coverage>").unwrap();
    let error = parse_reports(dir.path(), &BTreeMap::new(), &[report]).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Cobertura);
    assert!(format!("{error:#}").starts_with("invalid Cobertura report "));
}

#[test]
/// The library's alternate rendering equals the CLI's anyhow rendering of the same failure.
fn anyhow_rendering_matches_cli_chain() {
    let dir = root_with(&[("C.cs", CLASS)]);
    let opts = ScanOptions {
        coverage: Some(dir.path().join("absent.json")),
        ..options(dir.path())
    };
    let direct = scan_error(&opts);
    let converted = anyhow::Error::from(scan_error(&opts));
    assert_eq!(format!("{direct:#}"), format!("{converted:#}"));
    assert!(format!("{direct:#}").contains(": "));
}

#[test]
/// `Error` can cross threads and participates in the standard error ecosystem.
fn error_is_send_sync_static_std_error() {
    assert_error_traits::<Error>();
}
