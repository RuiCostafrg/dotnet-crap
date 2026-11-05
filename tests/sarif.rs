use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{fs, path::Path, process::Command};

fn scan(root: &Path, args: &[&str]) -> (i32, Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_dotnet-crap"))
        .arg("--root")
        .arg(root)
        .args(["--format", "sarif"])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    (
        output.status.code().unwrap(),
        serde_json::from_slice(&output.stdout).unwrap(),
    )
}
fn results(report: &Value) -> &[Value] {
    report["runs"][0]["results"].as_array().unwrap()
}

#[test]
fn score_threshold_empty_and_completeness_gates() {
    let dir = tempfile::tempdir().unwrap();
    let (code, report) = scan(dir.path(), &[]);
    assert_eq!(code, 0);
    assert_eq!(report["version"], "2.1.0");
    assert!(results(&report).is_empty());
    let (code, report) = scan(dir.path(), &["--fail-on-empty"]);
    assert_eq!(code, 1);
    assert_eq!(results(&report).len(), 1);
    assert_eq!(results(&report)[0]["ruleId"], "DNCRAP003");
    assert!(results(&report)[0].get("locations").is_none());
    fs::write(dir.path().join("C.cs"), "class C { int F() => 1; }").unwrap();
    let (code, report) = scan(dir.path(), &["--threshold", "2"]);
    assert_eq!(code, 0); // strict greater-than, not equality
    assert!(results(&report).is_empty());
    let (code, report) = scan(dir.path(), &["--require-coverage", "--threshold", "2"]);
    assert_eq!(code, 1);
    assert_eq!(results(&report).len(), 1);
    assert_eq!(results(&report)[0]["ruleId"], "DNCRAP002");
    let (code, report) = scan(dir.path(), &["--require-coverage", "--threshold", "1"]);
    assert_eq!(code, 1);
    assert_eq!(results(&report).len(), 2);
    assert_eq!(results(&report)[0]["ruleId"], "DNCRAP001");
    assert_eq!(results(&report)[1]["ruleId"], "DNCRAP002");
}

#[test]
fn unicode_uri_regions_and_fingerprints_are_deterministic() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("space #目录")).unwrap();
    let source = "class C {\n string 名() => \"quoted \\\" & < >\";\n int Other() => 1;\n}\n";
    let name = "space #目录/名%.cs";
    fs::write(dir.path().join(name), source).unwrap();
    let (code, report) = scan(dir.path(), &["--threshold", "0"]);
    assert_eq!(code, 1);
    let (_, repeated) = scan(dir.path(), &["--threshold", "0"]);
    assert_eq!(report, repeated);
    let first = &results(&report)[0];
    let location = &first["locations"][0]["physicalLocation"];
    assert_eq!(
        location["artifactLocation"]["uri"],
        "space%20%23%E7%9B%AE%E5%BD%95/%E5%90%8D%25.cs"
    );
    assert_eq!(location["artifactLocation"]["uriBaseId"], "%SRCROOT%");
    let base = report["runs"][0]["originalUriBaseIds"]["%SRCROOT%"]["uri"]
        .as_str()
        .unwrap();
    assert!(base.starts_with("file:///"));
    assert!(base.ends_with('/'));
    assert_eq!(location["region"], json!({"startLine":2,"endLine":2}));
    assert!(first["message"]["text"].as_str().unwrap().contains('名'));
    let fingerprint = &first["partialFingerprints"]["dotnetCrap/v1"];
    assert_eq!(fingerprint.as_str().unwrap().len(), 64);
    assert_ne!(
        fingerprint,
        &results(&report)[1]["partialFingerprints"]["dotnetCrap/v1"]
    );
    let (_, changed_threshold) = scan(dir.path(), &["--threshold", "1"]);
    assert_eq!(
        fingerprint,
        &results(&changed_threshold)[0]["partialFingerprints"]["dotnetCrap/v1"]
    );
    let coverage = dir.path().join("coverage.json");
    fs::write(&coverage, serde_json::to_vec(&json!({"version":1,"files":{name:{"sha256":hex::encode(Sha256::digest(source.as_bytes())),"lines":{"2":1,"3":1}}}})).unwrap()).unwrap();
    let (code, covered) = scan(
        dir.path(),
        &[
            "--coverage",
            coverage.to_str().unwrap(),
            "--require-coverage",
            "--threshold",
            "0",
        ],
    );
    assert_eq!(code, 1);
    assert_eq!(results(&covered).len(), 2);
    assert_eq!(
        fingerprint,
        &results(&covered)[0]["partialFingerprints"]["dotnetCrap/v1"]
    );
}

#[test]
fn staged_output_keeps_failed_reports_and_protects_source() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("C.cs");
    fs::write(&source, "class C { int F() => 1; }").unwrap();
    let output = dir.path().join("report.sarif");
    let status = Command::new(env!("CARGO_BIN_EXE_dotnet-crap"))
        .arg("--root")
        .arg(dir.path())
        .args(["--format", "sarif", "--threshold", "0", "--output"])
        .arg(&output)
        .output()
        .unwrap();
    assert_eq!(status.status.code(), Some(1));
    assert!(status.stdout.is_empty());
    let report: Value = serde_json::from_slice(&fs::read(&output).unwrap()).unwrap();
    assert_eq!(results(&report).len(), 1);
    let status = Command::new(env!("CARGO_BIN_EXE_dotnet-crap"))
        .arg("--root")
        .arg(dir.path())
        .args(["--format", "sarif", "--output"])
        .arg(&source)
        .output()
        .unwrap();
    assert_eq!(status.status.code(), Some(2));
    assert_eq!(
        fs::read_to_string(source).unwrap(),
        "class C { int F() => 1; }"
    );
}

#[test]
fn same_named_overloads_have_distinct_fingerprints() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("C.cs"),
        "class C { int F() => 1; int F(int x) => x; }",
    )
    .unwrap();
    let (code, report) = scan(dir.path(), &["--threshold", "0"]);
    assert_eq!(code, 1);
    assert_eq!(results(&report).len(), 2);
    assert_ne!(
        results(&report)[0]["partialFingerprints"],
        results(&report)[1]["partialFingerprints"]
    );
}

#[cfg(unix)]
#[test]
fn literal_backslashes_in_source_root_remain_literal_uri_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join(r"source\root");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("C.cs"), "class C { int F() => 1; }").unwrap();
    let (code, report) = scan(&root, &["--threshold", "0"]);
    assert_eq!(code, 1);
    let base = report["runs"][0]["originalUriBaseIds"]["%SRCROOT%"]["uri"]
        .as_str()
        .unwrap();
    assert!(base.ends_with("/source%5Croot/"), "{base}");
    assert!(!base.ends_with("/source/root/"));
}
