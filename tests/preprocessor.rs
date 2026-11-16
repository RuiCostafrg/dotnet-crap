//! Conditional-compilation fallback for files that do not parse as written.
use dotnet_crap::{
    collector::{collect, CollectOptions},
    scan, ErrorKind, ReportFormat, ScanOptions,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{ffi::OsString, fs, path::Path, process::Command};

/// Serilog-style `#if` that splits a method declaration from its body.
const SPLIT: &str = "class C\n{\n    Task OnEmptyBatchAsync()\n#if X\n    {\n        if (Ready) return Task.CompletedTask;\n        return Next();\n    }\n#else\n    ;\n#endif\n    int G(int a) { return a > 0 ? 1 : 0; }\n}\n";

fn tree(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (name, text) in files {
        fs::write(dir.path().join(name), text).unwrap();
    }
    dir
}

fn options(root: &Path, defines: &[&str]) -> ScanOptions {
    ScanOptions {
        root: root.to_path_buf(),
        threshold: 1e9,
        defines: defines.iter().map(|s| (*s).to_owned()).collect(),
        ..ScanOptions::default()
    }
}

/// `(line, name, complexity)` of every reported callable.
fn callables(root: &Path, defines: &[&str]) -> Vec<(usize, String, u32)> {
    scan(&options(root, defines))
        .unwrap()
        .report()
        .functions
        .iter()
        .map(|f| (f.line, f.name.clone(), f.complexity))
        .collect()
}

fn cli(root: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_dotnet-crap"))
        .arg("--root")
        .arg(root)
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn split_declaration_scans_with_and_without_its_symbol() {
    let dir = tree(&[("C.cs", SPLIT)]);
    assert_eq!(callables(dir.path(), &[]), [(12, "G".to_owned(), 2)]);
    assert_eq!(
        callables(dir.path(), &["X"]),
        [
            (3, "OnEmptyBatchAsync".to_owned(), 2),
            (12, "G".to_owned(), 2)
        ]
    );
    // Unrelated symbols change nothing; the hash stays over the original bytes.
    let result = scan(&options(dir.path(), &["Y", "Z"])).unwrap();
    assert_eq!(result.report().functions.len(), 1);
    let json: Value = serde_json::from_str(&result.render(ReportFormat::Json).unwrap()).unwrap();
    assert_eq!(json["functions"][0]["line"], 12);
    result.verify_sources().unwrap();
}

#[test]
fn trailing_directive_and_file_based_app_lines_scan() {
    let dir = tree(&[
        (
            "Last.cs",
            "#pragma warning disable 618\nclass L { int F(int x) { return x; } }\n#pragma warning restore 618",
        ),
        (
            "App.cs",
            "#:sdk Microsoft.NET.Sdk.Web\n#:package Humanizer@2.14.1\n#:property LangVersion=preview\nclass App { int Run(bool b) { return b ? 1 : 0; } }\n",
        ),
        (
            "Region.cs",
            "class R\n{\n    int H() => 1;\n}\n#region Tail\n#endregion",
        ),
    ]);
    assert_eq!(
        callables(dir.path(), &[]),
        [
            (4, "Run".to_owned(), 2),
            (2, "F".to_owned(), 1),
            (3, "H".to_owned(), 1)
        ]
    );
}

#[test]
fn files_that_parse_as_written_ignore_defines() {
    // Both branches are scored as written, and `#if A && B` keeps its
    // decision point, exactly as before the fallback existed.
    let text = "class C\n{\n#if A\n    int F() { return 1; }\n#else\n    int F() { return 2; }\n#endif\n    int G()\n    {\n#if A && B\n        return 1;\n#endif\n        return 0;\n    }\n}\n";
    let dir = tree(&[("C.cs", text)]);
    let plain = scan(&options(dir.path(), &[])).unwrap();
    let defined = scan(&options(dir.path(), &["A", "B"])).unwrap();
    for format in [ReportFormat::Json, ReportFormat::Sarif, ReportFormat::Text] {
        assert_eq!(
            plain.render(format).unwrap(),
            defined.render(format).unwrap()
        );
    }
    assert_eq!(
        callables(dir.path(), &["A"]),
        [
            (4, "F".to_owned(), 1),
            (6, "F".to_owned(), 1),
            (8, "G".to_owned(), 2)
        ]
    );
}

#[test]
fn unresolvable_files_keep_the_original_syntax_error() {
    for text in [
        // Still invalid once resolved.
        "#region R\nclass {\n#endregion\n",
        // Malformed directives: unbalanced, bad expression, unknown.
        "class C {\n int F()\n#if X\n { return 1; }\n",
        "class C {\n int F()\n#if X &&\n { return 1; }\n#else\n ;\n#endif\n}\n",
        "class C {\n int F()\n#ifdef X\n { return 1; }\n#endif\n}\n",
        // Nothing to resolve: a grammar limitation.
        "class C { void M() { var with = 1; } }\n",
    ] {
        let dir = tree(&[("Bad.cs", text)]);
        for defines in [&[][..], &["X"]] {
            let error = scan(&options(dir.path(), defines)).unwrap_err();
            assert_eq!(error.kind(), ErrorKind::Syntax, "{text:?}");
            assert_eq!(error.to_string(), "invalid C# syntax in Bad.cs");
        }
        let output = cli(dir.path(), &["--define", "X"]);
        assert_eq!(output.status.code(), Some(2));
        assert_eq!(
            String::from_utf8_lossy(&output.stderr),
            "error: invalid C# syntax in Bad.cs\n"
        );
    }
}

#[test]
fn resolved_files_bind_coverage_by_original_lines_and_hash() {
    let dir = tree(&[("C.cs", SPLIT)]);
    let hash = hex::encode(Sha256::digest(SPLIT.as_bytes()));
    let coverage = dir.path().join("coverage.json");
    fs::write(
        &coverage,
        format!(
            r#"{{"version":1,"files":{{"C.cs":{{"sha256":"{hash}","lines":{{"6":1,"7":0,"12":1,"13":0}}}}}}}}"#
        ),
    )
    .unwrap();
    let scan_with = |defines: &[&str]| {
        scan(&ScanOptions {
            coverage: Some(coverage.clone()),
            require_coverage: true,
            ..options(dir.path(), defines)
        })
        .unwrap()
    };
    let result = scan_with(&["X"]);
    let functions = &result.report().functions;
    assert_eq!(functions.len(), 2);
    // Lines 6 and 7 of the original file: one of two hit.
    assert_eq!((functions[0].line, functions[0].coverage), (3, Some(0.5)));
    assert_eq!((functions[1].line, functions[1].coverage), (12, Some(1.0)));
    assert!(result.report().coverage_complete);
    assert!(!result.report().failed);
    let result = scan_with(&[]);
    assert_eq!(result.report().functions.len(), 1);
    assert_eq!(result.report().functions[0].coverage, Some(1.0));

    // The CLI binds the same evidence through `--coverage`.
    let output = cli(
        dir.path(),
        &[
            "--define",
            "X",
            "--coverage",
            coverage.to_str().unwrap(),
            "--require-coverage",
            "--format",
            "json",
        ],
    );
    assert_eq!(output.status.code(), Some(0));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["functions"][0]["coverage"], 0.5);

    // A line past the end of the original file is still rejected.
    fs::write(
        &coverage,
        format!(r#"{{"version":1,"files":{{"C.cs":{{"sha256":"{hash}","lines":{{"14":1}}}}}}}}"#),
    )
    .unwrap();
    let error = scan(&ScanOptions {
        coverage: Some(coverage.clone()),
        ..options(dir.path(), &["X"])
    })
    .unwrap_err();
    assert_eq!(error.to_string(), "coverage line outside C.cs");
}

#[test]
fn invalid_symbols_are_input_errors() {
    let dir = tree(&[("C.cs", SPLIT)]);
    for bad in ["", "1X", "A-B", "true", "false", "A B"] {
        let error = scan(&options(dir.path(), &["X", bad])).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::InvalidInput, "{bad:?}");
        assert_eq!(
            error.to_string(),
            format!("invalid conditional compilation symbol {bad:?}: use a letter or `_` followed by letters, digits or `_`, other than `true` and `false`")
        );
    }
    for args in [
        &["--define", "1X"][..],
        &["--define", "X,"],
        &["--define", "A;-B"],
        &["--define=true"],
    ] {
        let output = cli(dir.path(), args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(String::from_utf8_lossy(&output.stderr)
            .starts_with("error: invalid conditional compilation symbol"));
    }
}

#[test]
fn root_and_path_errors_come_before_symbol_errors() {
    let dir = tree(&[("C.cs", SPLIT)]);
    let missing = dir.path().join("nope");
    let error = scan(&options(&missing, &["1X"])).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::InvalidInput);
    assert_eq!(error.to_string(), "invalid root");
    let output = cli(&missing, &["--define", "1X"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).starts_with("error: invalid root"));
    let path_error = scan(&ScanOptions {
        paths: vec!["Missing.cs".into()],
        ..options(dir.path(), &[])
    })
    .unwrap_err()
    .to_string();
    let error = scan(&ScanOptions {
        paths: vec!["Missing.cs".into()],
        ..options(dir.path(), &["1X"])
    })
    .unwrap_err();
    assert_eq!(error.to_string(), path_error);
    // `collect` scans before running anything, with the same order.
    let error = collect(&CollectOptions {
        root: missing,
        paths: vec![],
        output: dir.path().join("out.json"),
        command: ["python3", "-c", "", "{results}"]
            .into_iter()
            .map(OsString::from)
            .collect(),
        expected_reports: 1,
        timeout: std::time::Duration::from_secs(60),
        jobs: None,
        defines: vec!["1X".to_owned()],
    })
    .unwrap_err();
    assert_eq!(error.to_string(), "invalid root");
}

#[test]
fn cli_defines_repeat_and_split_on_commas_and_semicolons() {
    let dir = tree(&[(
        "C.cs",
        "class C {\n int F()\n#if A && B && C\n { return 1; }\n#else\n ;\n#endif\n}\n",
    )]);
    let names = |args: &[&str]| {
        let output = cli(dir.path(), &[args, &["--format", "json"]].concat());
        assert_eq!(output.status.code(), Some(0), "{args:?}");
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        report["functions"].as_array().unwrap().len()
    };
    assert_eq!(names(&[]), 0);
    assert_eq!(names(&["--define", "A", "--define", "B"]), 0);
    assert_eq!(
        names(&["--define", "A", "--define", "B", "--define", "C"]),
        1
    );
    assert_eq!(names(&["--define", "A,B;C"]), 1);
    assert_eq!(names(&["--define", "A, B ; C"]), 1);
    assert_eq!(names(&["--define=A;B", "--define", "C"]), 1);
}

const REPORT: &str = r#"import pathlib,sys
p=pathlib.Path(sys.argv[1]); (p/'run').mkdir()
(p/'run'/'coverage.cobertura.xml').write_text('<coverage><sources/><packages><package><classes><class filename="C.cs"><lines><line number="6" hits="1"/><line number="7" hits="0"/></lines></class></classes></package></packages></coverage>')
"#;

#[test]
fn collect_snapshots_files_that_need_the_fallback() {
    let dir = tree(&[("C.cs", SPLIT)]);
    let output = dir.path().join("native.json");
    let mut options = CollectOptions {
        root: dir.path().to_owned(),
        paths: vec![],
        output: output.clone(),
        command: ["python3", "-c", REPORT, "{results}"]
            .into_iter()
            .map(OsString::from)
            .collect(),
        expected_reports: 1,
        timeout: std::time::Duration::from_secs(60),
        jobs: None,
        defines: vec!["1X".to_owned()],
    };
    // An invalid symbol is rejected before the test command runs: the
    // command would create the marker.
    let marker = dir.path().join("ran");
    let touch = format!(
        "import pathlib; pathlib.Path({:?}).touch()",
        marker.to_str().unwrap()
    );
    let command = std::mem::replace(
        &mut options.command,
        ["python3", "-c", &touch, "{results}"]
            .into_iter()
            .map(OsString::from)
            .collect(),
    );
    let error = collect(&options).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::InvalidInput);
    assert!(!output.exists());
    assert!(!marker.exists());
    options.defines = vec!["X".to_owned()];
    options.command = command;
    collect(&options).unwrap();
    let native: Value = serde_json::from_slice(&fs::read(&output).unwrap()).unwrap();
    assert_eq!(
        native["files"]["C.cs"]["sha256"],
        hex::encode(Sha256::digest(SPLIT.as_bytes()))
    );
    let result = scan(&ScanOptions {
        coverage: Some(output.clone()),
        ..self::options(dir.path(), &["X"])
    })
    .unwrap();
    assert_eq!(result.report().functions[0].coverage, Some(0.5));

    // The CLI passes `--define` to collection and rejects invalid symbols
    // before the test command runs.
    let run = |define: &str, out: &str, script: &str| {
        Command::new(env!("CARGO_BIN_EXE_dotnet-crap"))
            .arg("collect")
            .arg("--root")
            .arg(dir.path())
            .arg("--output")
            .arg(dir.path().join(out))
            .args([
                "--define",
                define,
                "--",
                "python3",
                "-c",
                script,
                "{results}",
            ])
            .output()
            .unwrap()
    };
    let bad = run("A,1X", "bad.json", &touch);
    assert_eq!(bad.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&bad.stderr)
        .contains("invalid conditional compilation symbol \"1X\""));
    assert!(!dir.path().join("bad.json").exists());
    assert!(!marker.exists());
    let good = run("X", "good.json", REPORT);
    assert_eq!(
        good.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&good.stderr)
    );
    assert_eq!(
        fs::read(dir.path().join("good.json")).unwrap(),
        fs::read(&output).unwrap()
    );
}

