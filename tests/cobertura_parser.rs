use dotnet_crap::cobertura::{parse_reports, LineEvidence};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

/// Minimal report fixture with selected source text and real files on disk.
struct Fixture {
    directory: tempfile::TempDir,
    sources: BTreeMap<String, String>,
    reports: Vec<PathBuf>,
    next: usize,
}

impl Fixture {
    fn new() -> Self {
        Self {
            directory: tempfile::tempdir().unwrap(),
            sources: BTreeMap::new(),
            reports: Vec::new(),
            next: 0,
        }
    }

    fn root(&self) -> &Path {
        self.directory.path()
    }

    fn write(&self, relative: &str, text: &str) {
        let path = self.root().join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn source(&mut self, relative: &str, text: &str) {
        self.write(relative, text);
        self.sources.insert(relative.to_owned(), text.to_owned());
    }

    fn unselected(&mut self, relative: &str, text: &str) {
        self.write(relative, text);
    }

    fn report(&mut self, xml: &str) {
        let path = self
            .root()
            .join("reports")
            .join(format!("report-{}.xml", self.next));
        self.next += 1;
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, xml).unwrap();
        self.reports.push(path);
    }

    fn parse(&self) -> anyhow::Result<LineEvidence> {
        parse_reports(self.root(), &self.sources, &self.reports)
    }
}

/// Encode synthetic XML attributes without changing the path spelling under test.
fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Build minimal package structure while leaving path ownership explicit in each test.
fn document(prefixes: &[String], classes: &[String]) -> String {
    let sources = if prefixes.is_empty() {
        String::new()
    } else {
        format!(
            "<sources>{}</sources>",
            prefixes
                .iter()
                .map(|prefix| format!("<source>{}</source>", escape(prefix)))
                .collect::<String>()
        )
    };
    format!(
        "<coverage version=\"1.9\">{sources}<packages><package name=\"Assembly\"><classes>{}</classes></package></packages></coverage>",
        classes.concat()
    )
}

/// Include conflicting method-level hits to prove only class-level records contribute evidence.
fn class(filename: &str, lines: &[(u32, u64)]) -> String {
    let line_xml = lines
        .iter()
        .map(|(number, hits)| format!("<line number=\"{number}\" hits=\"{hits}\"/>"))
        .collect::<String>();
    // Method-level copies must never be treated as additional evidence.
    format!(
        "<class name=\"C\" filename=\"{}\"><methods><method name=\"M\"><lines><line number=\"1\" hits=\"999\"/></lines></method></methods><lines>{line_xml}</lines></class>",
        escape(filename)
    )
}

#[test]
/// Keep zeros and maximum counts independently of class ordering or repeated instrumentation.
fn merges_partial_classes_and_multiple_reports_by_maximum_hits() {
    let mut fixture = Fixture::new();
    fixture.source("src/Gauge.cs", "line1\nline2\nline3\nline4\nline5\n");
    fixture.report(&document(
        &[],
        &[class("src/Gauge.cs", &[(1, 1), (2, 0), (3, 5)])],
    ));
    fixture.report(&document(
        &[],
        &[
            class("src/Gauge.cs", &[(2, 4), (3, 2), (4, 0)]),
            class("src/Gauge.cs", &[(3, 7), (5, 0)]),
        ],
    ));

    let parsed = fixture.parse().unwrap();
    assert_eq!(
        parsed.get("src/Gauge.cs").unwrap(),
        &BTreeMap::from([(1, 1), (2, 4), (3, 7), (4, 0), (5, 0)])
    );
}

#[test]
/// Use distinct literal roots to ensure assembly union retains both source identities.
fn merges_selected_files_from_multiple_assemblies() {
    let mut fixture = Fixture::new();
    fixture.source("Alpha/A.cs", "a\nb\nc\n");
    fixture.source("Beta/B.cs", "a\nb\nc\n");
    let root = fixture.root().display().to_string();
    fixture.report(&document(
        &[format!("{root}/Alpha/")],
        &[class("A.cs", &[(1, 1), (2, 0)])],
    ));
    fixture.report(&document(
        &[format!("{root}/Beta/")],
        &[class("B.cs", &[(2, 3), (3, 0)])],
    ));

    let parsed = fixture.parse().unwrap();
    assert_eq!(
        parsed.get("Alpha/A.cs").unwrap(),
        &BTreeMap::from([(1, 1), (2, 0)])
    );
    assert_eq!(
        parsed.get("Beta/B.cs").unwrap(),
        &BTreeMap::from([(2, 3), (3, 0)])
    );
}

