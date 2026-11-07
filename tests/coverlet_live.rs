use std::{fs, path::Path, process::Command};

/// Inspect raw pinned collector method records independently of the production importer.
fn method_block<'a>(xml: &'a str, name: &str) -> &'a str {
    let marker = format!("<method name=\"{name}\"");
    let start = xml
        .find(&marker)
        .unwrap_or_else(|| panic!("Cobertura method {name:?} not found"));
    let end = xml[start..]
        .find("</method>")
        .map(|offset| start + offset + "</method>".len())
        .unwrap_or_else(|| panic!("Cobertura method {name:?} is not closed"));
    &xml[start..end]
}

/// Check physical line hits as well as the reporter summary against hand-derived expectations.
fn assert_method(xml: &str, name: &str, line_rate: &str, expected_lines: &[(&str, &str)]) {
    let method = method_block(xml, name);
    assert!(
        method.contains(&format!("line-rate=\"{line_rate}\"")),
        "unexpected {name} line-rate in {method}"
    );
    for (line, hits) in expected_lines {
        let record = format!("<line number=\"{line}\" hits=\"{hits}\"");
        assert!(method.contains(&record), "missing {record} in {method}");
    }
}

/// Distinct call counts identify xUnit versus MSTest/NUnit without trusting report order.
fn report_with_full_hits<'a>(reports: &[&'a String], line: &str, hits: &str) -> &'a str {
    let point = format!("<line number=\"{line}\" hits=\"{hits}\"");
    let matching: Vec<_> = reports
        .iter()
        .filter(|xml| method_block(xml, "Full").contains(&point))
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "expected exactly one Full report with {point}"
    );
    matching[0].as_str()
}

/// Discover actual runner attachments in the fresh fixture directory without assuming GUID names.
fn files_named(root: &Path, target: &str) -> Vec<std::path::PathBuf> {
    let mut pending = vec![root.to_path_buf()];
    let mut matches = Vec::new();
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).expect("read Coverlet results directory") {
            let path = entry.expect("read Coverlet result entry").path();
            if path.is_dir() {
                pending.push(path);
            } else if path.file_name().is_some_and(|name| name == target) {
                matches.push(path);
            }
        }
    }
    matches
}

