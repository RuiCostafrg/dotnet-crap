//! Safe publication of rendered scan reports.
use crate::error::{fail, ErrorKind, Result, ResultExt};
use std::{io::Write, path::Path};
use walkdir::WalkDir;

/// Publish a rendered report after rejecting source aliases and staging the bytes.
///
/// Callers should call `ScanResult::verify_sources` before relying on the
/// rendered output. This function verifies the sources again after staging and
/// before the staged file replaces `path`.
pub fn write_report(result: &crate::ScanResult, rendered: &str, path: &Path) -> Result<()> {
    if is_csharp_path(path) {
        fail!(Output, "report output cannot overwrite C# source");
    }
    if std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        fail!(Output, "report output cannot be a symbolic link");
    }
    if path.exists() {
        for input in result.input_paths().iter() {
            if same_file::is_same_file(path, input).kind(ErrorKind::Io)? {
                fail!(Output, "report output aliases an input");
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
                fail!(Output, "report output aliases C# source");
            }
        }
    }
    let parent = destination_parent(path);
    let mut staged = tempfile::NamedTempFile::new_in(parent).kind(ErrorKind::Output)?;
    staged
        .write_all(rendered.as_bytes())
        .kind(ErrorKind::Output)?;
    staged.as_file().sync_all().kind(ErrorKind::Output)?;
    result.verify_sources()?;
    staged.persist(path).kind(ErrorKind::Output)?;
    Ok(())
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