#[test]
/// Equivalent trailing-slash prefixes must not create artificial path ambiguity.
fn literal_source_prefixes_deduplicate_canonical_candidates() {
    let mut fixture = Fixture::new();
    fixture.source("lib/A.cs", "a\nb\nc\n");
    let root = fixture.root().display().to_string();
    fixture.report(&document(
        &[format!("{root}/lib"), format!("{root}/lib/")],
        &[class("A.cs", &[(1, 1)])],
    ));

    let parsed = fixture.parse().unwrap();
    assert_eq!(parsed.get("lib/A.cs").unwrap(), &BTreeMap::from([(1, 1)]));
}

#[cfg(unix)]
#[test]
/// Coverlet may use / as a source prefix; containment applies to the resolved file.
fn accepts_a_source_prefix_outside_the_root_only_for_a_contained_file() {
    let mut fixture = Fixture::new();
    fixture.source("B.cs", "a\nb\n");
    let relative_to_slash = fixture
        .root()
        .strip_prefix("/")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    fixture.report(&document(
        &["/".to_owned()],
        &[class(&format!("{relative_to_slash}/B.cs"), &[(2, 1)])],
    ));

    let parsed = fixture.parse().unwrap();
    assert_eq!(parsed.get("B.cs").unwrap(), &BTreeMap::from([(2, 1)]));
}

#[test]
/// A real unselected namesake must never lend hits to a selected file.
fn unselected_root_files_are_ignored_without_basename_guessing() {
    let mut fixture = Fixture::new();
    fixture.source("src/A.cs", "a\nb\n");
    fixture.unselected("other/A.cs", "a\nb\n");
    let root = fixture.root().display().to_string();
    fixture.report(&document(
        &[format!("{root}/other/")],
        &[class("A.cs", &[(1, 1)])],
    ));

    assert!(fixture.parse().unwrap().is_empty());
}

#[test]
/// Retain exact root-relative identities without searching the tree by suffix.
fn root_fallback_handles_an_exact_relative_file_when_prefixes_miss() {
    let mut fixture = Fixture::new();
    fixture.source("src/A.cs", "a\nb\n");
    fixture.unselected("other/A.cs", "a\nb\n");
    let root = fixture.root().display().to_string();
    fixture.report(&document(
        &[format!("{root}/missing/")],
        &[class("src/A.cs", &[(1, 1)])],
    ));

    assert_eq!(
        fixture.parse().unwrap().get("src/A.cs").unwrap(),
        &BTreeMap::from([(1, 1)])
    );
}

#[test]
/// A missing filename must not silently resolve to a similarly named selected file.
fn unknown_relative_filename_is_not_suffix_matched() {
    let mut fixture = Fixture::new();
    fixture.source("src/A.cs", "a\nb\n");
    fixture.report(&document(&[], &[class("A.cs", &[(1, 1)])]));

    let error = format!("{:#}", fixture.parse().unwrap_err());
    assert!(error.contains("does not resolve"), "{error}");
}

#[test]
/// Two real prefix candidates require an error rather than arbitrary ownership.
fn ambiguous_exact_candidates_are_rejected() {
    let mut fixture = Fixture::new();
    fixture.source("one/A.cs", "a\nb\n");
    fixture.source("two/A.cs", "a\nb\n");
    let root = fixture.root().display().to_string();
    fixture.report(&document(
        &[format!("{root}/one/"), format!("{root}/two/")],
        &[class("A.cs", &[(1, 1)])],
    ));

    let error = format!("{:#}", fixture.parse().unwrap_err());
    assert!(error.contains("ambiguous"), "{error}");
}

#[test]
/// Exercise host-native absolute identities separately from foreign-root rejection.
fn absolute_paths_and_backslash_separators_resolve_exactly() {
    let mut fixture = Fixture::new();
    fixture.source("src/A.cs", "a\nb\nc\n");
    let absolute = fixture.root().join("src/A.cs").display().to_string();
    fixture.report(&document(&[], &[class(&absolute, &[(1, 1)])]));
    fixture.report(&document(&[], &[class("src\\A.cs", &[(2, 0)])]));

    let parsed = fixture.parse().unwrap();
    assert_eq!(
        parsed.get("src/A.cs").unwrap(),
        &BTreeMap::from([(1, 1), (2, 0)])
    );
}