/// Run the pinned VSTest/Coverlet fixtures and check actual Cobertura line hits.
#[test]
fn pinned_coverlet_collector_emits_fresh_multproject_cobertura() {
    if std::env::var("DOTNET_CRAP_COVERLET_E2E").as_deref() != Ok("1") {
        return;
    }

    let sdk = Command::new("dotnet")
        .arg("--version")
        .output()
        .unwrap_or_else(|error| {
            panic!("DOTNET_CRAP_COVERLET_E2E=1 but dotnet is unavailable: {error}")
        });
    assert!(
        sdk.status.success(),
        "dotnet --version failed: {}",
        String::from_utf8_lossy(&sdk.stderr)
    );
    let sdk_version = String::from_utf8_lossy(&sdk.stdout).trim().to_owned();
    assert!(
        !sdk_version.is_empty(),
        "dotnet --version returned no SDK version"
    );

    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/coverlet");
    let workspace = tempfile::tempdir().unwrap();
    let fixture = workspace.path().join("fixture");
    for entry in walkdir::WalkDir::new(&source)
        .into_iter()
        .filter_entry(|e| !matches!(e.file_name().to_str(), Some("bin" | "obj")))
    {
        let entry = entry.unwrap();
        let target = fixture.join(entry.path().strip_prefix(&source).unwrap());
        if entry.file_type().is_dir() {
            fs::create_dir_all(target).unwrap();
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
    let solution = fixture.join("CoverletFixtures.sln");
    let results = tempfile::tempdir().expect("create fresh Coverlet results directory");
    assert_eq!(fs::read_dir(results.path()).unwrap().count(), 0);
    let output = Command::new("dotnet")
        .current_dir(&fixture)
        .arg("test")
        .arg(&solution)
        .arg("--collect:XPlat Code Coverage")
        .arg("--results-directory")
        .arg(results.path())
        .arg("-m:1")
        .args([
            "--",
            "DataCollectionRunSettings.DataCollectors.DataCollector.Configuration.Format=cobertura",
        ])
        .output()
        .expect("start dotnet test");
    assert!(
        output.status.success(),
        "dotnet SDK {sdk_version} collector run failed (status {}):\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let runner_output = String::from_utf8_lossy(&output.stdout);
    for assembly in [
        "Alpha.Tests.dll",
        "Beta.Tests.dll",
        "MSTest.Tests.dll",
        "NUnit.Tests.dll",
    ] {
        let summary = runner_output
            .lines()
            .find(|line| line.contains("Passed!") && line.contains(assembly))
            .unwrap_or_else(|| panic!("no passed test summary for {assembly}: {runner_output}"));
        let normalized = summary.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(
            normalized.contains("Failed: 0, Passed: 1, Skipped: 0, Total: 1,"),
            "{normalized}"
        );
    }
    let reports = files_named(results.path(), "coverage.cobertura.xml");
    assert_eq!(
        reports.len(),
        4,
        "expected one fresh report per test project"
    );
    let contents: Vec<String> = reports
        .iter()
        .map(|path| fs::read_to_string(path).expect("read Coverlet Cobertura report"))
        .collect();
    let alpha: Vec<_> = contents
        .iter()
        .filter(|xml| xml.contains("package name=\"Alpha.Library\""))
        .collect();
    let beta: Vec<_> = contents
        .iter()
        .filter(|xml| xml.contains("package name=\"Beta.Library\""))
        .collect();
    assert_eq!(
        alpha.len(),
        2,
        "xUnit and MSTest each produce Alpha evidence"
    );
    assert_eq!(beta.len(), 2, "xUnit and NUnit each produce Beta evidence");

    // See fixtures/coverlet/README.md for the independently derived line and CRAP expectations.
    for (calls, positives) in [("2", "1"), ("3", "2")] {
        let alpha = report_with_full_hits(&alpha, "13", calls);
        assert!(alpha.contains("filename=\"Gauge.First.cs\""));
        assert!(alpha.contains("filename=\"Gauge.Second.cs\""));
        assert_method(
            alpha,
            "Choose",
            "0.8",
            &[("6", "1"), ("7", "1"), ("8", "1"), ("9", "0"), ("10", "1")],
        );
        assert_method(
            alpha,
            "Full",
            "1",
            &[
                ("13", calls),
                ("14", calls),
                ("15", positives),
                ("16", "1"),
                ("17", calls),
            ],
        );
        assert_method(alpha, "Uncovered", "0", &[("5", "0")]);
        let beta = report_with_full_hits(&beta, "6", calls);
        assert_method(
            beta,
            "Full",
            "1",
            &[
                ("6", calls),
                ("7", calls),
                ("8", "1"),
                ("9", positives),
                ("10", calls),
            ],
        );
    }

    use dotnet_crap::{
        collector::{collect, CollectOptions},
        scan, ScanOptions,
    };
    let native = workspace.path().join("native.json");
    let options = CollectOptions {
        root: fixture.clone(),
        paths: vec!["Alpha.Library".into(), "Beta.Library".into()],
        timeout: std::time::Duration::from_secs(600),
        output: native.clone(),
        expected_reports: 4,
        command: [
            "dotnet",
            "test",
            "CoverletFixtures.sln",
            "--collect:XPlat Code Coverage",
            "--results-directory",
            "{results}",
            "-m:1",
            "--",
            "DataCollectionRunSettings.DataCollectors.DataCollector.Configuration.Format=cobertura",
        ]
        .into_iter()
        .map(Into::into)
        .collect(),
    };
    collect(&options).unwrap();
    let report = scan(&ScanOptions {
        coverage: Some(native),
        require_coverage: true,
        root: options.root.clone(),
        paths: options.paths.clone(),
        ..Default::default()
    })
    .unwrap();
    assert_eq!(report.report().functions.len(), 4);
    assert!(report.report().coverage_complete);
    for (name, ratio, score) in [("Choose", 0.8, 2.032), ("Uncovered", 0.0, 2.0)] {
        let item = report
            .report()
            .functions
            .iter()
            .find(|i| i.name == name)
            .unwrap();
        assert_eq!(item.coverage, Some(ratio));
        assert!((item.crap - score).abs() < 1e-12);
    }
    for item in report
        .report()
        .functions
        .iter()
        .filter(|i| i.name == "Full")
    {
        assert_eq!(item.coverage, Some(1.0));
        assert_eq!(item.crap, 2.0);
    }
    let mut failed = options.clone();
    failed.output = workspace.path().join("failure.json");
    let test_path = fixture.join("Alpha.Tests/AlphaCoverageTests.cs");
    let passing = fs::read_to_string(&test_path).unwrap();
    fs::write(
        &test_path,
        passing.replace(
            "Assert.Equal(1, gauge.Choose(1))",
            "Assert.Equal(77, gauge.Choose(1))",
        ),
    )
    .unwrap();
    let error = collect(&failed).unwrap_err();
    assert!(
        error.to_string().contains("trusted test command failed"),
        "{error:#}"
    );
    assert!(!failed.output.exists());
    fs::write(test_path, passing).unwrap();

    // Opting this pinned VSTest fixture into the MTP runner is unsupported.
    // This is a real SDK rejection, not an inferred parity assertion.
    fs::write(
        fixture.join("global.json"),
        r#"{"test":{"runner":"Microsoft.Testing.Platform"}}"#,
    )
    .unwrap();
    let probe = Command::new("dotnet")
        .current_dir(&fixture)
        .args(["test", "--solution", "CoverletFixtures.sln"])
        .output()
        .unwrap();
    assert!(!probe.status.success());
    let diagnostic = String::from_utf8_lossy(&probe.stderr);
    assert!(
        diagnostic.contains("All projects must use that test runner"),
        "{diagnostic}"
    );
    assert!(
        ["Alpha.Tests.csproj", "Beta.Tests.csproj"]
            .iter()
            .all(|name| diagnostic.contains(name)),
        "{diagnostic}"
    );
    failed.command.insert(2, "--solution".into());
    let error = collect(&failed).unwrap_err();
    assert!(
        error.to_string().contains("trusted test command failed"),
        "{error:#}"
    );
    assert!(!failed.output.exists());
}
