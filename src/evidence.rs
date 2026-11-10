//! Native coverage v1 schema, reading and validation.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path},
};

/// Native coverage document; its serialized form is the collector's output contract.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Coverage {
    pub(crate) version: u32,
    #[serde(deserialize_with = "unique_map")]
    pub(crate) files: BTreeMap<String, CoverageFile>,
}
/// Line hit counts for one source file, bound to the source SHA-256.
#[derive(Deserialize, Serialize)]
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
    serde_json::from_slice(&fs::read(path).context("reading coverage")?)
        .context("invalid coverage JSON")
}
/// Check the coverage version, entry paths, hashes and line keys.
pub(crate) fn validate(c: &Coverage) -> Result<()> {
    if c.version != 1 {
        bail!("unsupported coverage version");
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
            bail!("invalid coverage entry: {path}");
        }
        for l in f.lines.keys() {
            if l.parse::<usize>()
                .ok()
                .filter(|v| *v > 0 && v.to_string() == *l)
                .is_none()
            {
                bail!("invalid coverage line: {l}");
            }
        }
    }
    Ok(())
}
