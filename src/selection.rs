//! Input path resolution and C# source file discovery.

use anyhow::{bail, Context, Result};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};
use walkdir::WalkDir;

/// Whether a directory name is a build or VCS directory that is never scanned.
fn is_excluded_dir_name(name: &str) -> bool {
    matches!(name, "bin" | "obj" | ".git")
}
/// Resolve inputs against the canonical root and collect eligible `.cs` files.
/// An empty input list selects the root; escaping the root is an error.
pub(crate) fn select(root: &Path, paths: &[PathBuf]) -> Result<BTreeSet<PathBuf>> {
    let mut files = BTreeSet::new();
    for path in if paths.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        paths.to_vec()
    } {
        let candidate = if path.is_absolute() {
            path
        } else {
            root.join(path)
        };
        let candidate = candidate.canonicalize().context("invalid input path")?;
        if !candidate.starts_with(root) {
            bail!("input path escapes root");
        }
        if candidate
            .strip_prefix(root)?
            .components()
            .any(|part| part.as_os_str().to_str().is_some_and(is_excluded_dir_name))
        {
            continue;
        }
        for entry in WalkDir::new(candidate)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| {
                let n = e.file_name().to_string_lossy();
                e.depth() == 0
                    || !(e.file_type().is_symlink()
                        || e.file_type().is_dir() && is_excluded_dir_name(n.as_ref()))
            })
        {
            let e = entry?;
            if e.file_type().is_file()
                && e.path()
                    .extension()
                    .is_some_and(|x| x.eq_ignore_ascii_case("cs"))
            {
                let name = e.file_name().to_string_lossy().to_ascii_lowercase();
                if !name.ends_with(".g.cs")
                    && !name.ends_with(".generated.cs")
                    && !name.ends_with(".designer.cs")
                {
                    files.insert(e.path().to_path_buf());
                }
            }
        }
    }
    Ok(files)
}
