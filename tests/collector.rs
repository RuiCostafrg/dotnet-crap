use dotnet_crap::{
    collector::{collect, CollectOptions},
    scan, ScanOptions,
};
use serde_json::Value;
use std::{ffi::OsString, fs};

fn fixture(script: &str) -> (tempfile::TempDir, CollectOptions) {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("C.cs"),
        "class C {\n int F(int n) {\n if(n > 0) return 1;\n return 0;\n }\n}\n",
    )
    .unwrap();
    let options = CollectOptions {
        scan: ScanOptions {
            root: dir.path().to_owned(),
            ..Default::default()
        },
        output: dir.path().join("native.json"),
        expected_reports: 1,
        command: ["python3", "-c", script, "{results}"]
            .into_iter()
            .map(OsString::from)
            .collect(),
    };
    (dir, options)
}
const REPORT: &str = r#"import pathlib,sys
p=pathlib.Path(sys.argv[1]); assert p.is_dir() and not list(p.iterdir())
(p/'run').mkdir()
(p/'run'/'coverage.cobertura.xml').write_text('<coverage><sources/><packages><package><classes><class filename="C.cs"><lines><line number="3" hits="1"/><line number="4" hits="0"/></lines></class></classes></package></packages></coverage>')
"#;

#[test]
fn fresh_collection_preserves_zero_hits_hashes_and_hand_derived_scores() {
    let (_dir, options) = fixture(REPORT);
    collect(&options).unwrap();
    let native: Value = serde_json::from_slice(&fs::read(&options.output).unwrap()).unwrap();
    assert_eq!(native["version"], 1);
    assert_eq!(
        native["files"]["C.cs"]["lines"],
        serde_json::json!({"3":1,"4":0})
    );
    let source = fs::read(options.scan.root.join("C.cs")).unwrap();
    use sha2::Digest;
    assert_eq!(
        native["files"]["C.cs"]["sha256"],
        hex::encode(sha2::Sha256::digest(source))
    );
    let mut scan_options = options.scan.clone();
    scan_options.coverage = Some(options.output.clone());
    let result = scan(&scan_options).unwrap();
    let f = &result.report().functions[0];
    // One if -> CC 2; one of two supplied lines hit -> 1/2;
    // CRAP = 2 + 2^2 * (1 - 1/2)^3 = 2.5.
    assert_eq!(f.complexity, 2);
    assert_eq!(f.coverage, Some(0.5));
    assert_eq!(f.crap, 2.5);
    assert!(result.report().coverage_complete);
    let original = fs::read(&options.output).unwrap();
    assert!(collect(&options).is_err()); // never overwrite prior evidence
    assert_eq!(fs::read(&options.output).unwrap(), original);
}

#[test]
fn failures_leave_no_evidence_and_private_directories_are_removed() {
    for script in [
        "import sys; sys.exit(3)",
        "pass", // stale reports elsewhere never count
        "import pathlib,sys; pathlib.Path(sys.argv[1],'coverage.cobertura.xml').write_text('<bad')",
        "import pathlib,sys; pathlib.Path('C.cs').write_text('class Changed {}')",
    ] {
        let (_dir, options) = fixture(script);
        fs::write(
            options.scan.root.join("coverage.cobertura.xml"),
            "old output",
        )
        .unwrap();
        assert!(collect(&options).is_err(), "{script}");
        assert!(!options.output.exists());
        assert!(!fs::read_dir(&options.scan.root).unwrap().any(|e| e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".tmp")));
    }
    let (dir, mut options) = fixture(REPORT);
    options.command[2] = OsString::from(format!(
        "{}\npathlib.Path('results-path').write_text(str(p))\nraise SystemExit(1)",
        REPORT
    ));
    assert!(collect(&options).is_err());
    let private = fs::read_to_string(dir.path().join("results-path")).unwrap();
    assert!(!std::path::Path::new(&private).exists());
}

#[test]
fn failed_test_with_valid_report_and_changed_source_with_valid_report_do_not_publish() {
    for suffix in [
        "\nraise SystemExit(1)",
        "\npathlib.Path('C.cs').write_text('class Changed {}')",
        "\npathlib.Path('C.cs').unlink()",
    ] {
        let (_dir, mut options) = fixture(REPORT);
        options.command[2] = format!("{REPORT}{suffix}").into();
        assert!(collect(&options).is_err());
        assert!(!options.output.exists());
    }
}

#[test]
fn invalid_commands_inputs_and_destinations_fail_before_execution() {
    for target in ["C.cs", "generated.CS", "existing", "missing/child.json"] {
        let (_dir, mut options) = fixture("raise Exception('must not execute')");
        fs::write(options.scan.root.join("existing"), "keep").unwrap();
        options.output = options.scan.root.join(target);
        assert!(collect(&options).is_err());
        assert_eq!(
            fs::read_to_string(options.scan.root.join("existing")).unwrap(),
            "keep"
        );
        assert!(fs::read_to_string(options.scan.root.join("C.cs"))
            .unwrap()
            .starts_with("class C"));
    }
    for argv in [
        vec![],
        vec!["python3"],
        vec!["python3", "none"],
        vec!["python3", "{results}", "{results}"],
    ] {
        let (_dir, mut options) = fixture(REPORT);
        options.command = argv.into_iter().map(OsString::from).collect();
        assert!(collect(&options).is_err());
        assert!(!options.output.exists());
    }
    let (_dir, mut options) = fixture(REPORT);
    options.scan.coverage = Some("old.json".into());
    assert!(collect(&options).is_err());
    options.scan.coverage = None;
    options.scan.paths = vec!["absent.cs".into()];
    assert!(collect(&options).is_err());
    options.scan.paths.clear();
    fs::remove_file(options.scan.root.join("C.cs")).unwrap();
    assert!(collect(&options).is_err());
}

#[cfg(unix)]
#[test]
fn rejects_linked_reports_and_existing_source_aliases() {
    use std::os::unix::fs::symlink;
    let (dir, mut options) = fixture("import pathlib,sys; pathlib.Path(sys.argv[1],'coverage.cobertura.xml').symlink_to(pathlib.Path('C.cs').absolute())");
    assert!(collect(&options).is_err());
    assert!(!options.output.exists());
    fs::hard_link(dir.path().join("C.cs"), &options.output).unwrap();
    assert!(collect(&options).is_err());
    fs::remove_file(&options.output).unwrap();
    symlink("not-created", &options.output).unwrap();
    assert!(collect(&options).is_err());
    options.output = dir.path().join("other.json");
    options.command[0] = "no-such-dotnet-crap-test-command".into();
    assert!(collect(&options).is_err());
}

#[test]
fn cli_collect_uses_argv_and_keeps_standard_exit_mapping() {
    let (dir, options) = fixture(REPORT);
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_dotnet-crap"))
        .arg("collect")
        .arg("--root")
        .arg(dir.path())
        .arg("--output")
        .arg(&options.output)
        .args(["--path", "C.cs", "--", "python3", "-c", REPORT, "{results}"])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(options.output.is_file());
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_dotnet-crap"))
        .arg("collect")
        .arg("--root")
        .arg(dir.path())
        .arg("--output")
        .arg(&options.output)
        .args(["--", "python3", "-c", REPORT, "{results}"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("must not already exist"));
}

#[test]
fn incomplete_or_extra_project_reports_are_errors() {
    for expected in [0, 2] {
        let (_dir, mut options) = fixture(REPORT);
        options.expected_reports = expected;
        assert!(collect(&options).is_err());
        assert!(!options.output.exists());
    }
}