#[test]
fn absolute_paths_outside_the_root_are_rejected() {
    let outside = tempfile::tempdir().unwrap();
    let outside_file = outside.path().join("Outside.cs");
    fs::write(&outside_file, "a\nb\n").unwrap();
    let mut fixture = Fixture::new();
    fixture.source("A.cs", "a\nb\n");
    fixture.report(&document(
        &[],
        &[class(&outside_file.display().to_string(), &[(1, 1)])],
    ));

    let error = format!("{:#}", fixture.parse().unwrap_err());
    assert!(error.contains("outside the coverage root"), "{error}");
}

#[cfg(not(windows))]
#[test]
/// A drive from another host must not be reinterpreted as a relative Unix filename.
fn foreign_windows_roots_are_rejected() {
    let mut fixture = Fixture::new();
    fixture.source("A.cs", "a\nb\n");
    fixture.report(&document(&[], &[class("C:\\repo\\src\\A.cs", &[(1, 1)])]));

    let error = format!("{:#}", fixture.parse().unwrap_err());
    assert!(error.contains("Windows"), "{error}");
}

#[test]
/// Unmapped remote and compiler-placeholder identities cannot authenticate local source.
fn source_link_and_deterministic_paths_are_rejected() {
    for filename in [
        "https://raw.githubusercontent.com/example/repo/main/A.cs",
        "file:///repo/A.cs",
        "/_/MyLibrary/A.cs",
    ] {
        let mut fixture = Fixture::new();
        fixture.source("A.cs", "a\nb\n");
        fixture.report(&document(&[], &[class(filename, &[(1, 1)])]));
        assert!(fixture.parse().is_err(), "accepted {filename}");
    }
    let mut fixture = Fixture::new();
    fixture.source("A.cs", "a\nb\n");
    fixture.report(&document(
        &["https://example.com/source/".to_owned()],
        &[class("A.cs", &[(1, 1)])],
    ));
    assert!(fixture.parse().is_err());

    let mut fixture = Fixture::new();
    fixture.source("A.cs", "a\nb\n");
    fixture.report(&document(
        &["/_/MyLibrary/".to_owned()],
        &[class("A.cs", &[(1, 1)])],
    ));
    assert!(fixture.parse().is_err(), "accepted a deterministic prefix");
    #[cfg(not(windows))]
    {
        let mut fixture = Fixture::new();
        fixture.source("A.cs", "a\nb\n");
        fixture.report(&document(
            &["C:\\repo\\".to_owned()],
            &[class("A.cs", &[(1, 1)])],
        ));
        assert!(
            fixture.parse().is_err(),
            "accepted a foreign Windows prefix"
        );
    }
}

#[test]
/// Reject lexical parent traversal before filesystem normalization can hide it.
fn parent_components_and_source_prefix_escapes_are_rejected() {
    let mut fixture = Fixture::new();
    fixture.source("A.cs", "a\nb\n");
    fixture.report(&document(&[], &[class("../A.cs", &[(1, 1)])]));
    assert!(fixture.parse().is_err());

    let mut fixture = Fixture::new();
    fixture.source("A.cs", "a\nb\n");
    fixture.report(&document(
        &["../outside/".to_owned()],
        &[class("A.cs", &[(1, 1)])],
    ));
    assert!(fixture.parse().is_err());
}

#[test]
fn empty_report_slice_is_rejected() {
    let fixture = Fixture::new();
    let error = format!("{:#}", fixture.parse().unwrap_err());
    assert!(error.contains("no Cobertura reports"), "{error}");
}

#[test]
/// Require parser/security diagnostics, not a later missing-element failure.
fn malformed_and_unsafe_xml_is_rejected() {
    let malformed = "<coverage><packages><package></coverage>";
    assert_xml_error(malformed);

    let doctype = "<!DOCTYPE coverage SYSTEM \"https://example.com/coverage.dtd\"><coverage/>";
    assert_xml_error(doctype);

    let entity = "<!DOCTYPE coverage [<!ENTITY xxe SYSTEM \"file:///etc/passwd\">]><coverage>&xxe;</coverage>";
    assert_xml_error(entity);

    let stylesheet = "<?xml-stylesheet href=\"https://example.com/style.xsl\"?><coverage/>";
    assert_xml_error(stylesheet);
}

