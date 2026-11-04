use sha2::{Digest, Sha256};
use std::{fs, process::Command};
fn run(root: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_dotnet-crap"))
        .arg("--root")
        .arg(root)
        .args(args)
        .output()
        .unwrap()
}
#[test]
fn coverage_and_gates() {
    let dir = tempfile::tempdir().unwrap();
    let source = "class C {\n int F(int x) {\n if (x > 0 && x < 2) return 1;\n return 0;\n }\n}\n";
    fs::write(dir.path().join("a.cs"), source).unwrap();
    let output = run(dir.path(), &["--format", "json"]);
    assert_eq!(output.status.code(), Some(1));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["functions"][0]["complexity"], 3);
    assert_eq!(report["functions"][0]["crap"], 12.0);
    let hash = hex::encode(Sha256::digest(source.as_bytes()));
    let coverage = dir.path().join("coverage.json");
    fs::write(
        &coverage,
        format!(
            r#"{{"version":1,"files":{{"a.cs":{{"sha256":"{hash}","lines":{{"3":1,"4":0}}}}}}}}"#
        ),
    )
    .unwrap();
    let output = run(
        dir.path(),
        &[
            "--coverage",
            coverage.to_str().unwrap(),
            "--format",
            "json",
            "--require-coverage",
        ],
    );
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["functions"][0]["coverage"], 0.5);
    assert_eq!(report["functions"][0]["crap"], 4.125);
    fs::write(dir.path().join("a.cs"), format!("{source}\n")).unwrap();
    assert_eq!(
        run(dir.path(), &["--coverage", coverage.to_str().unwrap()])
            .status
            .code(),
        Some(2)
    );
}
#[test]
fn dotnet_fixture() {
    if std::env::var_os("DOTNET_CRAP_E2E").is_none() {
        return;
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/Fixture");
    assert!(Command::new("dotnet")
        .arg("run")
        .arg("--project")
        .arg(&root)
        .status()
        .unwrap()
        .success());
    assert!(run(&root, &["--threshold", "100", "--fail-on-empty"])
        .status
        .success());
}

#[test]
fn csharp_constructs_and_nested_ownership() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("Forms.cs"),
        r#"
using System;
abstract class C {
 public abstract int Abstract();
 public int Auto { get; set; }
 public int Prop => true ? 1 : 0;
 public C() { }
 int Switch(int x) { switch(x) { case 1: return 1; case 2: return 2; default: return 0; } }
 int Expr(int x) => x switch { 1 => 2, _ => 0 };
 int Outer() { int Local(int x) { if(x>0) return 1; return 0; } return Local(1); }
 int Loop(int x) { for(int i=0;i<x;i++) { if(i>1) break; } return x; }
 Func<int,int> Delegate() => delegate(int x) { if (x>0) return 1; return 0; };
}
"#,
    )
    .unwrap();
    let output = run(dir.path(), &["--format", "json", "--threshold", "100"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let items = report["functions"].as_array().unwrap();
    for (name, complexity) in [
        ("Prop", 2),
        ("C", 1),
        ("Switch", 3),
        ("Expr", 2),
        ("Outer", 1),
        ("Local", 2),
        ("Loop", 3),
        ("Delegate", 1),
        ("anonymous_method_expression", 2),
    ] {
        let item = items.iter().find(|i| i["name"] == name).unwrap();
        assert_eq!(item["complexity"], complexity, "{name}");
    }
    assert!(!items
        .iter()
        .any(|i| i["name"] == "Abstract" || i["name"] == "get"));
}

#[test]
fn threshold_empty_missing_coverage_and_input_errors() {
    let dir = tempfile::tempdir().unwrap();
    assert!(run(dir.path(), &[]).status.success());
    assert_eq!(run(dir.path(), &["--fail-on-empty"]).status.code(), Some(1));
    fs::write(dir.path().join("a.cs"), "class A { int F()=>1; }").unwrap();
    assert!(run(dir.path(), &["--threshold", "2"]).status.success());
    assert_eq!(
        run(dir.path(), &["--threshold", "1.999"]).status.code(),
        Some(1)
    );
    assert_eq!(
        run(dir.path(), &["--require-coverage"]).status.code(),
        Some(1)
    );
    for args in [
        vec!["--threshold", "NaN"],
        vec!["missing.cs"],
        vec!["../"],
        vec!["--threshold=-1"],
    ] {
        assert_eq!(run(dir.path(), &args).status.code(), Some(2));
    }
    fs::write(dir.path().join("a.cs"), "class A { int F( }").unwrap();
    let output = run(dir.path(), &[]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
}

#[test]
fn selection_deduplication_exclusions_and_output_safety() {
    let dir = tempfile::tempdir().unwrap();
    let source = "class A { int F()=>1; }";
    fs::write(dir.path().join("a.cs"), source).unwrap();
    fs::create_dir(dir.path().join("bin")).unwrap();
    fs::write(dir.path().join("bin/b.cs"), "INVALID").unwrap();
    fs::write(dir.path().join("ignore.g.cs"), "INVALID").unwrap();
    let output = run(dir.path(), &[".", "a.cs", "bin/b.cs", "--format", "json"]);
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["functions"].as_array().unwrap().len(), 1);
    let source_path = dir.path().join("a.cs");
    assert_eq!(
        run(dir.path(), &["--output", source_path.to_str().unwrap()])
            .status
            .code(),
        Some(2)
    );
    let alias = dir.path().join("alias.json");
    fs::hard_link(&source_path, &alias).unwrap();
    assert_eq!(
        run(dir.path(), &["--output", alias.to_str().unwrap()])
            .status
            .code(),
        Some(2)
    );
    assert_eq!(fs::read_to_string(&source_path).unwrap(), source);
    // PRoot may emulate the hardlink using a symlink. Restore an ordinary
    // source entry before exercising directory selection again.
    fs::remove_file(&source_path).unwrap();
    fs::write(&source_path, source).unwrap();
    let output_path = dir.path().join("report.json");
    assert_eq!(
        run(
            dir.path(),
            &[
                "--threshold",
                "0",
                "--format",
                "json",
                "--output",
                output_path.to_str().unwrap()
            ]
        )
        .status
        .code(),
        Some(1)
    );
    assert!(
        serde_json::from_slice::<serde_json::Value>(&fs::read(&output_path).unwrap()).unwrap()
            ["failed"]
            .as_bool()
            .unwrap()
    );
}

#[test]
fn rejects_duplicate_and_invalid_coverage() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.cs"), "class A { int F()=>1; }").unwrap();
    let path = dir.path().join("cov.json");
    for text in [
        r#"{"version":1,"files":{"a.cs":{"sha256":"0000000000000000000000000000000000000000000000000000000000000000","lines":{"1":0,"1":1}}}}"#,
        r#"{"version":2,"files":{}}"#,
        r#"{"version":1,"files":{},"extra":true}"#,
        r#"{"version":1,"files":{"../a.cs":{"sha256":"bad","lines":{}}}}"#,
    ] {
        fs::write(&path, text).unwrap();
        assert_eq!(
            run(dir.path(), &["--coverage", path.to_str().unwrap()])
                .status
                .code(),
            Some(2)
        );
    }
}
