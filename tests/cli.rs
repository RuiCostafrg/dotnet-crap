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
#[test]
fn skip_unparsed_skips_only_syntax_failures() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("good.cs"),
        "class Good { int M() { return 1; } }\n",
    )
    .unwrap();
    fs::write(dir.path().join("bad.cs"), "class Bad { void M( }\n").unwrap();
    // Without the flag the scan still fails on the first syntax error.
    let output = run(dir.path(), &[]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "error: invalid C# syntax in bad.cs\n"
    );
    // Text: the valid callable, then the skipped-file list.
    let output = run(dir.path(), &["--skip-unparsed"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(
        text.contains("good.cs:1 M complexity=1 coverage=absent CRAP=2.00\n"),
        "{text}"
    );
    assert!(
        text.ends_with("skipped 1 unparsed file(s):\n  bad.cs: invalid C# syntax in bad.cs\n"),
        "{text}"
    );
    // JSON: the skipped list is present only when non-empty.
    let output = run(dir.path(), &["--skip-unparsed", "--format", "json"]);
    assert_eq!(output.status.code(), Some(0));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["files_scanned"], 2);
    assert_eq!(report["functions"].as_array().unwrap().len(), 1);
    assert_eq!(
        report["skipped_files"],
        serde_json::json!([{"path": "bad.cs", "message": "invalid C# syntax in bad.cs"}])
    );
    // SARIF: one warning notification with the skipped file's location.
    let output = run(dir.path(), &["--skip-unparsed", "--format", "sarif"]);
    assert_eq!(output.status.code(), Some(0));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(report["runs"][0]["results"].as_array().unwrap().is_empty());
    let notifications = report["runs"][0]["invocations"][0]["toolExecutionNotifications"]
        .as_array()
        .unwrap();
    assert_eq!(notifications.len(), 1);
    assert_eq!(notifications[0]["level"], "warning");
    assert_eq!(
        notifications[0]["message"]["text"],
        "invalid C# syntax in bad.cs"
    );
    let location = &notifications[0]["locations"][0]["physicalLocation"];
    assert_eq!(location["artifactLocation"]["uri"], "bad.cs");
    assert_eq!(location["artifactLocation"]["uriBaseId"], "%SRCROOT%");
}

#[test]
fn skip_unparsed_does_not_skip_non_syntax_failures() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("good.cs"), "class Good { int M() => 1; }\n").unwrap();
    fs::write(dir.path().join("bad_utf8.cs"), [0xffu8, 0xfe, 0x41]).unwrap();
    let output = run(dir.path(), &["--skip-unparsed"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("invalid UTF-16 source"), "{stderr}");
    assert!(stderr.contains("bad_utf8.cs"), "{stderr}");
}

#[test]
fn json_has_no_skipped_files_key_without_skips() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("good.cs"), "class Good { int M() => 1; }\n").unwrap();
    for args in [
        vec!["--format", "json"],
        vec!["--format", "json", "--skip-unparsed"],
    ] {
        let output = run(dir.path(), &args);
        assert_eq!(output.status.code(), Some(0), "{args:?}");
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert!(report.get("skipped_files").is_none(), "{args:?}");
    }
}

#[test]
fn jobs_rejects_zero_and_never_changes_output() {
    let dir = tempfile::tempdir().unwrap();
    for i in 0..12 {
        let body = (0..=i % 4).fold(String::new(), |s, n| {
            format!("{s} int M{n}(int x) {{ if (x > {n}) return 1; return 2; }}")
        });
        fs::write(
            dir.path().join(format!("f{i:02}.cs")),
            format!("class C{i} {{{body} }}"),
        )
        .unwrap();
    }
    for format in ["text", "json", "sarif"] {
        let reference = run(dir.path(), &["--format", format]);
        assert!(reference.status.success(), "{reference:?}");
        for jobs in ["1", "2", "3", "8", "64"] {
            let output = run(dir.path(), &["--format", format, "--jobs", jobs]);
            assert_eq!(output.status.code(), Some(0));
            assert_eq!(output.stdout, reference.stdout, "{format} --jobs {jobs}");
        }
    }
    for invalid in ["0", "-1", "abc", ""] {
        let output = run(dir.path(), &[&format!("--jobs={invalid}")]);
        assert_eq!(output.status.code(), Some(2), "--jobs {invalid:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("is not a positive integer; use 1 or more"),
            "{stderr}"
        );
        assert!(output.stdout.is_empty());
    }
    fs::write(dir.path().join("f05.cs"), "class {").unwrap();
    for jobs in ["1", "4"] {
        let output = run(dir.path(), &["--jobs", jobs]);
        assert_eq!(output.status.code(), Some(2));
        assert_eq!(
            String::from_utf8_lossy(&output.stderr),
            "error: invalid C# syntax in f05.cs\n"
        );
    }
}

const COMPLEX: &str =
    "class C {\n int F(int x) {\n if (x > 0 && x < 2) return 1;\n return 0;\n }\n}\n";

fn config_dir(config: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.cs"), COMPLEX).unwrap();
    fs::write(dir.path().join("dotnet-crap.json"), config).unwrap();
    dir
}

