//! Safe publication of rendered scan reports.
use crate::error::{fail, ErrorKind, Result, ResultExt};
use std::{io::Write, path::Path};
use walkdir::WalkDir;

/// Publish a rendered report after rejecting aliases to scan inputs and C# sources.
///
/// Callers should call `ScanResult::verify_sources` before relying on the
/// rendered output. This function verifies the sources again after staging and
/// before the staged file replaces `path`.
pub fn write_report(result: &crate::ScanResult, rendered: &str, path: &Path) -> Result<()> {
    publish(result, rendered, path, "report output", true)
}

/// Stage `text` beside `path` and move it into place once the sources are
/// verified again, after the checks `write_report` documents. `label` names the
/// output in errors. Without `replace`, an existing `path` is an error and is
/// never overwritten, even by a file created while staging.
pub(crate) fn publish(
    result: &crate::ScanResult,
    text: &str,
    path: &Path,
    label: &str,
    replace: bool,
) -> Result<()> {
    if is_csharp_path(path) {
        fail!(Output, "{label} cannot overwrite C# source");
    }
    if std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        fail!(Output, "{label} cannot be a symbolic link");
    }
    if path.exists() {
        for input in result.input_paths().iter() {
            if same_file::is_same_file(path, input).kind(ErrorKind::Io)? {
                fail!(Output, "{label} aliases an input");
            }
        }
        // Also protect ignored/generated/unselected sources. Do not follow
        // directory links while looking for aliases.
        for entry in WalkDir::new(result.root()).follow_links(false) {
            let entry = entry.kind(ErrorKind::Io)?;
            if is_csharp_path(entry.path())
                && entry.path().is_file()
                && same_file::is_same_file(path, entry.path()).kind(ErrorKind::Io)?
            {
                fail!(Output, "{label} aliases C# source");
            }
        }
    }
    let parent = destination_parent(path);
    let mut staged = tempfile::NamedTempFile::new_in(parent).kind(ErrorKind::Output)?;
    staged.write_all(text.as_bytes()).kind(ErrorKind::Output)?;
    staged.as_file().sync_all().kind(ErrorKind::Output)?;
    result.verify_sources()?;
    if replace {
        staged.persist(path).kind(ErrorKind::Output)?;
    } else if let Err(e) = staged.persist_noclobber(path) {
        if e.error.kind() == std::io::ErrorKind::AlreadyExists {
            fail!(
                Output,
                "{label} already exists: {} (use --overwrite-baseline to replace it)",
                path.display()
            );
        }
        Err(e).kind(ErrorKind::Output)?;
    }
    Ok(())
}

/// Whether two output paths name the same file, existing or not. A missing file
/// is identified by its canonical parent directory and its name.
pub fn same_destination(a: &Path, b: &Path) -> Result<bool> {
    let identify = |path: &Path| -> Result<std::path::PathBuf> {
        if path.exists() {
            return path.canonicalize().kind(ErrorKind::Io);
        }
        let parent = destination_parent(path)
            .canonicalize()
            .kind(ErrorKind::Io)?;
        Ok(parent.join(path.file_name().unwrap_or_default()))
    };
    Ok(identify(a)? == identify(b)?)
}

/// Resolve the directory that receives a staged file.
pub(crate) fn destination_parent(path: &Path) -> &Path {
    path.parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
}

/// Match C# source extensions case-insensitively.
pub(crate) fn is_csharp_path(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("cs"))
}

#[cfg(test)]
mod tests {
    use super::same_destination;

    #[test]
    fn destinations_are_compared_by_file_not_spelling() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.json");
        let b = dir.path().join("b.json");
        // Neither exists: the canonical parent and the name decide.
        assert!(same_destination(&a, &a).unwrap());
        assert!(same_destination(&a, &dir.path().join(".").join("a.json")).unwrap());
        assert!(!same_destination(&a, &b).unwrap());
        // An existing file is identified through its canonical path.
        std::fs::write(&a, "x").unwrap();
        assert!(same_destination(&a, &dir.path().join(".").join("a.json")).unwrap());
        assert!(!same_destination(&a, &b).unwrap());
        // A missing parent directory is an I/O error.
        let missing = dir.path().join("nope").join("a.json");
        assert!(same_destination(&a, &missing).is_err());
    }
}