/// Assert that `text` in `Bad.cs` reports the original syntax error with each
/// symbol set, through the library and the CLI.
fn assert_declines(text: &str, defines: &[&[&str]]) {
    let dir = tree(&[("Bad.cs", text)]);
    for names in defines {
        let error = scan(&options(dir.path(), names)).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Syntax, "{text:?} {names:?}");
        assert_eq!(error.to_string(), "invalid C# syntax in Bad.cs");
        let args: Vec<&str> = names.iter().flat_map(|n| ["--define", n]).collect();
        let output = cli(dir.path(), &args);
        assert_eq!(output.status.code(), Some(2), "{text:?} {names:?}");
        assert_eq!(
            String::from_utf8_lossy(&output.stderr),
            "error: invalid C# syntax in Bad.cs\n"
        );
    }
}

#[test]
fn directives_inside_comments_and_strings_are_not_directives() {
    // A `#define` inside a verbatim string defines nothing.
    let text = "class C\n{\n    string S = @\"\n#define FEATURE\n\";\n    int F()\n#if FEATURE\n    {\n        if (S.Length > 0) return 1;\n        return 0;\n    }\n#else\n    => 0;\n#endif\n}\n";
    let dir = tree(&[("C.cs", text)]);
    assert_eq!(callables(dir.path(), &[]), [(6, "F".to_owned(), 1)]);
    assert_eq!(
        callables(dir.path(), &["FEATURE"]),
        [(6, "F".to_owned(), 2)]
    );

    // `#if false` and `#endif` in two block comments keep the `if` between.
    let straddle = "class C\n{\n    int F(int x)\n    {\n        /*\n#if false\n        */\n        if (x > 0) return 1;\n        /*\n#endif\n        */\n        return 0;\n    }\n}\n#pragma warning restore 618";
    // The same straddle around a whole method keeps the method.
    let around = "class D\n{\n    /*\n#if false\n    */\n    int F(int x) { return x > 0 ? 1 : 0; }\n    /*\n#endif\n    */\n}\n#pragma warning restore 618";
    // An unpaired `#if` in a comment opens no group.
    let unpaired = "class E\n{\n    /*\n#if DEBUG\n    */\n    int F(int x) { return x > 0 ? 1 : 0; }\n}\n#pragma warning restore 618";
    // A raw string and an interpolated verbatim string hide `#` lines too.
    let strings = "class G\n{\n    string R = \"\"\"\n        #if false\n        \"\"\";\n    string V(int x) => $@\"{x}\n#else\n{(x > 0 ? 1 : 0)}\";\n}\n#pragma warning restore 618";
    let dir = tree(&[
        ("A.cs", straddle),
        ("B.cs", around),
        ("E.cs", unpaired),
        ("G.cs", strings),
    ]);
    for names in [&[][..], &["DEBUG"]] {
        assert_eq!(
            callables(dir.path(), names),
            [
                (3, "F".to_owned(), 2),
                (6, "F".to_owned(), 2),
                (6, "F".to_owned(), 2),
                (6, "V".to_owned(), 2)
            ]
        );
    }
    // A `#` line inside an interpolation hole is not followed.
    assert_declines(
        "class H\n{\n    string V(int x) => $@\"{\n#if false\nx}\";\n}\n#pragma warning restore 618",
        &[&[]],
    );
}

