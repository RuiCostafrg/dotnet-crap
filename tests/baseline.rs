//! Regression baselines end to end: writing, comparing, policy, reporting and
//! the library API. A callable with `n` `if` statements has complexity `n + 1`
//! and, without coverage, CRAP score `(n + 1)^2 + (n + 1)`: 2 ifs score 12,
//! 3 score 20 and 4 score 30, against the default threshold of 10.

use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_dotnet-crap"))
        .arg("--root")
        .arg(root)
        .args(args)
        .output()
        .unwrap()
}

fn code(output: &Output) -> i32 {
    output.status.code().unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

/// A method with `ifs` branches, so complexity `ifs + 1`.
fn method(name: &str, ifs: usize) -> String {
    let branches: String = (1..=ifs)
        .map(|i| format!("        if (a > {i}) return {i};\n"))
        .collect();
    format!("    int {name}(int a)\n    {{\n{branches}        return 0;\n    }}\n")
}

/// A class holding the given `(name, branches)` methods.
fn class(name: &str, methods: &[(&str, usize)]) -> String {
    let body: String = methods.iter().map(|(n, i)| method(n, *i)).collect();
    format!("class {name}\n{{\n{body}}}\n")
}

struct Project {
    dir: tempfile::TempDir,
}

impl Project {
    fn new(files: &[(&str, String)]) -> Self {
        let project = Self {
            dir: tempfile::tempdir().unwrap(),
        };
        for (name, text) in files {
            project.write(name, text);
        }
        project
    }

    fn root(&self) -> &Path {
        self.dir.path()
    }

    fn write(&self, name: &str, text: &str) {
        let path = self.root().join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn baseline(&self) -> PathBuf {
        self.root().join("baseline.json")
    }

    fn run(&self, args: &[&str]) -> Output {
        run(self.root(), args)
    }

    /// Run with `--baseline` pointing at this project's baseline file.
    fn compare(&self, args: &[&str]) -> Output {
        let baseline = self.baseline();
        let mut all = vec!["--baseline", baseline.to_str().unwrap()];
        all.extend_from_slice(args);
        self.run(&all)
    }

    /// Write this project's baseline, expecting success.
    fn write_baseline(&self, args: &[&str]) -> Output {
        let baseline = self.baseline();
        let mut all = vec!["--write-baseline", baseline.to_str().unwrap()];
        all.extend_from_slice(args);
        let output = self.run(&all);
        assert!(output.status.success() || code(&output) == 1, "{output:?}");
        output
    }

    fn json(&self, args: &[&str]) -> Value {
        let mut all = vec!["--format", "json"];
        all.extend_from_slice(args);
        serde_json::from_slice(&self.run(&all).stdout).unwrap()
    }
}

fn one_complex() -> Project {
    Project::new(&[("A.cs", class("A", &[("Simple", 0), ("Complex", 2)]))])
}

#[test]
fn writing_then_comparing_the_same_code_passes() {
    let p = one_complex();
    assert_eq!(
        code(&p.run(&[])),
        1,
        "the violation fails without a baseline"
    );
    let written = p.write_baseline(&[]);
    assert_eq!(code(&written), 0);
    assert!(stderr(&written).contains("wrote baseline with 1 entries to"));
    let compared = p.compare(&[]);
    assert_eq!(code(&compared), 0, "{}", stderr(&compared));
    let text = stdout(&compared);
    assert!(text.contains("CRAP=12.00 [unchanged]\n"), "{text}");
    assert!(
        text.contains("baseline: 0 new, 0 worse, 1 unchanged, 0 improved, 0 resolved\n"),
        "{text}"
    );
    // Passing callables carry no suffix.
    assert!(text.contains("CRAP=2.00\n"), "{text}");
}

#[test]
fn the_report_is_still_rendered_when_writing() {
    let p = one_complex();
    let plain = p.run(&[]);
    let written = p.write_baseline(&[]);
    assert_eq!(stdout(&written), stdout(&plain));
    let json = p.json(&[
        "--write-baseline",
        p.root().join("other.json").to_str().unwrap(),
    ]);
    // Writing accepted the violation, so the report no longer fails.
    assert_eq!(json["failed"], false);
    assert!(json.get("baseline").is_none());
    assert!(p.root().join("other.json").exists());
}

#[test]
fn a_new_violation_fails_and_is_marked() {
    let p = one_complex();
    p.write_baseline(&[]);
    p.write(
        "A.cs",
        &class("A", &[("Simple", 0), ("Complex", 2), ("Fresh", 3)]),
    );
    let output = p.compare(&[]);
    assert_eq!(code(&output), 1);
    let text = stdout(&output);
    assert!(text.contains("Fresh complexity=4"), "{text}");
    assert!(text.contains("CRAP=20.00 [new]\n"), "{text}");
    assert!(text.contains("CRAP=12.00 [unchanged]\n"), "{text}");
    assert!(
        text.contains("baseline: 1 new, 0 worse, 1 unchanged, 0 improved, 0 resolved\n"),
        "{text}"
    );
}

#[test]
fn a_baselined_callable_that_gets_worse_fails() {
    let p = one_complex();
    p.write_baseline(&[]);
    p.write("A.cs", &class("A", &[("Simple", 0), ("Complex", 3)]));
    let output = p.compare(&[]);
    assert_eq!(code(&output), 1);
    let text = stdout(&output);
    assert!(text.contains("CRAP=20.00 [worse, was 12.00]\n"), "{text}");
    assert!(
        text.contains("baseline: 0 new, 1 worse, 0 unchanged, 0 improved, 0 resolved\n"),
        "{text}"
    );
}

#[test]
fn an_improved_callable_passes_and_a_fixed_one_is_resolved() {
    let p = Project::new(&[("A.cs", class("A", &[("Big", 4), ("Other", 2)]))]);
    p.write_baseline(&[]);
    p.write("A.cs", &class("A", &[("Big", 3), ("Other", 0)]));
    let output = p.compare(&[]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    let text = stdout(&output);
    assert!(
        text.contains("CRAP=20.00 [improved, was 30.00]\n"),
        "{text}"
    );
    assert!(
        text.contains("baseline: 0 new, 0 worse, 0 unchanged, 1 improved, 1 resolved\n"),
        "{text}"
    );
}

#[test]
fn formatting_comments_renames_and_moves_keep_matching() {
    let p = one_complex();
    p.write_baseline(&[]);
    // Reformatted, commented, and the method renamed: same tokens.
    p.write(
        "A.cs",
        "class A {\n // note\n int Simple(int a) { return 0; }\n /* c */ int Renamed ( int a ) { if (a>1) return 1; if (a>2) return 2; return 0; } // end\n}\n",
    );
    assert_eq!(code(&p.compare(&[])), 0);
    // The file renamed and the method moved to another class.
    fs::remove_file(p.root().join("A.cs")).unwrap();
    p.write(
        "sub/B.cs",
        "namespace N { class Other { int Renamed(int a) { if (a > 1) return 1; if (a > 2) return 2; return 0; } } }\n",
    );
    let output = p.compare(&[]);
    assert_eq!(code(&output), 0, "{}", stdout(&output));
    assert!(stdout(&output).contains("[unchanged]"));
}

#[test]
fn baseline_move_pairs_files_that_matching_alone_cannot() {
    // Two files define the same callable with the same body, so once both move
    // neither the fingerprint nor the identity is unique.
    let same = class("P", &[("Run", 2)]);
    let p = Project::new(&[("a/P.cs", same.clone()), ("b/P.cs", same.clone())]);
    p.write_baseline(&[]);
    fs::remove_dir_all(p.root().join("a")).unwrap();
    fs::remove_dir_all(p.root().join("b")).unwrap();
    p.write("x/P.cs", &same);
    p.write("y/P.cs", &same);
    let output = p.compare(&[]);
    assert_eq!(code(&output), 1);
    assert!(stdout(&output).contains("[new]"));
    let moved = p.compare(&[
        "--baseline-move",
        "a/P.cs=x/P.cs",
        "--baseline-move",
        "b/P.cs=y/P.cs",
    ]);
    assert_eq!(code(&moved), 0, "{}", stdout(&moved));
    assert!(
        stdout(&moved).contains("baseline: 0 new, 0 worse, 2 unchanged, 0 improved, 0 resolved")
    );
}

#[test]
fn a_threshold_mismatch_is_an_input_error() {
    let p = one_complex();
    p.write_baseline(&[]);
    let output = p.compare(&["--threshold", "5"]);
    assert_eq!(code(&output), 2);
    assert!(
        stderr(&output).contains("baseline threshold 10 does not match --threshold 5"),
        "{}",
        stderr(&output)
    );
    // The same threshold written out explicitly matches.
    assert_eq!(code(&p.compare(&["--threshold", "10"])), 0);
}

#[test]
fn an_existing_baseline_is_never_silently_rewritten() {
    let p = one_complex();
    p.write_baseline(&[]);
    let before = fs::read(p.baseline()).unwrap();
    p.write("A.cs", &class("A", &[("Complex", 3)]));
    let baseline = p.baseline();
    let args = ["--write-baseline", baseline.to_str().unwrap()];
    let refused = p.run(&args);
    assert_eq!(code(&refused), 2);
    assert!(
        stderr(&refused).contains("already exists"),
        "{}",
        stderr(&refused)
    );
    assert!(stderr(&refused).contains("--overwrite-baseline"));
    assert_eq!(fs::read(p.baseline()).unwrap(), before);
    // Explicit permission replaces it, atomically and without leftovers.
    let replaced = p.run(&[args[0], args[1], "--overwrite-baseline"]);
    assert_eq!(code(&replaced), 0);
    assert_ne!(fs::read(p.baseline()).unwrap(), before);
    let mut names: Vec<_> = fs::read_dir(p.root())
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    assert_eq!(names, ["A.cs", "baseline.json"]);
    // Rewriting unchanged code gives identical bytes.
    let again = fs::read(p.baseline()).unwrap();
    p.run(&[args[0], args[1], "--overwrite-baseline"]);
    assert_eq!(fs::read(p.baseline()).unwrap(), again);
}

#[test]
fn the_baseline_file_records_failing_callables_deterministically() {
    let p = Project::new(&[
        ("b/B.cs", class("B", &[("Zed", 2)])),
        (
            "a/A.cs",
            class("A", &[("Fine", 0), ("Late", 3), ("Early", 2)]),
        ),
    ]);
    p.write_baseline(&[]);
    let file: Value = serde_json::from_slice(&fs::read(p.baseline()).unwrap()).unwrap();
    assert_eq!(file["version"], 1);
    assert_eq!(file["threshold"], 10.0);
    let entries = file["entries"].as_array().unwrap();
    let order: Vec<(&str, &str)> = entries
        .iter()
        .map(|e| {
            (
                e["file"].as_str().unwrap(),
                e["signature"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        order,
        [
            ("a/A.cs", "Late(int)"),
            ("a/A.cs", "Early(int)"),
            ("b/B.cs", "Zed(int)")
        ]
    );
    let late = &entries[0];
    assert_eq!(late["kind"], "method_declaration");
    assert_eq!(late["scope"], "A");
    assert_eq!(late["occurrence"], 0);
    assert_eq!(late["complexity"], 4);
    assert_eq!(late["crap"], 20.0);
    assert!(late["coverage"].is_null());
    assert_eq!(late["line"], 7);
    let fingerprint = late["fingerprint"].as_str().unwrap();
    assert_eq!(fingerprint.len(), 64);
    // The fingerprint is internal to baselines and absent from the JSON report.
    let report = p.json(&[]);
    assert!(report["functions"][0].get("fingerprint").is_none());
    let text = fs::read_to_string(p.baseline()).unwrap();
    assert!(text.ends_with("}\n") && text.starts_with("{\n  \"version\": 1,"));
}

#[test]
fn completeness_policies_are_never_suppressed_by_a_baseline() {
    let p = one_complex();
    p.write_baseline(&[]);
    let output = p.compare(&["--require-coverage"]);
    assert_eq!(code(&output), 1);
    assert!(stdout(&output).contains("Coverage missing for one or more callables."));
    // Nothing is new or worse, so only the completeness policy failed.
    assert!(stdout(&output).contains("baseline: 0 new, 0 worse, 1 unchanged"));
    let empty = Project::new(&[]);
    fs::write(
        empty.baseline(),
        r#"{"version":1,"threshold":10.0,"entries":[]}"#,
    )
    .unwrap();
    assert_eq!(code(&empty.compare(&[])), 0);
    assert_eq!(code(&empty.compare(&["--fail-on-empty"])), 1);
    // Writing a baseline exits 0 unless a completeness policy fails; it still writes.
    let q = one_complex();
    let output = q.write_baseline(&["--require-coverage"]);
    assert_eq!(code(&output), 1);
    assert!(q.baseline().exists());
    let r = Project::new(&[]);
    assert_eq!(code(&r.write_baseline(&["--fail-on-empty"])), 1);
    assert_eq!(code(&r.write_baseline(&["--overwrite-baseline"])), 0);
}

#[test]
fn the_config_key_resolves_against_the_config_file_and_can_be_overridden() {
    let p = one_complex();
    p.write_baseline(&[]);
    fs::create_dir(p.root().join("ci")).unwrap();
    fs::rename(p.baseline(), p.root().join("ci/base.json")).unwrap();
    p.write(
        "dotnet-crap.json",
        r#"{"version":1,"baseline":"ci/base.json"}"#,
    );
    // Run from an unrelated working directory: the path is the config's, not the cwd's.
    let from_elsewhere = Command::new(env!("CARGO_BIN_EXE_dotnet-crap"))
        .current_dir(std::env::temp_dir())
        .arg("--root")
        .arg(p.root())
        .output()
        .unwrap();
    assert_eq!(code(&from_elsewhere), 0, "{}", stderr(&from_elsewhere));
    assert!(stdout(&from_elsewhere).contains("[unchanged]"));
    // `--no-baseline` brings the violation back.
    assert_eq!(code(&p.run(&["--no-baseline"])), 1);
    // The command line wins over the config.
    let other = p.root().join("other.json");
    fs::write(&other, r#"{"version":1,"threshold":10.0,"entries":[]}"#).unwrap();
    let output = p.run(&["--baseline", other.to_str().unwrap()]);
    assert_eq!(code(&output), 1);
    assert!(stdout(&output).contains("[new]"));
    // Writing replaces comparing, even a configured baseline, and may refresh it.
    let written = p.run(&[
        "--write-baseline",
        p.root().join("ci/base.json").to_str().unwrap(),
        "--overwrite-baseline",
    ]);
    assert_eq!(code(&written), 0, "{}", stderr(&written));
    // Without a `--config`, the key is not read from a file that is not found.
    assert_eq!(code(&p.run(&["--no-config"])), 1);
}

#[test]
fn unusable_baselines_and_flag_combinations_are_input_errors() {
    let p = one_complex();
    let baseline = p.baseline();
    let path = baseline.to_str().unwrap();
    // Malformed, versioned wrongly, unknown fields and missing files name the file.
    for (text, expected) in [
        ("{ nope", "invalid baseline"),
        (
            r#"{"version":2,"threshold":10,"entries":[]}"#,
            "unsupported baseline version 2",
        ),
        (
            r#"{"version":1,"threshold":10,"entries":[],"x":1}"#,
            "unknown field",
        ),
    ] {
        fs::write(&baseline, text).unwrap();
        let output = p.compare(&[]);
        assert_eq!(code(&output), 2, "{text}");
        let message = stderr(&output);
        assert!(
            message.contains(expected) && message.contains("baseline.json"),
            "{message}"
        );
    }
    fs::remove_file(&baseline).unwrap();
    assert_eq!(code(&p.compare(&[])), 2);
    // A baseline cannot be a C# source, and `--write-baseline` cannot overwrite one.
    let cs = p.root().join("B.cs");
    assert_eq!(code(&p.run(&["--baseline", cs.to_str().unwrap()])), 2);
    let output = p.run(&[
        "--write-baseline",
        cs.to_str().unwrap(),
        "--overwrite-baseline",
    ]);
    assert_eq!(code(&output), 2);
    assert!(stderr(&output).contains("cannot overwrite C# source"));
    // Flag conflicts and requirements are rejected up front.
    let other = p.root().join("o.json");
    let other = other.to_str().unwrap();
    for args in [
        vec!["--baseline", path, "--write-baseline", other],
        vec!["--baseline", path, "--no-baseline"],
        vec!["--overwrite-baseline"],
        vec!["--baseline-move", "a=b"],
        vec!["--baseline-move", "nonsense", "--baseline", path],
        vec!["--output", other, "--write-baseline", other],
    ] {
        let output = p.run(&args);
        assert_eq!(code(&output), 2, "{args:?}: {}", stderr(&output));
    }
    assert!(!p.root().join("o.json").exists());
    // A report cannot replace the baseline it was compared with.
    p.write_baseline(&[]);
    let output = p.compare(&["--output", path]);
    assert_eq!(code(&output), 2);
    assert!(
        stderr(&output).contains("aliases an input"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn json_reports_carry_baseline_states_and_counts_only_with_a_baseline() {
    let p = Project::new(&[(
        "A.cs",
        class("A", &[("Keep", 2), ("Worse", 2), ("Fixed", 2)]),
    )]);
    let plain = p.json(&[]);
    assert!(plain.get("baseline").is_none());
    assert!(plain["functions"]
        .as_array()
        .unwrap()
        .iter()
        .all(|f| f.get("baseline").is_none()));
    assert_eq!(plain["functions"][0]["scope"], "A");
    assert_eq!(plain["functions"][0]["signature"], "Keep(int)");
    p.write_baseline(&[]);
    p.write(
        "A.cs",
        &class("A", &[("Keep", 2), ("Worse", 3), ("Added", 4), ("Pass", 0)]),
    );
    let report = p.json(&["--baseline", p.baseline().to_str().unwrap()]);
    let by_name = |name: &str| {
        report["functions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["name"] == name)
            .unwrap()
            .clone()
    };
    assert_eq!(by_name("Keep")["baseline"], "unchanged");
    assert_eq!(by_name("Worse")["baseline"], "worse");
    assert_eq!(by_name("Added")["baseline"], "new");
    assert!(by_name("Pass").get("baseline").is_none());
    assert_eq!(
        report["baseline"],
        serde_json::json!({"new":1,"worse":1,"unchanged":1,"improved":0,"resolved":1})
    );
    assert_eq!(report["failed"], true);
}

#[test]
#[cfg(unix)]
fn sarif_resolves_only_the_selected_literal_backslash_path() {
    let literal = r"src\C.cs";
    let p = Project::new(&[
        (literal, class("Literal", &[("Fixed", 2)])),
        ("src/C.cs", class("Nested", &[("StillFailing", 2)])),
    ]);
    p.write_baseline(&[]);
    p.write(literal, &class("Literal", &[("Fixed", 0)]));

    let output = p.compare(&["--format", "sarif", literal]);
    assert_eq!(code(&output), 0, "{output:?}");
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    let results = report["runs"][0]["results"].as_array().unwrap();
    assert_eq!(results.len(), 1, "{results:?}");
    assert_eq!(results[0]["baselineState"], "absent");
    assert_eq!(
        results[0]["locations"][0]["physicalLocation"]["artifactLocation"]["uri"],
        "src%5CC.cs"
    );
}

#[test]
fn sarif_marks_baseline_states_and_reports_resolved_entries_as_absent() {
    let p = Project::new(&[(
        "A.cs",
        class(
            "A",
            &[("Keep", 2), ("Worse", 2), ("Fixed", 2), ("Better", 4)],
        ),
    )]);
    p.write_baseline(&[]);
    p.write(
        "A.cs",
        &class(
            "A",
            &[("Keep", 2), ("Worse", 3), ("Added", 4), ("Better", 3)],
        ),
    );
    let sarif = |args: &[&str]| -> Value {
        let mut all = vec!["--format", "sarif"];
        all.extend_from_slice(args);
        serde_json::from_slice(&p.run(&all).stdout).unwrap()
    };
    let report = sarif(&["--baseline", p.baseline().to_str().unwrap()]);
    let results = report["runs"][0]["results"].as_array().unwrap();
    let find = |needle: &str| {
        results
            .iter()
            .find(|r| r["message"]["text"].as_str().unwrap().starts_with(needle))
            .unwrap()
            .clone()
    };
    assert_eq!(find("Keep ")["baselineState"], "unchanged");
    assert_eq!(find("Better ")["baselineState"], "unchanged");
    assert_eq!(find("Worse ")["baselineState"], "updated");
    assert_eq!(find("Added ")["baselineState"], "new");
    for failing in ["Keep ", "Better ", "Worse ", "Added "] {
        assert_eq!(find(failing)["level"], "error");
        assert!(find(failing).get("kind").is_none());
    }
    let absent = find("Fixed(int)");
    assert_eq!(absent["baselineState"], "absent");
    assert_eq!(absent["kind"], "pass");
    assert_eq!(absent["level"], "none");
    assert_eq!(absent["ruleId"], "DNCRAP001");
    assert_eq!(
        absent["locations"][0]["physicalLocation"]["artifactLocation"]["uri"],
        "A.cs"
    );
    assert!(
        absent["partialFingerprints"]["dotnetCrap/v1"]
            .as_str()
            .unwrap()
            .len()
            == 64
    );
    assert_eq!(results.len(), 5);
    // Existing fingerprints are the same with and without a baseline.
    let plain = sarif(&[]);
    let fingerprint = |report: &Value, needle: &str| {
        report["runs"][0]["results"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["message"]["text"].as_str().unwrap().starts_with(needle))
            .unwrap()["partialFingerprints"]["dotnetCrap/v1"]
            .clone()
    };
    assert_eq!(
        fingerprint(&plain, "Worse "),
        fingerprint(&report, "Worse ")
    );
    // Without a baseline no result has a state; completeness results never do.
    assert!(plain["runs"][0]["results"]
        .as_array()
        .unwrap()
        .iter()
        .all(|r| r.get("baselineState").is_none()));
    let completeness = sarif(&[
        "--baseline",
        p.baseline().to_str().unwrap(),
        "--require-coverage",
    ]);
    let missing: Vec<&Value> = completeness["runs"][0]["results"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["ruleId"] == "DNCRAP002")
        .collect();
    assert_eq!(missing.len(), 4);
    assert!(missing.iter().all(|r| r.get("baselineState").is_none()));
}

#[test]
fn the_library_scans_against_a_baseline_and_writes_one() {
    use dotnet_crap::{scan, BaselineState, ErrorKind, ReportFormat, ScanOptions};
    let p = one_complex();
    let options = ScanOptions {
        root: p.root().to_path_buf(),
        ..Default::default()
    };
    let result = scan(&options).unwrap();
    assert!(result.report().failed);
    assert_eq!(result.report().baseline, None);
    assert!(!result.completeness_failed());
    // Refused until told to replace; the count is the failing callables.
    assert_eq!(result.write_baseline(&p.baseline(), false).unwrap(), 1);
    let refused = result.write_baseline(&p.baseline(), false).unwrap_err();
    assert_eq!(refused.kind(), ErrorKind::Output);
    assert_eq!(result.write_baseline(&p.baseline(), true).unwrap(), 1);
    // The baseline is an input of the scan that read it, so it cannot be replaced from it.
    let compared = scan(&ScanOptions {
        baseline: Some(p.baseline()),
        ..options.clone()
    })
    .unwrap();
    assert!(!compared.report().failed);
    let summary = compared.report().baseline.unwrap();
    assert_eq!(
        (summary.unchanged, summary.new, summary.resolved),
        (1, 0, 0)
    );
    assert_eq!(
        compared.report().functions[1].baseline,
        Some(BaselineState::Unchanged)
    );
    assert!(compared.input_paths().contains(&p.baseline()));
    let error = compared.write_baseline(&p.baseline(), true).unwrap_err();
    assert!(error.to_string().contains("aliases an input"));
    assert!(compared
        .render(ReportFormat::Text)
        .unwrap()
        .contains("baseline: 0 new"));
    // `accept_scores` leaves only completeness policies.
    let mut strict = scan(&ScanOptions {
        require_coverage: true,
        ..options.clone()
    })
    .unwrap();
    strict.accept_scores();
    assert!(strict.report().failed && strict.completeness_failed());
    let mut lax = scan(&options).unwrap();
    lax.accept_scores();
    assert!(!lax.report().failed);
    // Moves need a baseline; a mismatching threshold and a bad file are input errors.
    let moves = vec![("a".to_owned(), "b".to_owned())];
    let error = scan(&ScanOptions {
        baseline_moves: moves.clone(),
        ..options.clone()
    })
    .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::InvalidInput);
    scan(&ScanOptions {
        baseline: Some(p.baseline()),
        baseline_moves: moves,
        ..options.clone()
    })
    .unwrap();
    let error = scan(&ScanOptions {
        baseline: Some(p.baseline()),
        threshold: 3.0,
        ..options
    })
    .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::InvalidInput);
}
