//! Hand-scored logical patterns, using physical SDK-compilable C# sources.
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf, process::Command};

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/modern-patterns")
}

fn scan(file: &str, coverage: Option<&std::path::Path>, threshold: &str) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_dotnet-crap"));
    command.args([
        "--root",
        fixture().to_str().unwrap(),
        "--format",
        "json",
        "--threshold",
        threshold,
    ]);
    if let Some(path) = coverage {
        command
            .arg("--coverage")
            .arg(path)
            .arg("--require-coverage");
    }
    command.arg(file).output().unwrap()
}

#[test]
fn logical_patterns_have_independently_counted_complexities() {
    let output = scan("Patterns.cs", None, "100");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    let actual: Vec<_> = report["functions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            (
                f["name"].as_str().unwrap(),
                f["kind"].as_str().unwrap(),
                f["complexity"].as_u64().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        actual,
        vec![
            ("Logical", "method_declaration", 3), // base + and + or
            ("Negated", "method_declaration", 2), // base + and; not adds nothing
            ("Simple", "method_declaration", 1),
            ("Property", "method_declaration", 3),
            ("List", "method_declaration", 3),
            ("Expression", "method_declaration", 5), // base + two arms + and + or
            ("Statement", "method_declaration", 6),  // base + two cases + when + and + or
            ("Guard", "method_declaration", 5),      // base + arm + when + && + and
            ("Owned", "method_declaration", 3), // base + arm + ternary; nested patterns excluded
            ("Local", "local_function_statement", 3),
            ("lambda_expression", "lambda_expression", 2),
            ("Text", "method_declaration", 1),
        ]
    );
}

#[test]
fn pattern_decisions_and_coverage_stay_with_nested_callable_owners() {
    let file = "Patterns.cs";
    let source = fs::read_to_string(fixture().join(file)).unwrap();
    let line = |needle: &str| source.lines().position(|l| l.contains(needle)).unwrap() + 1;
    let points = json!({
        line("public static int Owned").to_string(): 1,
        line("return x switch").to_string(): 0,
        line("int Local").to_string(): 0,
        line("Func<int, bool>").to_string(): 1,
    });
    let temporary = tempfile::tempdir().unwrap();
    let coverage = temporary.path().join("coverage.json");
    fs::write(
        &coverage,
        json!({"version":1,"files":{file:{
            "sha256":hex::encode(Sha256::digest(source.as_bytes())),"lines":points
        }}})
        .to_string(),
    )
    .unwrap();
    // Only the three selected records have supplied points: scan without
    // --require-coverage, which must still reject the other missing records.
    let output = Command::new(env!("CARGO_BIN_EXE_dotnet-crap"))
        .args([
            "--root",
            fixture().to_str().unwrap(),
            "--format",
            "json",
            "--threshold",
            "100",
        ])
        .arg("--coverage")
        .arg(&coverage)
        .arg(file)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    for (name, cc, fraction, crap) in [
        ("Owned", 3, 0.5, 4.125),
        ("Local", 3, 0.0, 12.0),
        ("lambda_expression", 2, 1.0, 2.0),
    ] {
        let f = report["functions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["name"] == name)
            .unwrap();
        assert_eq!(f["complexity"], cc);
        assert_eq!(f["coverage"].as_f64(), Some(fraction));
        assert_eq!(f["crap"].as_f64(), Some(crap));
    }
    assert_eq!(scan(file, Some(&coverage), "100").status.code(), Some(1));
}

#[test]
fn real_sdk_executes_patterns_and_records_partial_and_full_line_evidence() {
    if std::env::var_os("DOTNET_CRAP_E2E").is_none() {
        return;
    }
    let root = fixture();
    let file = "PatternBranch.cs";
    let original = fs::read(root.join(file)).unwrap();
    for full in [false, true] {
        let temporary = tempfile::tempdir().unwrap();
        let hits = temporary.path().join("hits.json");
        let mut command = Command::new("dotnet");
        command
            .arg("run")
            .arg("--project")
            .arg(root.join("ModernPatterns.csproj"))
            .arg("--")
            .arg("--source")
            .arg(root.join(file))
            .arg("--coverage-output")
            .arg(&hits);
        if full {
            command.arg("--full");
        }
        let execution = command.output().unwrap();
        assert!(
            execution.status.success(),
            "stdout={} stderr={}",
            String::from_utf8_lossy(&execution.stdout),
            String::from_utf8_lossy(&execution.stderr)
        );
        assert!(String::from_utf8_lossy(&execution.stdout)
            .contains("modern pattern fixture execution passed"));
        assert_eq!(fs::read(root.join(file)).unwrap(), original);
        let points: Value = serde_json::from_slice(&fs::read(hits).unwrap()).unwrap();
        assert_eq!(points.as_object().unwrap().len(), 2);
        assert_eq!(
            points
                .as_object()
                .unwrap()
                .values()
                .filter(|v| v.as_u64().unwrap() > 0)
                .count(),
            if full { 2 } else { 1 }
        );
        let coverage = temporary.path().join("native.json");
        fs::write(
            &coverage,
            json!({"version":1,"files":{file:{
                "sha256":hex::encode(Sha256::digest(&original)),"lines":points
            }}})
            .to_string(),
        )
        .unwrap();
        let expected = if full { 4.0 } else { 6.0 }; // M=4, C=1 or 1/2
        let output = scan(file, Some(&coverage), &expected.to_string());
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        let functions = report["functions"].as_array().unwrap();
        assert_eq!(functions.len(), 1);
        assert_eq!(functions[0]["complexity"], 4);
        assert_eq!(
            functions[0]["coverage"].as_f64(),
            Some(if full { 1.0 } else { 0.5 })
        );
        assert_eq!(functions[0]["crap"].as_f64(), Some(expected));
        assert_eq!(
            scan(file, Some(&coverage), &(expected - 0.001).to_string())
                .status
                .code(),
            Some(1)
        );
    }
}
