//! The published JSON Schemas against the real outputs and the real loaders.

use dotnet_crap::{baseline, config};
use jsonschema::Validator;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

const NAMES: [&str; 4] = ["report", "coverage", "config", "baseline"];

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn schema_json(name: &str) -> Value {
    let text =
        fs::read_to_string(manifest().join(format!("schemas/{name}.v1.schema.json"))).unwrap();
    serde_json::from_str(&text).unwrap()
}

fn validator(name: &str) -> Validator {
    jsonschema::draft202012::new(&schema_json(name)).unwrap()
}

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_dotnet-crap"))
        .arg("--root")
        .arg(root)
        .args(args)
        .output()
        .unwrap()
}

fn stdout_json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|e| {
        panic!(
            "stdout is not JSON ({e}); stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

/// Copy the accuracy fixture's files (not build output such as `bin/` and `obj/`,
/// which CI creates by building it) into a fresh temp directory.
fn accuracy_copy() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for entry in fs::read_dir(manifest().join("tests/fixtures/accuracy")).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_file() {
            fs::copy(entry.path(), dir.path().join(entry.file_name())).unwrap();
        }
    }
    dir
}

/// Native coverage reporting every line of every `.cs` file in `root` as hit.
fn native_coverage(root: &Path) -> Value {
    let mut files = serde_json::Map::new();
    for entry in fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "cs") {
            let bytes = fs::read(&path).unwrap();
            let lines: serde_json::Map<String, Value> =
                (1..=String::from_utf8_lossy(&bytes).lines().count())
                    .map(|n| (n.to_string(), json!(1)))
                    .collect();
            files.insert(
                path.file_name().unwrap().to_str().unwrap().to_owned(),
                json!({"sha256": hex::encode(Sha256::digest(&bytes)), "lines": lines}),
            );
        }
    }
    json!({"version": 1, "files": files})
}

fn assert_valid(validator: &Validator, instance: &Value, what: &str) {
    let errors: Vec<String> = validator
        .iter_errors(instance)
        .map(|e| e.to_string())
        .collect();
    assert!(errors.is_empty(), "{what} violates its schema: {errors:?}");
}

#[test]
fn every_schema_is_a_valid_draft_2020_12_schema() {
    for name in NAMES {
        let schema = schema_json(name);
        jsonschema::meta::validate(&schema).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(
            schema["$schema"],
            "https://json-schema.org/draft/2020-12/schema"
        );
        assert_eq!(
            schema["$id"],
            format!(
                "https://raw.githubusercontent.com/RuiCostafrg/dotnet-crap/main/schemas/{name}.v1.schema.json"
            )
        );
        assert!(schema["title"].is_string() && schema["description"].is_string());
        jsonschema::draft202012::new(&schema).unwrap();
    }
}

