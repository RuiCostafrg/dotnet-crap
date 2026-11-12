use dotnet_crap::{scan, ReportFormat, ScanOptions};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

/// Build scan options for an isolated fixture while retaining public defaults.
fn options(root: &Path) -> ScanOptions {
    ScanOptions {
        root: root.to_path_buf(),
        ..ScanOptions::default()
    }
}

/// Invoke the real CLI for the same fixture without changing the test process directory.
fn cli(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_dotnet-crap"))
        .arg("--root")
        .arg(root)
        .args(args)
        .output()
        .unwrap()
}

/// Compare all rendered bytes and gate exits through the library and CLI boundaries.
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

/// Require identical diagnostics, empty stdout and exit 2 for invalid inputs in every format.
fn assert_invalid(opts: &ScanOptions, cli_args: &[&str], message: &str) {
    let error = scan(opts).expect_err("invalid input must fail");
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

#[test]
fn serialization_and_captured_policy_keep_the_existing_contract() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.cs"), "class C { void F() {} }\n").unwrap();
    let mut opts = options(dir.path());
    opts.threshold = 1.0;
    opts.require_coverage = true;
    let result = scan(&opts).unwrap();
    opts.threshold = 100.0;
    opts.require_coverage = false;
    assert!(!scan(&opts).unwrap().report().failed);
    assert_eq!(
        result.render(ReportFormat::Json).unwrap(),
        concat!(
            "{\n  \"threshold\": 1.0,\n  \"files_scanned\": 1,\n",
            "  \"coverage_complete\": false,\n  \"functions\": [\n    {\n",
            "      \"file\": \"a.cs\",\n      \"name\": \"F\",\n",
            "      \"kind\": \"method_declaration\",\n      \"line\": 1,\n",
            "      \"column\": 11,\n      \"end_line\": 1,\n",
            "      \"complexity\": 1,\n      \"coverage\": null,\n",
            "      \"crap\": 2.0\n    }\n  ],\n  \"failed\": true\n}\n"
        )
    );
    assert!(result
        .render(ReportFormat::Text)
        .unwrap()
        .starts_with("Coverage missing"));
    let sarif: Value = serde_json::from_str(&result.render(ReportFormat::Sarif).unwrap()).unwrap();
    let run = &sarif["runs"][0];
    assert_eq!(
        run["results"][0]["partialFingerprints"]["dotnetCrap/v1"],
        "d0e351aa246e10d614c46e9fd304b17caf372430147b6807bbbe8e347e4c591e"
    );
    assert_eq!(run["results"][1]["ruleId"], "DNCRAP002");
    let ids: Vec<_> = run["tool"]["driver"]["rules"]
        .as_array()
        .unwrap()
        .iter()
        .map(|rule| rule["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["DNCRAP001", "DNCRAP002", "DNCRAP003"]);
}

#[test]
fn coverage_inputs_and_sparse_nested_ownership_are_retained() {
    let dir = tempfile::tempdir().unwrap();
    let source = "class C {\n void F() {\n  void A() {}\n  System.Console.WriteLine(1);\n  void B() {}\n  System.Console.WriteLine(2);\n }\n}\n";
    fs::write(dir.path().join("a.cs"), source).unwrap();
    let coverage = dir.path().join("coverage.json");
    let hash = hex::encode(Sha256::digest(source));
    fs::write(&coverage, format!(r#"{{"version":1,"files":{{"a.cs":{{"sha256":"{hash}","lines":{{"3":1,"4":0,"5":1,"6":1}}}}}}}}"#)).unwrap();
    let mut opts = options(dir.path());
    opts.coverage = Some(coverage.clone());
    opts.require_coverage = true;
    let result = scan(&opts).unwrap();
    assert_eq!(
        result.input_paths(),
        &[
            dir.path().join("a.cs").canonicalize().unwrap(),
            coverage.clone()
        ]
    );
    assert_eq!(result.report().functions[0].coverage, Some(0.5));
    assert_eq!(result.report().functions[0].crap, 1.125);
    fs::write(
        &coverage,
        format!(r#"{{"version":1,"files":{{"a.cs":{{"sha256":"{hash}","lines":{{}}}}}}}}"#),
    )
    .unwrap();
    let result = scan(&opts).unwrap();
    assert!(!result.report().coverage_complete);
    assert!(result.report().failed);
    assert!(result
        .report()
        .functions
        .iter()
        .all(|item| item.coverage.is_none()));
}

#[test]
fn library_selection_deduplicates_and_rejects_root_escape() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("nested")).unwrap();
    for name in ["nested/a.cs", "nested/skip.g.cs", "z.cs"] {
        fs::write(dir.path().join(name), "class C { void F() {} }").unwrap();
    }
    let mut opts = options(dir.path());
    opts.paths = vec!["nested".into(), dir.path().join("nested/a.cs")];
    let result = scan(&opts).unwrap();
    assert_eq!(result.report().files_scanned, 1);
    assert_eq!(result.report().functions[0].file, "nested/a.cs");
    assert_equivalent(dir.path(), &opts, &["nested", "nested/a.cs"]);
    opts.paths = vec!["..".into()];
    assert_invalid(&opts, &[".."], "input path escapes root");
    opts.paths.clear();
    for (threshold, arg) in [
        (f64::INFINITY, "--threshold=inf"),
        (f64::NEG_INFINITY, "--threshold=-inf"),
    ] {
        opts.threshold = threshold;
        assert_invalid(&opts, &[arg], "threshold must be finite and nonnegative");
    }
}

#[test]
fn cli_preserves_coverage_and_unselected_source_aliases() {
    let dir = tempfile::tempdir().unwrap();
    let source = "class C { void F() {} }\n";
    fs::write(dir.path().join("a.cs"), source).unwrap();
    let coverage = dir.path().join("coverage.json");
    let hash = hex::encode(Sha256::digest(source));
    let evidence =
        format!(r#"{{"version":1,"files":{{"a.cs":{{"sha256":"{hash}","lines":{{"1":1}}}}}}}}"#);
    fs::write(&coverage, &evidence).unwrap();
    let alias = dir.path().join("alias.json");
    fs::hard_link(&coverage, &alias).unwrap();
    for output in [&coverage, &alias] {
        let output = cli(
            dir.path(),
            &[
                "--coverage",
                coverage.to_str().unwrap(),
                "--output",
                output.to_str().unwrap(),
            ],
        );
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert_eq!(fs::read_to_string(&coverage).unwrap(), evidence);
    }
    let ignored = dir.path().join("ignored.g.cs");
    fs::write(&ignored, source).unwrap();
    let alias = dir.path().join("ignored.json");
    fs::hard_link(&ignored, &alias).unwrap();
    let output = cli(dir.path(), &["--output", alias.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(fs::read_to_string(&ignored).unwrap(), source);
}

#[cfg(unix)]
#[test]
fn cli_preserves_symlink_output_targets() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("target.json");
    let output = dir.path().join("link.json");
    fs::write(&target, "original").unwrap();
    std::os::unix::fs::symlink(&target, &output).unwrap();
    let result = cli(dir.path(), &["--output", output.to_str().unwrap()]);
    assert_eq!(result.status.code(), Some(2));
    assert!(String::from_utf8(result.stderr)
        .unwrap()
        .contains("symbolic link"));
    assert_eq!(fs::read_to_string(&target).unwrap(), "original");
    assert!(output.is_symlink());
}
