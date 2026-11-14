//! Native coverage v1 schema, reading and validation.

use crate::error::{fail, ErrorKind, Result, ResultExt};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path},
};

/// Native coverage document; its serialized form is the collector's output contract.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Coverage {
    pub(crate) version: u32,
    #[serde(deserialize_with = "unique_map")]
    pub(crate) files: BTreeMap<String, CoverageFile>,
}
/// Line hit counts for one source file, bound to the source SHA-256.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CoverageFile {
    pub(crate) sha256: String,
    #[serde(deserialize_with = "unique_map")]
    pub(crate) lines: BTreeMap<String, u64>,
}
/// Deserialize coverage maps while rejecting duplicate keys instead of overwriting evidence.
fn unique_map<'de, D, V>(deserializer: D) -> std::result::Result<BTreeMap<String, V>, D::Error>
where
    D: serde::Deserializer<'de>,
    V: Deserialize<'de>,
{
    struct Unique<V>(std::marker::PhantomData<V>);
    impl<'de, V: Deserialize<'de>> serde::de::Visitor<'de> for Unique<V> {
        type Value = BTreeMap<String, V>;
        /// Describe the expected coverage map when deserialization receives another JSON type.
        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a map with unique keys")
        }
        /// Retain each key once and fail immediately when a duplicate would replace evidence.
        fn visit_map<M: serde::de::MapAccess<'de>>(
            self,
            mut map: M,
        ) -> std::result::Result<Self::Value, M::Error> {
            let mut entries = BTreeMap::new();
            while let Some((key, value)) = map.next_entry::<String, V>()? {
                if entries.insert(key, value).is_some() {
                    return Err(serde::de::Error::custom("duplicate coverage map key"));
                }
            }
            Ok(entries)
        }
    }
    deserializer.deserialize_map(Unique(std::marker::PhantomData))
}
/// Read and parse a native coverage JSON file.
pub(crate) fn read(path: &Path) -> Result<Coverage> {
    serde_json::from_slice(&fs::read(path).context(ErrorKind::Io, "reading coverage")?)
        .context(ErrorKind::Coverage, "invalid coverage JSON")
}
/// Check the coverage version, entry paths, hashes and line keys.
pub(crate) fn validate(c: &Coverage) -> Result<()> {
    if c.version != 1 {
        fail!(Coverage, "unsupported coverage version");
    }
    for (path, f) in &c.files {
        if path.contains(['\\', ':'])
            || Path::new(path)
                .components()
                .any(|x| !matches!(x, Component::Normal(_)))
            || path.is_empty()
            || path.split('/').any(|part| part == "." || part == "..")
            || path.contains("//")
            || path.ends_with('/')
            || f.sha256.len() != 64
            || !f.sha256.bytes().all(|b| b.is_ascii_hexdigit())
        {
            fail!(Coverage, "invalid coverage entry: {path}");
        }
        for l in f.lines.keys() {
            if l.parse::<usize>()
                .ok()
                .filter(|v| *v > 0 && v.to_string() == *l)
                .is_none()
            {
                fail!(Coverage, "invalid coverage line: {l}");
            }
        }
    }
    Ok(())
}
/// Bind one validated coverage entry to a scanned source: the entry must carry
/// the source's SHA-256 (any hex case) and every line key must stay within the
/// source's `str::lines` count.
pub(crate) fn check_binding(
    c: &CoverageFile,
    hash: &str,
    line_count: usize,
    relative: &str,
) -> Result<()> {
    if !hash.eq_ignore_ascii_case(&c.sha256) {
        fail!(Coverage, "stale coverage for {relative}");
    }
    if c.lines
        .keys()
        .any(|l| l.parse::<usize>().unwrap_or(usize::MAX) > line_count)
    {
        fail!(Coverage, "coverage line outside {relative}");
    }
    Ok(())
}
/// Check native coverage at `path` against a scan snapshot without re-scanning:
/// the same reader, validator and binding check as `scan`, with each source's
/// hash and line count taken from the snapshot. `files` holds
/// `(relative, sha256, line_count)` per selected source, in scan order, so the
/// first failing source is the one a scan of the same bytes reports. Entries
/// for unselected files are ignored, as in `scan`. The caller still has to
/// prove the source bytes are unchanged.
pub(crate) fn check_snapshot(path: &Path, files: &[(String, String, usize)]) -> Result<()> {
    let coverage = read(path)?;
    validate(&coverage)?;
    for (relative, hash, line_count) in files {
        if let Some(c) = coverage.files.get(relative) {
            check_binding(c, hash, *line_count, relative)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod snapshot_tests {
    use super::*;
    use crate::ErrorKind;

    const HASH: &str = "ab00000000000000000000000000000000000000000000000000000000000000";

    /// Write `json` and check it against snapshot sources `a.cs` (3 lines, upper-case
    /// hash) then `b.cs` (1 line).
    fn check(json: &str) -> Result<()> {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.json");
        fs::write(&path, json).unwrap();
        check_snapshot(
            &path,
            &[
                ("a.cs".into(), HASH.to_uppercase(), 3),
                ("b.cs".into(), "cd".repeat(32), 1),
            ],
        )
    }
    /// One-entry native document.
    fn entry(name: &str, sha: &str, line: &str) -> String {
        format!(
            r#"{{"version":1,"files":{{"{name}":{{"sha256":"{sha}","lines":{{"{line}":1}}}}}}}}"#
        )
    }

    #[test]
    /// A matching hash in any case and the last line pass; unselected entries are ignored.
    fn accepts_bound_entries_and_ignores_unselected_files() {
        check(&entry("a.cs", HASH, "3")).unwrap();
        check(&entry("b.cs", &"CD".repeat(32), "1")).unwrap();
        check(&entry("c.cs", &"ef".repeat(32), "999")).unwrap();
        check(r#"{"version":1,"files":{}}"#).unwrap();
    }

    #[test]
    /// Every selected source is bound, not only the first one.
    fn checks_every_selected_source() {
        let error = check(&entry("b.cs", &"cd".repeat(32), "2")).unwrap_err();
        assert_eq!(error.to_string(), "coverage line outside b.cs");
        let error = check(&entry("b.cs", HASH, "1")).unwrap_err();
        assert_eq!(error.to_string(), "stale coverage for b.cs");
    }

    #[test]
    /// The same reader, validator and binding errors as `scan`, with the same kinds.
    fn rejects_like_scan() {
        for (json, kind, text) in [
            (
                entry("a.cs", &"cd".repeat(32), "1"),
                ErrorKind::Coverage,
                "stale coverage for a.cs",
            ),
            (
                entry("a.cs", HASH, "4"),
                ErrorKind::Coverage,
                "coverage line outside a.cs",
            ),
            (
                entry("a.cs", HASH, "0"),
                ErrorKind::Coverage,
                "invalid coverage line: 0",
            ),
            (
                entry("../a.cs", HASH, "1"),
                ErrorKind::Coverage,
                "invalid coverage entry: ../a.cs",
            ),
            ("{".to_owned(), ErrorKind::Coverage, "invalid coverage JSON"),
            (
                r#"{"version":2,"files":{}}"#.to_owned(),
                ErrorKind::Coverage,
                "unsupported coverage version",
            ),
        ] {
            let error = check(&json).unwrap_err();
            assert_eq!(error.kind(), kind, "{json}");
            assert_eq!(error.to_string(), text, "{json}");
        }
    }

    #[test]
    /// A missing staged file is an I/O error, as in `scan`.
    fn reports_unreadable_coverage_as_io() {
        let dir = tempfile::tempdir().unwrap();
        let error = check_snapshot(&dir.path().join("missing.json"), &[]).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Io);
    }
}
