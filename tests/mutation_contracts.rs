use std::{fs, process::Command};

#[test]
fn malformed_coverage_map_reports_expected_type() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.cs"), "class C { void M() {} }").unwrap();
    let coverage = dir.path().join("coverage.json");
    fs::write(&coverage, r#"{"version":1,"files":[]}"#).unwrap();
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
    assert!(String::from_utf8_lossy(&output.stderr).contains("a map with unique keys"));
}
