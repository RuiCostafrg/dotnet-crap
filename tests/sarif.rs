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
    decode(output)
}
fn decode(output: std::process::Output) -> (i32, Value) {
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

/// Express the canonical path as a decoded file-URI tail, with UNC hosts in the authority.
fn expected_file_uri_tail(canonical: &str, windows: bool) -> String {
    if windows {
        let normalized = canonical
            .strip_prefix(r"\\?\")
            .unwrap_or(canonical)
            .replace('\\', "/");
        let path = normalized.trim_end_matches('/');
        if let Some(unc) = path
            .strip_prefix("UNC/")
            .or_else(|| path.strip_prefix("//"))
        {
            format!("{unc}/")
        } else {
            format!("/{path}/")
        }
    } else {
        format!("{}/", canonical.trim_end_matches('/'))
    }
}

#[test]
/// Check local-drive and UNC authority expectations on every host, without requiring a share.
fn file_uri_expectations_distinguish_drive_paths_and_unc_authorities() {
    for (path, expected) in [
        (r"C:\source root", "/C:/source root/"),
        (r"\\?\C:\source root", "/C:/source root/"),
        (r"\\server\share\source root", "server/share/source root/"),
        (
            r"\\?\UNC\server\share\source root",
            "server/share/source root/",
        ),
    ] {
        assert_eq!(expected_file_uri_tail(path, true), expected);
    }
    assert_eq!(expected_file_uri_tail("/", false), "/");
    assert_eq!(
        expected_file_uri_tail(r"/source\root", false),
        "/source\\root/"
    );
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
/// Verify encoded Unicode paths, canonical root identity and finding fingerprints through the CLI.
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
    assert!(base.starts_with("file://"));
    assert!(base.ends_with('/'));
    // Decode URI octets independently and compare the exact canonical root.
    let mut decoded = Vec::new();
    let mut bytes = base
        .strip_prefix("file://")
        .unwrap()
        .as_bytes()
        .iter()
        .copied();
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            let digits = [bytes.next().unwrap(), bytes.next().unwrap()];
            decoded.push(u8::from_str_radix(std::str::from_utf8(&digits).unwrap(), 16).unwrap());
        } else {
            decoded.push(byte);
        }
    }
    let actual = String::from_utf8(decoded).unwrap();
    let canonical = dir.path().canonicalize().unwrap();
    let expected = expected_file_uri_tail(canonical.to_str().unwrap(), cfg!(windows));
    assert_eq!(actual, expected);
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
    let (code, covered) = decode(
        Command::new(env!("CARGO_BIN_EXE_dotnet-crap"))
            .arg("--root")
            .arg(dir.path())
            .args(["--format", "sarif", "--coverage"])
            .arg(&coverage)
            .args(["--require-coverage", "--threshold", "0"])
            .output()
            .unwrap(),
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

#[cfg(unix)]
#[test]
fn special_source_names_are_encoded_without_coverage_and_rejected_with_it() {
    for (name, uri) in [
        (r"literal\name.cs", "literal%5Cname.cs"),
        ("colon:name.cs", "colon%3Aname.cs"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(name), "class C { void F() {} }").unwrap();
        let (code, report) = scan(dir.path(), &["--threshold", "0"]);
        assert_eq!(code, 1);
        assert_eq!(results(&report).len(), 1);
        assert_eq!(
            results(&report)[0]["locations"][0]["physicalLocation"]["artifactLocation"]["uri"],
            uri
        );
        let output = Command::new(env!("CARGO_BIN_EXE_dotnet-crap"))
            .arg("--root")
            .arg(dir.path())
            .args(["--format", "json"])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(0));
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["functions"][0]["file"], name);

        let coverage = dir.path().join("coverage.json");
        fs::write(&coverage, r#"{"version":1,"files":{}}"#).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_dotnet-crap"))
            .arg("--root")
            .arg(dir.path())
            .args(["--format", "sarif", "--coverage"])
            .arg(&coverage)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("unsupported by native coverage"));
    }
}

// APFS rejects invalid UTF-8 path components before the CLI can inspect them.
#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn non_utf8_source_root_is_rejected_instead_of_fabricating_a_uri() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    let dir = tempfile::tempdir().unwrap();
    let root = dir
        .path()
        .join(OsString::from_vec(b"invalid-\xff".to_vec()));
    fs::create_dir(&root).unwrap();
    fs::write(root.join("C.cs"), "class C { void F() {} }").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_dotnet-crap"))
        .arg("--root")
        .arg(&root)
        .args(["--format", "sarif"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("source root must be valid UTF-8"));
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn non_utf8_source_filename_is_rejected_without_lossy_report_identity() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path()
            .join(OsString::from_vec(b"invalid-\xff.cs".to_vec())),
        "class C { void F() {} }",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_dotnet-crap"))
        .arg("--root")
        .arg(dir.path())
        .args(["--format", "sarif"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("non-UTF-8 source path"));
}
