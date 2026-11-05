use dotnet_crap::{scan, ReportFormat, ScanOptions};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

fn options(root: &Path) -> ScanOptions {
    ScanOptions {
        root: root.to_path_buf(),
        ..ScanOptions::default()
    }
}

fn cli(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_dotnet-crap"))
        .arg("--root")
        .arg(root)
        .args(args)
        .output()
        .unwrap()
}

fn assert_equivalent(root: &Path, opts: &ScanOptions, cli_args: &[&str]) {
    let result = scan(opts).unwrap();
    for (format, name) in [
        (ReportFormat::Text, "text"),
        (ReportFormat::Json, "json"),
        (ReportFormat::Sarif, "sarif"),
    ] {
        let library = result.render(format).unwrap();
        let output = cli(root, &[cli_args, &["--format", name]].concat());
        assert_eq!(
            output.status.code(),
            Some(if result.report().failed { 1 } else { 0 })
        );
        let cli_text = String::from_utf8(output.stdout).unwrap();
        if format == ReportFormat::Json {
            let actual: Value = serde_json::from_str(&library).unwrap();
            let expected: Value = serde_json::from_str(&cli_text).unwrap();
            assert_eq!(actual, expected, "JSON values differ");
            assert_eq!(library, cli_text, "JSON field order/bytes changed");
        } else {
            assert_eq!(library, cli_text);
        }
    }
}

fn assert_invalid(opts: &ScanOptions, cli_args: &[&str], message: &str) {
    let error = scan(opts).err().expect("invalid input must fail");
    assert!(format!("{error:#}").contains(message));
    for name in ["text", "json", "sarif"] {
        let output = cli(&opts.root, &[cli_args, &["--format", name]].concat());
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert_eq!(
            String::from_utf8(output.stderr).unwrap(),
            format!(
                "error: {}\n",
                dotnet_crap::terminal_safe(&format!("{error:#}"))
            )
        );
    }
}

#[test]
fn library_defaults_match_documented_cli_defaults() {
    let opts = ScanOptions::default();
    assert_eq!(opts.root, Path::new("."));
    assert_eq!(opts.threshold, 10.0);
    assert!(opts.coverage.is_none());
    assert!(!opts.require_coverage);
    assert!(!opts.fail_on_empty);
    assert!(opts.paths.is_empty());
}

#[test]
fn scan_has_independent_results_and_matches_cli_in_all_formats() {
    let dir = tempfile::tempdir().unwrap();
    let source = "class C { int F(int x) { if (x > 0 && x < 2) return 1; return 0; } }\n";
    fs::write(dir.path().join("a.cs"), source).unwrap();
    let mut opts = options(dir.path());
    opts.threshold = 12.0;
    let result = scan(&opts).unwrap();
    assert_eq!(result.root(), dir.path().canonicalize().unwrap());
    assert_eq!(
        result.input_paths(),
        &[dir.path().join("a.cs").canonicalize().unwrap()]
    );
    assert_eq!(result.report().files_scanned, 1);
    assert_eq!(result.report().functions.len(), 1);
    assert_eq!(result.report().functions[0].name, "F");
    assert_eq!(result.report().functions[0].complexity, 3);
    assert_eq!(result.report().functions[0].crap, 12.0);
    assert!(!result.report().failed, "threshold equality must pass");
    opts.threshold = 11.999;
    assert!(scan(&opts).unwrap().report().failed);
    assert_equivalent(dir.path(), &opts, &["--threshold", "11.999"]);

    opts.threshold = 12.0;
    assert_equivalent(dir.path(), &opts, &["--threshold", "12"]);
    opts.threshold = 0.0;
    assert_equivalent(dir.path(), &opts, &["--threshold", "0"]);
}

#[test]
fn coverage_empty_and_error_policies_match_cli() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.cs"), "class A { int F()=>1; }\n").unwrap();
    let src = fs::read(dir.path().join("a.cs")).unwrap();
    let hash = hex::encode(Sha256::digest(src));
    let coverage = dir.path().join("coverage.json");
    fs::write(
        &coverage,
        format!(r#"{{"version":1,"files":{{"a.cs":{{"sha256":"{hash}","lines":{{"1":1}}}}}}}}"#),
    )
    .unwrap();
    let mut opts = options(dir.path());
    opts.coverage = Some(coverage.clone());
    opts.require_coverage = true;
    assert!(scan(&opts).unwrap().report().coverage_complete);
    assert_equivalent(
        dir.path(),
        &opts,
        &[
            "--coverage",
            coverage.to_str().unwrap(),
            "--require-coverage",
        ],
    );

    let empty = tempfile::tempdir().unwrap();
    let mut empty_opts = options(empty.path());
    assert!(!scan(&empty_opts).unwrap().report().failed);
    assert_equivalent(empty.path(), &empty_opts, &[]);
    empty_opts.fail_on_empty = true;
    assert!(scan(&empty_opts).unwrap().report().failed);
    assert_equivalent(empty.path(), &empty_opts, &["--fail-on-empty"]);

    let mut missing = options(dir.path());
    missing.require_coverage = true;
    let result = scan(&missing).unwrap();
    assert!(result.report().failed);
    assert!(!result.report().coverage_complete);
    assert_equivalent(dir.path(), &missing, &["--require-coverage"]);

    // Malformed syntax/evidence and invalid options or roots are input errors.
    fs::write(dir.path().join("bad.cs"), "class A { int F( }").unwrap();
    assert_invalid(&options(dir.path()), &[], "invalid C# syntax");
    assert_eq!(cli(dir.path(), &[]).status.code(), Some(2));
    fs::write(dir.path().join("bad.cs"), "class A { int F()=>1; }").unwrap();
    fs::write(&coverage, "{ definitely not json").unwrap();
    opts.coverage = Some(coverage);
    assert_invalid(
        &opts,
        &[
            "--coverage",
            opts.coverage.as_ref().unwrap().to_str().unwrap(),
        ],
        "invalid coverage JSON",
    );
    assert_eq!(
        cli(
            dir.path(),
            &[
                "--coverage",
                opts.coverage.as_ref().unwrap().to_str().unwrap()
            ]
        )
        .status
        .code(),
        Some(2)
    );
    let mut invalid = options(dir.path());
    invalid.threshold = f64::NAN;
    assert_invalid(
        &invalid,
        &["--threshold", "NaN"],
        "threshold must be finite and nonnegative",
    );
    assert_eq!(
        cli(dir.path(), &["--threshold", "NaN"]).status.code(),
        Some(2)
    );
    invalid.threshold = -1.0;
    assert_invalid(
        &invalid,
        &["--threshold=-1"],
        "threshold must be finite and nonnegative",
    );
    invalid = options(&dir.path().join("missing"));
    assert_invalid(&invalid, &[], "invalid root");
    assert_eq!(cli(&dir.path().join("missing"), &[]).status.code(), Some(2));
}

#[test]
fn result_detects_source_byte_changes() {
    let dir = tempfile::tempdir().unwrap();
    let source_path = dir.path().join("a.cs");
    fs::write(&source_path, "class A { int F()=>1; }\n").unwrap();
    let result = scan(&options(dir.path())).unwrap();
    result.verify_sources().unwrap();
    fs::write(&source_path, "class A { int F()=>2; }\n").unwrap();
    assert!(result.verify_sources().is_err());
}
