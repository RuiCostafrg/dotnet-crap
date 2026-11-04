use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{fs, path::Path, process::Command};

fn scan(root: &Path, coverage: Option<&Path>) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_dotnet-crap"));
    command.args(["--format", "json", "--threshold", "2", "--root"]);
    command.arg(root).arg("Calculator.cs");
    if let Some(path) = coverage {
        command
            .arg("--coverage")
            .arg(path)
            .arg("--require-coverage");
    }
    command.output().unwrap()
}

#[test]
fn actual_csharp_partial_and_full_coverage_change_the_gate() {
    if std::env::var_os("DOTNET_CRAP_E2E").is_none() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/CoverageFixture");
    for file in ["CoverageFixture.csproj", "Calculator.cs", "Program.cs"] {
        fs::copy(fixture.join(file), dir.path().join(file)).unwrap();
    }
    let source_path = dir.path().join("Calculator.cs");
    let original = fs::read(&source_path).unwrap();
    // Snapshot before executing tests; never stamp an old report as fresh.
    let hash = hex::encode(Sha256::digest(&original));
    let uncovered = scan(dir.path(), None);
    assert_eq!(uncovered.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&uncovered.stdout).unwrap();
    assert_eq!(report["functions"][0]["complexity"], 2);
    assert_eq!(report["functions"][0]["crap"], 6.0);

    for (mode, expected_exit, covered) in [("partial", 1, 2.0 / 3.0), ("full", 0, 1.0)] {
        let output = Command::new("dotnet")
            .arg("run")
            .arg("--project")
            .arg(dir.path().join("CoverageFixture.csproj"))
            .args(["--verbosity", "quiet", "--"])
            .arg(&source_path)
            .arg(mode)
            .output()
            .expect("DOTNET_CRAP_E2E requires the .NET 10 SDK");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(fs::read(&source_path).unwrap(), original);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let lines: Value = serde_json::from_str(stdout.lines().last().unwrap()).unwrap();
        let path = dir.path().join(format!("{mode}.json"));
        fs::write(
            &path,
            serde_json::to_vec(&json!({
                "version": 1,
                "files": {"Calculator.cs": {"sha256": hash, "lines": lines}}
            }))
            .unwrap(),
        )
        .unwrap();
        let output = scan(dir.path(), Some(&path));
        assert_eq!(
            output.status.code(),
            Some(expected_exit),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert!((report["functions"][0]["coverage"].as_f64().unwrap() - covered).abs() < 1e-9);
        assert!(
            (report["functions"][0]["crap"].as_f64().unwrap()
                - (2.0 + 4.0 * (1.0 - covered).powi(3)))
            .abs()
                < 1e-9
        );
    }
    fs::write(&source_path, [original.as_slice(), b"\n"].concat()).unwrap();
    assert_eq!(
        scan(dir.path(), Some(&dir.path().join("full.json")))
            .status
            .code(),
        Some(2)
    );
}
