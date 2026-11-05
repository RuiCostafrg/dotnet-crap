use sha2::{Digest, Sha256};
use std::{fs, process::Command};

fn check(file: &str, name: &str, complexity: u64, lines: &[(&str, u64)], expected: f64) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/accuracy");
    let bytes = fs::read(root.join(file)).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let coverage = dir.path().join("coverage.json");
    let points: serde_json::Map<String, serde_json::Value> = lines
        .iter()
        .map(|(l, h)| ((*l).into(), (*h).into()))
        .collect();
    fs::write(&coverage, serde_json::to_vec(&serde_json::json!({"version":1,"files":{file:{"sha256":hex::encode(Sha256::digest(&bytes)),"lines":points}}})).unwrap()).unwrap();
    let run = |threshold: String| {
        Command::new(env!("CARGO_BIN_EXE_dotnet-crap"))
            .args([
                "--root",
                root.to_str().unwrap(),
                "--coverage",
                coverage.to_str().unwrap(),
                "--format",
                "json",
                "--threshold",
                &threshold,
                file,
            ])
            .output()
            .unwrap()
    };
    let output = run(expected.to_string());
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let item = report["functions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["name"] == name)
        .unwrap();
    assert_eq!(item["complexity"], complexity);
    assert_eq!(item["coverage"].as_f64().unwrap(), 0.5);
    assert_eq!(item["crap"].as_f64().unwrap(), expected);
    assert_eq!(run((expected - 0.001).to_string()).status.code(), Some(1));
}
#[test]
fn stacked_switch() {
    check("Switch.cs", "Pick", 3, &[("5", 1), ("6", 0)], 4.125);
}
#[test]
fn branches() {
    check("Branches.cs", "Run", 5, &[("4", 1), ("5", 0)], 8.125);
}
#[test]
fn nested_ownership() {
    check(
        "Nested.cs",
        "Outer",
        2,
        &[("3", 1), ("4", 1), ("5", 0)],
        2.5,
    );
}