#[test]
fn config_threshold_is_applied_and_cli_overrides_it() {
    // The only function scores 12 uncovered.
    let dir = config_dir(r#"{"version":1,"threshold":20}"#);
    assert_eq!(run(dir.path(), &[]).status.code(), Some(0));
    assert_eq!(
        run(dir.path(), &["--threshold", "5"]).status.code(),
        Some(1)
    );
    let dir = config_dir(r#"{"version":1,"threshold":5}"#);
    assert_eq!(run(dir.path(), &[]).status.code(), Some(1));
    assert_eq!(
        run(dir.path(), &["--threshold", "20"]).status.code(),
        Some(0)
    );
}

#[test]
fn boolean_cli_flags_override_config_both_ways() {
    let dir = config_dir(r#"{"version":1,"threshold":100,"require_coverage":true}"#);
    assert_eq!(run(dir.path(), &[]).status.code(), Some(1));
    assert_eq!(
        run(dir.path(), &["--no-require-coverage"]).status.code(),
        Some(0)
    );
    // The last of a flag and its negation wins.
    assert_eq!(
        run(dir.path(), &["--no-require-coverage", "--require-coverage"])
            .status
            .code(),
        Some(1)
    );
    let dir = config_dir(r#"{"version":1,"threshold":100}"#);
    assert_eq!(
        run(dir.path(), &["--require-coverage"]).status.code(),
        Some(1)
    );
    let empty = tempfile::tempdir().unwrap();
    fs::write(
        empty.path().join("dotnet-crap.json"),
        r#"{"version":1,"fail_on_empty":true}"#,
    )
    .unwrap();
    assert_eq!(run(empty.path(), &[]).status.code(), Some(1));
    assert_eq!(
        run(empty.path(), &["--no-fail-on-empty"]).status.code(),
        Some(0)
    );
}

#[test]
fn config_skip_unparsed_is_overridden_by_no_skip_unparsed() {
    let dir = config_dir(r#"{"version":1,"skip_unparsed":true}"#);
    fs::write(dir.path().join("Bad.cs"), "class Bad { void M( }\n").unwrap();
    assert_eq!(
        run(dir.path(), &["--threshold", "100"]).status.code(),
        Some(0)
    );
    assert_eq!(
        run(dir.path(), &["--threshold", "100", "--no-skip-unparsed"])
            .status
            .code(),
        Some(2)
    );
}

#[test]
fn config_include_exclude_and_cli_replacement() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["src/a.cs", "tests/b.cs"] {
        let path = dir.path().join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, COMPLEX).unwrap();
    }
    fs::write(
        dir.path().join("dotnet-crap.json"),
        r#"{"version":1,"threshold":100,"exclude":["tests/**"]}"#,
    )
    .unwrap();
    let files = |args: &[&str]| -> Vec<String> {
        let out = run(
            dir.path(),
            &[args, &["--threshold", "100", "--format", "json"]].concat(),
        );
        assert_eq!(out.status.code(), Some(0));
        let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        report["functions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["file"].as_str().unwrap().to_owned())
            .collect()
    };
    assert_eq!(files(&[]), ["src/a.cs"]);
    // A CLI list replaces the config list instead of adding to it.
    assert_eq!(files(&["--exclude", "src/**"]), ["tests/b.cs"]);
    assert_eq!(files(&["--no-config"]), ["src/a.cs", "tests/b.cs"]);
    assert_eq!(files(&["--include", "src/**", "--no-config"]), ["src/a.cs"]);
}

#[test]
fn no_ignore_and_gitignore_are_honored() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.cs"), COMPLEX).unwrap();
    fs::write(dir.path().join("b.cs"), COMPLEX).unwrap();
    fs::write(dir.path().join(".gitignore"), "b.cs\n").unwrap();
    let count = |args: &[&str]| {
        let out = run(
            dir.path(),
            &[args, &["--threshold", "100", "--format", "json"]].concat(),
        );
        let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        report["functions"].as_array().unwrap().len()
    };
    assert_eq!(count(&[]), 1);
    assert_eq!(count(&["--no-ignore"]), 2);
    fs::write(
        dir.path().join("dotnet-crap.json"),
        r#"{"version":1,"respect_ignore_files":false}"#,
    )
    .unwrap();
    assert_eq!(count(&[]), 2);
}

#[test]
fn no_config_ignores_an_invalid_discovered_file() {
    let dir = config_dir("{ not json");
    assert_eq!(run(dir.path(), &[]).status.code(), Some(2));
    assert_eq!(run(dir.path(), &["--no-config"]).status.code(), Some(1));
}

#[test]
fn explicit_config_errors_exit_two_and_name_the_file() {
    let dir = config_dir(r#"{"version":1}"#);
    let output = run(dir.path(), &["--config", "missing.json"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("missing.json"));
    let bad = dir.path().join("bad.json");
    fs::write(&bad, r#"{"version":3}"#).unwrap();
    let output = run(dir.path(), &["--config", bad.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("bad.json"));
    let discovered = config_dir(r#"{"version":1,"oops":true}"#);
    let output = run(discovered.path(), &[]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("dotnet-crap.json"));
}

#[test]
fn explicit_config_is_used_over_the_root_file() {
    let dir = config_dir(r#"{"version":1,"threshold":1}"#);
    let other = dir.path().join("other.json");
    fs::write(&other, r#"{"version":1,"threshold":50}"#).unwrap();
    assert_eq!(
        run(dir.path(), &["--config", other.to_str().unwrap()])
            .status
            .code(),
        Some(0)
    );
}

#[test]
fn config_and_no_config_conflict() {
    let dir = config_dir(r#"{"version":1}"#);
    let output = run(dir.path(), &["--config", "x.json", "--no-config"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot be used with"));
}
