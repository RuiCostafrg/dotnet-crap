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

fn scan(root: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_dotnet-crap"))
        .arg("--root")
        .arg(root)
        .args(args)
        .output()
        .unwrap()
}
fn coverage(root: &std::path::Path, source: &str, lines: serde_json::Value) -> std::path::PathBuf {
    use sha2::{Digest, Sha256};
    let path = root.join("coverage.json");
    fs::write(&path, serde_json::to_vec(&serde_json::json!({
        "version":1,"files":{"C.cs":{"sha256":hex::encode(Sha256::digest(source.as_bytes())),"lines":lines}}
    })).unwrap()).unwrap();
    path
}

#[test]
fn coverage_ownership_and_locations_use_exact_callable_boundaries() {
    let dir = tempfile::tempdir().unwrap();
    let source = "class C {\n\n  int Outer() {\n    int x = 1;\n    int Local() {\n      if (x > 0) return 1;\n      return 0;\n    }\n    return Local();\n  }\n}\n";
    fs::write(dir.path().join("C.cs"), source).unwrap();
    let path = coverage(
        dir.path(),
        source,
        serde_json::json!({"2":0,"3":2,"5":0,"8":0,"9":0,"10":2,"11":0}),
    );
    let output = scan(
        dir.path(),
        &[
            "--format",
            "json",
            "--threshold",
            "100",
            "--coverage",
            path.to_str().unwrap(),
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let functions = report["functions"].as_array().unwrap();
    assert_eq!(functions.len(), 2);
    let outer = functions.iter().find(|f| f["name"] == "Outer").unwrap();
    assert_eq!(outer["line"], 3);
    assert_eq!(outer["column"], 3);
    assert_eq!(outer["end_line"], 10);
    assert_eq!(outer["complexity"], 1);
    assert!((outer["coverage"].as_f64().unwrap() - 2.0 / 3.0).abs() < 1e-12);
    let local = functions.iter().find(|f| f["name"] == "Local").unwrap();
    assert_eq!(local["line"], 5);
    assert_eq!(local["column"], 5);
    assert_eq!(local["end_line"], 8);
    assert_eq!(local["complexity"], 2);
    assert_eq!(local["coverage"], 0.0);
}

#[test]
fn switch_arms_and_initialized_auto_properties_are_distinct() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("C.cs"),
        "class C { int P { get; } = 1; int F(int x) => x switch { 1 => 10, 2 => 20, _ => 0 }; }",
    )
    .unwrap();
    let output = scan(dir.path(), &["--format", "json", "--threshold", "100"]);
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let functions = report["functions"].as_array().unwrap();
    assert_eq!(functions.len(), 1);
    assert_eq!(functions[0]["name"], "F");
    assert_eq!(functions[0]["complexity"], 3);
}

#[test]
fn coverage_global_path_hash_and_line_contracts_are_independent() {
    use sha2::{Digest, Sha256};
    let dir = tempfile::tempdir().unwrap();
    let source = "class C { int F() => 1; }\n\n";
    fs::write(dir.path().join("C.cs"), source).unwrap();
    let hash = hex::encode(Sha256::digest(source.as_bytes()));
    let path = dir.path().join("coverage.json");
    for name in [
        "",
        "../C.cs",
        "/C.cs",
        "folder//C.cs",
        "folder/",
        "folder\\C.cs",
        "C:/C.cs",
        "./C.cs",
    ] {
        fs::write(
            &path,
            serde_json::to_vec(
                &serde_json::json!({"version":1,"files":{name:{"sha256":hash,"lines":{}}}}),
            )
            .unwrap(),
        )
        .unwrap();
        let output = scan(dir.path(), &["--coverage", path.to_str().unwrap()]);
        assert_eq!(output.status.code(), Some(2), "path={name}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("invalid coverage entry"));
    }
    for hash in ["x".repeat(64), "a".repeat(63), "a".repeat(65)] {
        // An unselected file ensures stale-source validation cannot mask a
        // missing global format check.
        fs::write(
            &path,
            serde_json::to_vec(
                &serde_json::json!({"version":1,"files":{"other.cs":{"sha256":hash,"lines":{}}}}),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            scan(dir.path(), &["--coverage", path.to_str().unwrap()])
                .status
                .code(),
            Some(2)
        );
    }
    for line in ["0", "00", "01", "+1", "-1", "1.0", "x"] {
        coverage(dir.path(), source, serde_json::json!({line:0}));
        let output = scan(dir.path(), &["--coverage", path.to_str().unwrap()]);
        assert_eq!(output.status.code(), Some(2), "line={line}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("invalid coverage line"));
    }
    // The last line is valid; a point beyond the source is not.
    coverage(dir.path(), source, serde_json::json!({"1":1,"2":0}));
    assert!(scan(dir.path(), &["--coverage", path.to_str().unwrap()])
        .status
        .success());
    coverage(dir.path(), source, serde_json::json!({"3":0}));
    assert_eq!(
        scan(dir.path(), &["--coverage", path.to_str().unwrap()])
            .status
            .code(),
        Some(2)
    );
}

#[test]
fn nested_source_selection_keeps_normal_directories() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::create_dir(dir.path().join("obj")).unwrap();
    fs::write(dir.path().join("src/C.cs"), "class C { void F() {} }").unwrap();
    fs::write(dir.path().join("obj/Invalid.cs"), "INVALID").unwrap();
    fs::write(dir.path().join("notes.txt"), "INVALID").unwrap();
    let output = scan(dir.path(), &["--format", "json", "--fail-on-empty"]);
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["files_scanned"], 1);
    assert_eq!(report["functions"][0]["file"], "src/C.cs");
}

#[test]
fn existing_relative_reports_are_replaced_without_alias_false_positives() {
    let dir = tempfile::tempdir().unwrap();
    let source = "class C { void F() {} }";
    fs::write(dir.path().join("C.cs"), source).unwrap();
    fs::write(dir.path().join("report.json"), "old report").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_dotnet-crap"))
        .current_dir(dir.path())
        .args(["--root", ".", "--format", "json", "--output", "report.json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    let report: serde_json::Value =
        serde_json::from_slice(&fs::read(dir.path().join("report.json")).unwrap()).unwrap();
    assert_eq!(report["functions"][0]["name"], "F");
    assert_eq!(fs::read_to_string(dir.path().join("C.cs")).unwrap(), source);
}

#[test]
fn text_reports_coverage_requirement_only_when_unsatisfied() {
    let dir = tempfile::tempdir().unwrap();
    let source = "class C { void F() {} }\n";
    fs::write(dir.path().join("C.cs"), source).unwrap();
    let ordinary = scan(dir.path(), &[]);
    assert!(ordinary.status.success());
    assert!(!String::from_utf8_lossy(&ordinary.stdout).contains("Coverage missing"));
    let required = scan(dir.path(), &["--require-coverage"]);
    assert_eq!(required.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&required.stdout).contains("Coverage missing"));
    let path = coverage(dir.path(), source, serde_json::json!({"1":1}));
    let covered = scan(
        dir.path(),
        &["--require-coverage", "--coverage", path.to_str().unwrap()],
    );
    assert!(covered.status.success());
    assert!(!String::from_utf8_lossy(&covered.stdout).contains("Coverage missing"));
    assert!(String::from_utf8_lossy(&covered.stdout).contains("coverage=100.0%"));
}