#[test]
fn encoding_paths_and_terminal_controls() {
    let dir = tempfile::tempdir().unwrap();
    let source = "class C { int M() => 1; }";
    let mut utf16 = vec![0xff, 0xfe];
    for unit in source.encode_utf16() {
        utf16.extend(unit.to_le_bytes());
    }
    fs::write(dir.path().join("upper.CS"), &utf16).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_dotnet-crap"))
        .args(["--root", dir.path().to_str().unwrap(), "--format", "json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["files_scanned"], 1);
    let coverage = dir.path().join("coverage.json");
    fs::write(&coverage, serde_json::json!({"version":1,"files":{"./upper.CS":{"sha256":hex::encode(Sha256::digest(&utf16)),"lines":{}}}}).to_string()).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_dotnet-crap"))
        .args([
            "--root",
            dir.path().to_str().unwrap(),
            "--coverage",
            coverage.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    #[cfg(unix)]
    {
        fs::write(dir.path().join("escape\u{1b}.cs"), source).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_dotnet-crap"))
            .args(["--root", dir.path().to_str().unwrap()])
            .output()
            .unwrap();
        assert!(!output.stdout.contains(&0x1b));
    }
}

#[test]
fn execute_accuracy_fixtures_with_sdk() {
    if std::env::var_os("DOTNET_CRAP_E2E").is_none() {
        return;
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/accuracy");
    let files = ["Switch.cs", "Branches.cs", "Nested.cs"];
    let snapshot: Vec<_> = files
        .iter()
        .map(|file| fs::read(root.join(file)).unwrap())
        .collect();
    for full in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let hits = dir.path().join("hits.json");
        let mut command = Command::new("dotnet");
        command
            .arg("run")
            .arg("--project")
            .arg(root.join("Accuracy.csproj"))
            .args(["--", "--coverage-output"])
            .arg(&hits);
        if full {
            command.arg("--full");
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("fixture execution passed"));
        let executed: serde_json::Value =
            serde_json::from_slice(&fs::read(&hits).unwrap()).unwrap();
        let mut native_files = serde_json::Map::new();
        for (file, original) in files.iter().zip(&snapshot) {
            assert_eq!(
                fs::read(root.join(file)).unwrap(),
                *original,
                "fixture source changed during execution"
            );
            native_files.insert((*file).into(), serde_json::json!({"sha256":hex::encode(Sha256::digest(original)),"lines":executed[*file]}));
        }
        let coverage = dir.path().join("coverage.json");
        fs::write(
            &coverage,
            serde_json::to_vec(&serde_json::json!({"version":1,"files":native_files})).unwrap(),
        )
        .unwrap();
        let scan = |threshold: &str| {
            Command::new(env!("CARGO_BIN_EXE_dotnet-crap"))
                .arg("--root")
                .arg(&root)
                .args(files)
                .arg("--coverage")
                .arg(&coverage)
                .args([
                    "--require-coverage",
                    "--format",
                    "json",
                    "--threshold",
                    threshold,
                ])
                .output()
                .unwrap()
        };
        let output = scan("100");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["files_scanned"], 3);
        assert_eq!(report["coverage_complete"], true);
        let functions = report["functions"].as_array().unwrap();
        assert_eq!(functions.len(), 4);
        for (name, complexity, partial_score) in [
            ("Pick", 3, 4.125),
            ("Run", 5, 8.125),
            ("Outer", 2, 2.5),
            ("Inner", 1, 1.0),
        ] {
            let item = functions.iter().find(|i| i["name"] == name).unwrap();
            assert_eq!(item["complexity"], complexity);
            assert_eq!(
                item["coverage"],
                if full || name == "Inner" { 1.0 } else { 0.5 }
            );
            assert_eq!(
                item["crap"],
                if full {
                    f64::from(complexity)
                } else {
                    partial_score
                }
            );
        }
        let boundary = if full { "5" } else { "8.125" };
        let lower = if full { "4.999" } else { "8.124" };
        assert!(scan(boundary).status.success());
        assert_eq!(scan(lower).status.code(), Some(1));
    }
}

#[test]
fn bom_encodings_bind_coverage_to_original_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let source = "class C { int M() => 1; }\n";
    let mut little = vec![0xff, 0xfe];
    let mut big = vec![0xfe, 0xff];
    for unit in source.encode_utf16() {
        little.extend(unit.to_le_bytes());
        big.extend(unit.to_be_bytes());
    }
    let mut bom = vec![0xef, 0xbb, 0xbf];
    bom.extend(source.as_bytes());
    for bytes in [little, big, bom] {
        fs::write(dir.path().join("C.CS"), &bytes).unwrap();
        let coverage = dir.path().join("coverage.json");
        let write_coverage = |hash: String| {
            fs::write(
                &coverage,
                serde_json::json!({"version":1,"files":{"C.CS":{"sha256":hash,"lines":{"1":1}}}})
                    .to_string(),
            )
            .unwrap()
        };
        write_coverage(hex::encode(Sha256::digest(&bytes)));
        let scan = || {
            Command::new(env!("CARGO_BIN_EXE_dotnet-crap"))
                .arg("--root")
                .arg(dir.path())
                .arg("--coverage")
                .arg(&coverage)
                .args(["--require-coverage", "--format", "json", "--threshold", "1"])
                .output()
                .unwrap()
        };
        let output = scan();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["functions"][0]["crap"], 1.0);
        assert_eq!(report["functions"][0]["coverage"], 1.0);
        write_coverage(hex::encode(Sha256::digest(source.as_bytes())));
        assert_eq!(scan().status.code(), Some(2));
    }
    for bytes in [vec![0xff, 0xfe, 0x01], vec![0xfe, 0xff, 0xd8, 0x00]] {
        fs::write(dir.path().join("C.CS"), bytes).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_dotnet-crap"))
            .arg("--root")
            .arg(dir.path())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn deep_syntax_is_scored_without_recursive_rust_traversal() {
    let dir = tempfile::tempdir().unwrap();
    let source = format!(
        "class Deep {{ int F(bool flag) {{ {}return 1;{} return 0; }} }}",
        "if (flag) {".repeat(2000),
        "}".repeat(2000)
    );
    fs::write(dir.path().join("Deep.cs"), source).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_dotnet-crap"))
        .arg("--root")
        .arg(dir.path())
        .args(["--format", "json", "--threshold", "10000000"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["functions"][0]["complexity"], 2001);
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn invalid_utf8_filenames_fail_instead_of_collapsing_source_identity() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(OsString::from_vec(b"C-\xff.cs".to_vec())),
        "class C { void F() {} }",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_dotnet-crap"))
        .arg("--root")
        .arg(dir.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("non-UTF-8 source path"));
}

#[cfg(unix)]
#[test]
fn invalid_source_diagnostics_escape_filename_controls() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("invalid\u{1b}\n.cs"), "class C { int F( }").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_dotnet-crap"))
        .arg("--root")
        .arg(dir.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(!output.stderr.contains(&0x1b));
    assert_eq!(output.stderr.iter().filter(|&&b| b == b'\n').count(), 1);
    assert!(String::from_utf8_lossy(&output.stderr).contains(r"invalid\u{1b}\n.cs"));
}

#[cfg(unix)]
#[test]
fn source_filenames_unrepresentable_in_native_coverage_fail_explicitly() {
    for name in [r"literal\name.cs", "colon:name.cs"] {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(name), "class C { void F() {} }").unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_dotnet-crap"))
            .arg("--root")
            .arg(dir.path())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("unsupported by native coverage"));
    }
}

#[test]
fn long_nested_ranges_exclude_only_the_local_callable_span() {
    let dir = tempfile::tempdir().unwrap();
    let source = format!("class C {{\n int Outer() {{\n int Local() {{\n{} return 1;\n }}\n return Local();\n }}\n}}\n", "\n".repeat(20000));
    fs::write(dir.path().join("C.cs"), &source).unwrap();
    let coverage = dir.path().join("coverage.json");
    fs::write(
        &coverage,
        serde_json::json!({"version":1,"files":{"C.cs":{
            "sha256":hex::encode(Sha256::digest(source.as_bytes())),
            "lines":{"2":1,"3":0,"10000":0,"20004":0,"20005":0,"20006":1,"20007":1}
        }}})
        .to_string(),
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_dotnet-crap"))
        .arg("--root")
        .arg(dir.path())
        .arg("--coverage")
        .arg(&coverage)
        .args(["--format", "json", "--require-coverage"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let functions = report["functions"].as_array().unwrap();
    let outer = functions.iter().find(|f| f["name"] == "Outer").unwrap();
    let local = functions.iter().find(|f| f["name"] == "Local").unwrap();
    assert_eq!(outer["coverage"], 1.0);
    assert_eq!(outer["crap"], 1.0);
    assert_eq!(local["coverage"], 0.0);
    assert_eq!(local["crap"], 2.0);
}