/// Distinguish XML rejection from unrelated structural or source-resolution errors.
fn assert_xml_error(xml: &str) {
    let mut fixture = Fixture::new();
    fixture.source("A.cs", "a\nb\n");
    fixture.report(xml);
    let error = format!("{:#}", fixture.parse().unwrap_err());
    assert!(
        error.contains("forbidden") || error.contains("malformed"),
        "{error}"
    );
}

#[test]
/// Missing structural containers remain invalid even though empty classes are allowed.
fn required_coverage_structure_is_enforced() {
    for xml in [
        "<not-coverage/>",
        "<coverage><packages/></coverage>",
        "<coverage><packages><package/></packages></coverage>",
        "<coverage><packages><package><classes/></package></packages></coverage>",
        "<coverage><packages><package><classes><class filename=\"A.cs\"/></classes></package></packages></coverage>",
    ] {
        let mut fixture = Fixture::new();
        fixture.source("A.cs", "a\nb\n");
        fixture.report(xml);
        assert!(fixture.parse().is_err(), "accepted {xml}");
    }
}

#[test]
/// Reject noncanonical numeric spellings, overflow and out-of-source evidence.
fn invalid_line_numbers_and_hit_counts_are_rejected() {
    for line in [
        "<line number=\"0\" hits=\"1\"/>",
        "<line number=\"4\" hits=\"1\"/>",
        "<line number=\"-1\" hits=\"1\"/>",
        "<line number=\"+1\" hits=\"1\"/>",
        "<line number=\"01\" hits=\"1\"/>",
        "<line number=\"4294967296\" hits=\"1\"/>",
        "<line number=\"x\" hits=\"1\"/>",
        "<line hits=\"1\"/>",
        "<line number=\"1\"/>",
        "<line number=\"1\" hits=\"-1\"/>",
        "<line number=\"1\" hits=\"+1\"/>",
        "<line number=\"1\" hits=\"01\"/>",
        "<line number=\"1\" hits=\"1.5\"/>",
        "<line number=\"1\" hits=\"18446744073709551616\"/>",
    ] {
        let mut fixture = Fixture::new();
        fixture.source("A.cs", "a\nb\nc\n");
        let xml = format!(
            "<coverage><packages><package><classes><class filename=\"A.cs\"><lines>{line}</lines></class></classes></package></packages></coverage>"
        );
        fixture.report(&xml);
        assert!(fixture.parse().is_err(), "accepted {line}");
    }
}

#[test]
fn class_filename_is_required() {
    let mut fixture = Fixture::new();
    fixture.source("A.cs", "a\nb\n");
    fixture.report(
        "<coverage><packages><package><classes><class><lines><line number=\"1\" hits=\"1\"/></lines></class></classes></package></packages></coverage>",
    );
    let error = format!("{:#}", fixture.parse().unwrap_err());
    assert!(error.contains("filename"), "{error}");
}

#[test]
fn source_text_line_bounds_use_original_utf8() {
    let mut fixture = Fixture::new();
    fixture.source("A.cs", "first\r\nsecond\r\n");
    fixture.report(&document(&[], &[class("A.cs", &[(1, 0), (2, 1)])]));
    assert_eq!(
        fixture.parse().unwrap().get("A.cs").unwrap(),
        &BTreeMap::from([(1, 0), (2, 1)])
    );

    let mut fixture = Fixture::new();
    fixture.source("A.cs", "first\r\nsecond\r\n");
    fixture.report(&document(&[], &[class("A.cs", &[(3, 1)])]));
    assert!(fixture.parse().is_err());
}

#[test]
/// An uncovered method has present zero evidence, unlike a method with no supplied lines.
fn zero_hit_only_evidence_is_preserved_as_zero() {
    let mut fixture = Fixture::new();
    fixture.source("A.cs", "a\nb\n");
    fixture.report(&document(&[], &[class("A.cs", &[(1, 0), (2, 0)])]));
    assert_eq!(
        fixture.parse().unwrap().get("A.cs").unwrap(),
        &BTreeMap::from([(1, 0), (2, 0)])
    );
}

