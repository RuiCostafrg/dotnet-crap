//! Files changed since a Git revision, for `--changed-since`.

use crate::error::{fail, ErrorKind, Result, ResultExt};
use std::{
    collections::BTreeSet,
    ffi::OsStr,
    path::{Path, PathBuf},
    process::Command,
};

/// Run `git` in `dir` with options that keep repository configuration from
/// running programs (fsmonitor, external diff, textconv), returning stdout.
fn git(dir: &Path, args: &[&OsStr]) -> Result<Vec<u8>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "core.fsmonitor=false", "-c", "core.quotePath=false"])
        .args(args)
        .output()
        .context(
            ErrorKind::InvalidInput,
            "--changed-since needs `git` on PATH",
        )?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        fail!(
            InvalidInput,
            "--changed-since: git {} failed: {}",
            args.iter()
                .map(|a| a.to_string_lossy())
                .collect::<Vec<_>>()
                .join(" "),
            stderr.trim()
        );
    }
    Ok(output.stdout)
}

/// Split NUL-terminated `git -z` output into paths joined onto `top`.
fn paths(top: &Path, output: &[u8]) -> Result<Vec<PathBuf>> {
    output
        .split(|&b| b == 0)
        .filter(|p| !p.is_empty())
        .map(|p| {
            std::str::from_utf8(p)
                .map(|p| top.join(p))
                .context(ErrorKind::InvalidInput, "non-UTF-8 path from git")
        })
        .collect()
}

/// Canonical paths of the existing files in the repository containing `root`
/// that differ from the merge base of `revision` and `HEAD`: committed,
/// staged, unstaged and untracked (but not ignored) changes. Deleted files
/// are not included.
pub(crate) fn files(root: &Path, revision: &str) -> Result<BTreeSet<PathBuf>> {
    if revision.is_empty() || revision.starts_with('-') {
        fail!(
            InvalidInput,
            "--changed-since needs a revision, not {revision:?}"
        );
    }
    let top = git(root, &["rev-parse".as_ref(), "--show-toplevel".as_ref()])?;
    let top = String::from_utf8(top)
        .map(|t| PathBuf::from(t.trim_end_matches(['\n', '\r'])))
        .context(
            ErrorKind::InvalidInput,
            "non-UTF-8 repository path from git",
        )?;
    let base = match git(
        &top,
        &["merge-base".as_ref(), revision.as_ref(), "HEAD".as_ref()],
    ) {
        Ok(base) => base,
        Err(e) => {
            let shallow = git(
                &top,
                &["rev-parse".as_ref(), "--is-shallow-repository".as_ref()],
            )
            .is_ok_and(|out| out.starts_with(b"true"));
            if shallow {
                fail!(
                    InvalidInput,
                    "{e}; this is a shallow clone, so fetch the history of both {revision:?} and HEAD (for actions/checkout, set fetch-depth: 0)"
                );
            }
            return Err(e);
        }
    };
    let base = String::from_utf8_lossy(&base).trim().to_owned();
    let diff = git(
        &top,
        &[
            "diff".as_ref(),
            "--name-only".as_ref(),
            "-z".as_ref(),
            "--no-ext-diff".as_ref(),
            "--no-textconv".as_ref(),
            "--diff-filter=d".as_ref(),
            base.as_ref(),
            "--".as_ref(),
        ],
    )?;
    let untracked = git(
        &top,
        &[
            "ls-files".as_ref(),
            "--others".as_ref(),
            "--exclude-standard".as_ref(),
            "-z".as_ref(),
        ],
    )?;
    let mut changed = BTreeSet::new();
    for path in paths(&top, &diff)?
        .into_iter()
        .chain(paths(&top, &untracked)?)
    {
        // Canonical, to compare with the selection. A path that no longer
        // resolves is not a file to score; the caller keeps only selected
        // files, so paths outside the root fall away there.
        if let Ok(path) = path.canonicalize() {
            changed.insert(path);
        }
    }
    Ok(changed)
}
