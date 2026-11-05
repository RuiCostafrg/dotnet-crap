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
    check("Switch.cs", "Pick", 3, &[("2", 1), ("5", 0)], 4.125);
}
#[test]
fn branches() {
    check("Branches.cs", "Run", 5, &[("2", 1), ("4", 0)], 8.125);
}
#[test]
fn nested_ownership() {
    check(
        "Nested.cs",
        "Outer",
        1,
        &[("2", 1), ("3", 1), ("4", 0)],
        1.125,
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
    let output = Command::new("dotnet")
        .args([
            "run",
            "--project",
            root.join("Accuracy.csproj").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("fixture execution passed"));
}