#[test]
/// Filesystem canonicalization must not hide ambiguous lexical path spellings.
fn noncanonical_filenames_are_rejected_even_when_they_name_existing_files() {
    for filename in ["./src/A.cs", "src/./A.cs", "src//A.cs", "src/A.cs/"] {
        let mut fixture = Fixture::new();
        fixture.source("src/A.cs", "a\nb\n");
        fixture.report(&document(&[], &[class(filename, &[(1, 1)])]));
        assert!(fixture.parse().is_err(), "accepted {filename}");
    }
}

#[test]
fn unc_paths_are_explicitly_unsupported_on_every_host() {
    let mut fixture = Fixture::new();
    fixture.source("A.cs", "a\nb\n");
    fixture.report(&document(&[], &[class(r"\\server\share\A.cs", &[(1, 1)])]));
    let error = format!("{:#}", fixture.parse().unwrap_err());
    assert!(error.contains("UNC"), "{error}");
}

#[test]
/// Two selected namesakes make text truncation an observable evidence-ownership error.
fn source_prefix_comments_and_cdata_cannot_truncate_ownership() {
    for prefix in ["alpha<!--split-->/beta", "alpha<![CDATA[/beta]]>"] {
        let mut fixture = Fixture::new();
        fixture.source("alpha/A.cs", "a\nb\n");
        fixture.source("alpha/beta/A.cs", "a\nb\n");
        fixture.report(
            &document(&["placeholder".into()], &[class("A.cs", &[(1, 1)])])
                .replace("placeholder", prefix),
        );
        let evidence = fixture.parse().unwrap();
        assert_eq!(evidence.keys().collect::<Vec<_>>(), vec!["alpha/beta/A.cs"]);
    }
    let mut fixture = Fixture::new();
    fixture.source("alpha/A.cs", "a\nb\n");
    fixture.report(
        &document(&["placeholder".into()], &[class("A.cs", &[(1, 1)])])
            .replace("placeholder", "alpha<child/>"),
    );
    assert!(format!("{:#}", fixture.parse().unwrap_err()).contains("must not contain elements"));
}

#[test]
/// Zero executable lines must not manufacture a coverage denominator or abort other evidence.
fn empty_selected_and_unselected_classes_leave_evidence_absent() {
    let mut fixture = Fixture::new();
    fixture.source("A.cs", "a\nb\n");
    fixture.unselected("B.cs", "a\nb\n");
    fixture.report(&document(&[], &[class("A.cs", &[]), class("B.cs", &[])]));
    assert!(fixture.parse().unwrap().is_empty());
}

#[test]
fn ordinary_underscore_directory_is_an_exact_local_path() {
    let mut fixture = Fixture::new();
    fixture.source("_/A.cs", "a\nb\n");
    fixture.report(&document(&[], &[class("_/A.cs", &[(1, 1)])]));
    assert_eq!(fixture.parse().unwrap()["_/A.cs"], BTreeMap::from([(1, 1)]));
}

#[test]
fn oversized_xml_returns_a_controlled_error_before_allocation() {
    let mut fixture = Fixture::new();
    fixture.source("A.cs", "a\nb\n");
    fixture.report("<coverage/>");
    fs::OpenOptions::new()
        .write(true)
        .open(&fixture.reports[0])
        .unwrap()
        .set_len(16 * 1024 * 1024 + 1)
        .unwrap();
    let error = format!("{:#}", fixture.parse().unwrap_err());
    assert!(error.contains("size limit"), "{error}");
}

#[test]
/// Excluded assemblies are harmless beside real classes; a wholly classless report still fails.
fn empty_packages_beside_class_records_do_not_discard_valid_evidence() {
    let mut fixture = Fixture::new();
    fixture.source("A.cs", "a\nb\n");
    let valid = document(&[], &[class("A.cs", &[(1, 1), (2, 0)])]);
    fixture.report(&valid.replace(
        "<packages>",
        "<packages><package name=\"Excluded\"><classes/></package>",
    ));
    assert_eq!(
        fixture.parse().unwrap()["A.cs"],
        BTreeMap::from([(1, 1), (2, 0)])
    );
    fixture.report(
        "<coverage><packages><package name=\"Excluded\"><classes/></package></packages></coverage>",
    );
    assert!(format!("{:#}", fixture.parse().unwrap_err()).contains("no class records"));
}