#[test]
fn real_json_reports_validate() {
    let report = validator("report");
    let plain = accuracy_copy();
    let out = run(plain.path(), &["--format", "json"]);
    let value = stdout_json(&out);
    assert_valid(&report, &value, "plain report");
    assert_eq!(value["version"], 1);
    assert!(
        String::from_utf8_lossy(&out.stdout).starts_with("{\n  \"version\": 1,\n"),
        "version is the first field"
    );
    assert!(!value["functions"].as_array().unwrap().is_empty());
    assert!(value["functions"][0]["coverage"].is_null());

    // Skipped files.
    let skipped = accuracy_copy();
    fs::write(skipped.path().join("Broken.cs"), "class { { {").unwrap();
    let value = stdout_json(&run(
        skipped.path(),
        &["--format", "json", "--skip-unparsed"],
    ));
    assert_valid(&report, &value, "skipped report");
    assert!(!value["skipped_files"].as_array().unwrap().is_empty());

    // Partially scored files.
    let partial = accuracy_copy();
    fs::write(
        partial.path().join("Broken.cs"),
        "class B { int Ok() { return 1; } int Bad() { return ) ; } }\n",
    )
    .unwrap();
    let value = stdout_json(&run(
        partial.path(),
        &["--format", "json", "--partial-unparsed"],
    ));
    assert_valid(&report, &value, "partial report");
    assert_eq!(value["partial_files"][0]["unscored"], 1, "{value}");

    // Coverage.
    let covered = accuracy_copy();
    let coverage = covered.path().join("coverage.json");
    let doc = native_coverage(covered.path());
    fs::write(&coverage, doc.to_string()).unwrap();
    assert_valid(&validator("coverage"), &doc, "generated native coverage");
    let value = stdout_json(&run(
        covered.path(),
        &["--format", "json", "--coverage", coverage.to_str().unwrap()],
    ));
    assert_valid(&report, &value, "covered report");
    assert!(value["functions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["coverage"].is_number()));
}

#[test]
fn written_baselines_and_baseline_reports_validate() {
    let dir = accuracy_copy();
    let path = dir.path().join("baseline.json");
    let out = run(
        dir.path(),
        &[
            "--threshold",
            "1",
            "--write-baseline",
            path.to_str().unwrap(),
        ],
    );
    assert_eq!(out.status.code(), Some(0));
    let written: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert!(!written["entries"].as_array().unwrap().is_empty());
    assert_valid(&validator("baseline"), &written, "written baseline");
    baseline::read(&path).unwrap();

    // Make one callable worse so several states can occur, then compare.
    fs::write(
        dir.path().join("Extra.cs"),
        "class Extra { int F(int a) { if (a > 1) return 1; if (a > 2) return 2; return 0; } }\n",
    )
    .unwrap();
    let out = run(
        dir.path(),
        &[
            "--threshold",
            "1",
            "--format",
            "json",
            "--baseline",
            path.to_str().unwrap(),
        ],
    );
    let value = stdout_json(&out);
    assert_valid(&validator("report"), &value, "baseline report");
    assert!(value["baseline"].is_object());
    assert!(value["functions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["baseline"].is_string()));
}

/// The ```json blocks of a markdown file.
fn json_blocks(doc: &str) -> Vec<Value> {
    let text = fs::read_to_string(manifest().join(doc)).unwrap();
    let mut blocks = vec![];
    let mut current: Option<String> = None;
    for line in text.lines() {
        match (&mut current, line.trim_end()) {
            (None, "```json") => current = Some(String::new()),
            (Some(block), "```") => {
                blocks.push(serde_json::from_str(block).unwrap_or_else(|e| panic!("{doc}: {e}")));
                current = None;
            }
            (Some(block), l) => {
                block.push_str(l);
                block.push('\n');
            }
            _ => {}
        }
    }
    blocks
}

fn temp_file(name: &str, text: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(name);
    fs::write(&path, text).unwrap();
    (dir, path)
}

#[test]
fn documented_examples_validate_and_load() {
    let blocks = json_blocks("docs/CONFIG.md");
    assert!(!blocks.is_empty());
    for block in blocks {
        assert_valid(&validator("config"), &block, "CONFIG.md example");
        let (_dir, path) = temp_file("dotnet-crap.json", &block.to_string());
        config::load(&path).unwrap();
    }
    let blocks = json_blocks("docs/BASELINES.md");
    assert!(!blocks.is_empty());
    for block in blocks {
        assert_valid(&validator("baseline"), &block, "BASELINES.md example");
        let (_dir, path) = temp_file("baseline.json", &block.to_string());
        baseline::read(&path).unwrap();
    }
}

/// Whether the Rust loader for `kind` accepts `doc`.
fn loader_accepts(kind: &str, doc: &Value) -> bool {
    let (dir, path) = temp_file("input.json", &doc.to_string());
    match kind {
        "config" => config::load(&path).is_ok(),
        "baseline" => baseline::read(&path).is_ok(),
        _ => {
            // Native coverage is read by a scan of an empty root: entries are validated
            // but bound to no source.
            let cov = dir.path().join("input.json");
            let out = run(dir.path(), &["--coverage", cov.to_str().unwrap()]);
            out.status.code() != Some(2)
        }
    }
}

fn agree(kind: &str, valid: &[Value], invalid: &[Value]) {
    let schema = validator(kind);
    for doc in valid {
        assert!(schema.is_valid(doc), "{kind} schema rejects {doc}");
        assert!(loader_accepts(kind, doc), "{kind} loader rejects {doc}");
    }
    for doc in invalid {
        assert!(!schema.is_valid(doc), "{kind} schema accepts {doc}");
        assert!(!loader_accepts(kind, doc), "{kind} loader accepts {doc}");
    }
}

#[test]
fn config_schema_agrees_with_the_loader() {
    agree(
        "config",
        &[
            json!({"version": 1}),
            json!({"version": 1, "threshold": 0}),
            json!({"version": 1, "threshold": 7.5, "include": ["src/**"], "exclude": [],
                   "respect_ignore_files": false, "require_coverage": true,
                   "fail_on_empty": true, "skip_unparsed": true, "partial_unparsed": true, "defines": ["A"],
                   "baseline": "b.json"}),
            json!({"version": 1, "threshold": null, "baseline": null}),
        ],
        &[
            json!({}),
            json!({"version": 2}),
            json!({"version": "1"}),
            json!({"version": 1, "treshold": 3}),
            json!({"version": 1, "threshold": -1}),
            json!({"version": 1, "threshold": "10"}),
            json!({"version": 1, "include": "src"}),
            json!({"version": 1, "include": [1]}),
            json!({"version": 1, "require_coverage": "yes"}),
            json!({"version": 1, "baseline": 3}),
            json!([]),
        ],
    );
}

fn entry() -> Value {
    json!({"file": "a.cs", "kind": "method_declaration", "scope": "A", "signature": "M()",
           "occurrence": 0, "fingerprint": "ab", "line": 1, "complexity": 3,
           "coverage": null, "crap": 12.0})
}

#[test]
fn baseline_schema_agrees_with_the_loader() {
    let with = |key: &str, value: Value| {
        let mut e = entry();
        e[key] = value;
        json!({"version": 1, "threshold": 10.0, "entries": [e]})
    };
    agree(
        "baseline",
        &[
            json!({"version": 1, "threshold": 10, "entries": []}),
            json!({"version": 1, "threshold": 10.5, "entries": [entry()]}),
            with("coverage", json!(0.5)),
        ],
        &[
            json!({"threshold": 10, "entries": []}),
            json!({"version": 2, "threshold": 10, "entries": []}),
            json!({"version": 1, "entries": []}),
            json!({"version": 1, "threshold": "10", "entries": []}),
            json!({"version": 1, "threshold": 10, "entries": [], "extra": 1}),
            json!({"version": 1, "threshold": 10, "entries": {}}),
            with("extra", json!(1)),
            with("occurrence", json!(-1)),
            with("complexity", json!("3")),
            with("line", json!(1.5)),
            with("crap", json!("12")),
            with("file", json!(null)),
        ],
    );
}

#[test]
fn coverage_schema_agrees_with_the_reader() {
    let sha = "ab".repeat(32);
    let doc = |name: &str, sha: &str, line: &str, hits: Value| json!({"version": 1, "files": {name: {"sha256": sha, "lines": {line: hits}}}});
    agree(
        "coverage",
        &[
            json!({"version": 1, "files": {}}),
            doc("a.cs", &sha, "1", json!(0)),
            doc("src/dir/A.cs", &sha.to_uppercase(), "120", json!(7)),
            json!({"version": 1, "files": {"a.cs": {"sha256": sha, "lines": {}}}}),
        ],
        &[
            json!({"files": {}}),
            json!({"version": 2, "files": {}}),
            json!({"version": "1", "files": {}}),
            json!({"version": 1, "files": {}, "extra": true}),
            json!({"version": 1, "files": []}),
            doc("a.cs", "bad", "1", json!(1)),
            doc("a.cs", &"g".repeat(64), "1", json!(1)),
            doc("a.cs", &sha, "0", json!(1)),
            doc("a.cs", &sha, "01", json!(1)),
            doc("a.cs", &sha, "-1", json!(1)),
            doc("a.cs", &sha, "x", json!(1)),
            doc("a.cs", &sha, "1", json!(-1)),
            doc("a.cs", &sha, "1", json!(1.5)),
            doc("a.cs", &sha, "1", json!("1")),
            doc("../a.cs", &sha, "1", json!(1)),
            doc("a/./b.cs", &sha, "1", json!(1)),
            doc("/a.cs", &sha, "1", json!(1)),
            doc("a//b.cs", &sha, "1", json!(1)),
            doc("a/", &sha, "1", json!(1)),
            doc("a\\b.cs", &sha, "1", json!(1)),
            doc("C:/a.cs", &sha, "1", json!(1)),
            doc("", &sha, "1", json!(1)),
            json!({"version": 1, "files": {"a.cs": {"sha256": sha, "lines": {}, "extra": 1}}}),
            json!({"version": 1, "files": {"a.cs": {"sha256": sha}}}),
            json!({"version": 1, "files": {"a.cs": {"lines": {}}}}),
        ],
    );
}