#[test]
fn directives_the_compiler_rejects_decline() {
    // An active `#error`; inactive, it is skipped.
    let error =
        "class C {\n int F()\n#if X\n#error not supported\n { return 1; }\n#else\n ;\n#endif\n}\n";
    assert_declines(error, &[&["X"]]);
    let dir = tree(&[("C.cs", error)]);
    assert_eq!(callables(dir.path(), &[]), []);
    for text in [
        // `#define` after the first token.
        "class C {\n int F()\n#define X\n#if X\n { return 1; }\n#else\n ;\n#endif\n}\n",
        // Unknown directives, also in inactive text.
        "class C {\n int F()\n#if X\n#warn x\n { return 1; }\n#else\n ;\n#endif\n}\n",
        // An invalid expression in inactive text.
        "class C {\n int F()\n#if false\n#elif X &&\n { return 1; }\n#else\n ;\n#endif\n}\n",
        // Unbalanced or crossed regions.
        "#region R\nclass C {\n int F()\n#if X\n { return 1; }\n#else\n ;\n#endif\n}\n",
        "class C {\n int F()\n#if X\n#region R\n { return 1; }\n#else\n ;\n#endregion\n#endif\n}\n",
        // A lone carriage return is a line break the fallback does not split on.
        "class C {\r int F()\n#if X\n { return 1; }\n#else\n ;\n#endif\n}\n",
    ] {
        assert_declines(text, &[&[], &["X"]]);
    }
}

