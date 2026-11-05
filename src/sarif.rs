use anyhow::{Context, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::Path};

use crate::{Item, Report};

/// Render deterministic SARIF score, required-coverage and empty-selection diagnostics.
/// Reject a non-UTF-8 source root rather than fabricate a finding location.
pub fn render(
    report: &Report,
    root: &Path,
    require_coverage: bool,
    fail_on_empty: bool,
) -> Result<String> {
    let mut results = Vec::new();
    let mut occurrences = BTreeMap::new();
    for item in &report.functions {
        // Distinguish overloads and anonymous callables without including score,
        // coverage or threshold, which change as a finding is addressed.
        let occurrence = occurrences
            .entry((&item.file, &item.kind, &item.name))
            .or_insert(0usize);
        let identity = format!(
            "{}\0{}\0{}\0{}",
            item.file, item.kind, item.name, occurrence
        );
        *occurrence += 1;
        if item.crap > report.threshold {
            results.push(result(
                "DNCRAP001",
                format!(
                    "{} has CRAP score {:.2}, exceeding threshold {:.2}",
                    item.name, item.crap, report.threshold
                ),
                Some(item),
                &identity,
                json!({"score": item.crap, "threshold": report.threshold}),
            ));
        }
        if require_coverage && item.coverage.is_none() {
            results.push(result(
                "DNCRAP002",
                format!("{} has no required coverage data", item.name),
                Some(item),
                &identity,
                json!({}),
            ));
        }
    }
    if fail_on_empty && report.functions.is_empty() {
        results.push(result(
            "DNCRAP003",
            "No implemented C# callables found".into(),
            None,
            "empty-selection",
            json!({}),
        ));
    }
    let root_uri = root_uri(root.to_str().context("source root must be valid UTF-8")?);
    Ok(serde_json::to_string_pretty(&json!({
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "version": "2.1.0",
        "runs": [{"tool":{"driver":{"name":"dotnet-crap","version":env!("CARGO_PKG_VERSION"),"rules":[
            {"id":"DNCRAP001","shortDescription":{"text":"CRAP score exceeds threshold"}},
            {"id":"DNCRAP002","shortDescription":{"text":"Required coverage is missing"}},
            {"id":"DNCRAP003","shortDescription":{"text":"No callables found"}}
        ]}},"originalUriBaseIds":{"%SRCROOT%":{"uri":root_uri}},"results":results}]
    }))?)
}

/// Build a rule-specific stable fingerprint and an optional line-only source location.
fn result(
    rule: &str,
    message: String,
    item: Option<&Item>,
    identity: &str,
    properties: Value,
) -> Value {
    let fingerprint = hex::encode(Sha256::digest(format!("{rule}\0{identity}").as_bytes()));
    let mut result = json!({"ruleId":rule,"level":"error","message":{"text":message},"partialFingerprints":{"dotnetCrap/v1":fingerprint},"properties":properties});
    if let Some(item) = item {
        // Existing scanner columns count UTF-8 bytes. SARIF columns use Unicode
        // code points or UTF-16 units, so emit accurate line-only regions.
        result["locations"] = json!([{"physicalLocation":{"artifactLocation":{"uri":uri(&item.file),"uriBaseId":"%SRCROOT%"},"region":{"startLine":item.line,"endLine":item.end_line}}}]);
    }
    result
}

/// Percent-encode UTF-8 path bytes while preserving directory separators and unreserved bytes.
fn uri(path: &str) -> String {
    let mut out = String::new();
    for b in path.as_bytes() {
        if b.is_ascii_alphanumeric() || b"/-._~".contains(b) {
            out.push(*b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Encode canonical Unix, Windows drive and UNC roots as directory file URIs.
/// Preserve literal backslashes in Unix roots instead of treating them as separators.
fn root_uri(path: &str) -> String {
    // Canonical Unix paths begin with a single slash; a backslash in such a
    // path is a literal filename character, not a Windows separator.
    if path.starts_with('/') && !path.starts_with("//") {
        return format!("file://{}/", uri(path).trim_end_matches('/'));
    }
    let normalized = path.replace('\\', "/");
    let normalized = normalized.strip_prefix("//?/").unwrap_or(&normalized);
    let unc;
    let normalized = if let Some(rest) = normalized.strip_prefix("UNC/") {
        unc = format!("//{rest}");
        &unc
    } else {
        normalized
    };
    let encoded = uri(normalized);
    let encoded = encoded.trim_end_matches('/');
    if normalized.starts_with("//") {
        format!("file:{encoded}/")
    } else if normalized.starts_with('/') {
        format!("file://{encoded}/")
    } else {
        format!("file:///{}/", encoded.replacen("%3A", ":", 1))
    }
}

#[cfg(test)]
mod tests {
    use super::root_uri;

    #[test]
    /// Verify drive, UNC, extended Windows and literal Unix path encoding independently.
    fn source_root_file_uris_cover_release_platform_paths() {
        assert_eq!(root_uri("/tmp/a b"), "file:///tmp/a%20b/");
        assert_eq!(root_uri(r"/tmp/a\b"), "file:///tmp/a%5Cb/");
        assert_eq!(root_uri("/"), "file:///");
        assert_eq!(root_uri(r"C:\src\a b"), "file:///C:/src/a%20b/");
        assert_eq!(root_uri(r"\\?\C:\src"), "file:///C:/src/");
        assert_eq!(root_uri(r"\\server\share\src"), "file://server/share/src/");
        assert_eq!(root_uri(r"\\?\UNC\server\share"), "file://server/share/");
    }
}