#[test]
fn expression_depth_is_capped_at_256() {
    let text = |depth: usize| {
        format!(
            "class C {{\n int F()\n#if {}X{}\n {{ return 1; }}\n#else\n ;\n#endif\n}}\n",
            "(".repeat(depth),
            ")".repeat(depth)
        )
    };
    let dir = tree(&[("C.cs", &text(256))]);
    let output = cli(dir.path(), &["--define", "X", "--format", "json"]);
    assert_eq!(output.status.code(), Some(0));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["functions"][0]["name"], "F");
    assert_declines(&text(257), &[&["X"]]);
}

#[test]
fn byte_order_marks_are_not_part_of_the_first_line() {
    let text = "#pragma warning disable 1\nclass C { int F(bool b) { return b ? 1 : 0; } }\n#pragma warning restore 1";
    let mut utf8 = b"\xef\xbb\xbf".to_vec();
    utf8.extend_from_slice(text.as_bytes());
    let mut utf16 = vec![0xff, 0xfe];
    utf16.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("A.cs"), &utf8).unwrap();
    fs::write(dir.path().join("B.cs"), &utf16).unwrap();
    assert_eq!(
        callables(dir.path(), &[]),
        [(2, "F".to_owned(), 2), (2, "F".to_owned(), 2)]
    );
    scan(&options(dir.path(), &[]))
        .unwrap()
        .verify_sources()
        .unwrap();
}
