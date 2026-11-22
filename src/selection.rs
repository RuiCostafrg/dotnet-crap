//! Input path resolution and C# source file discovery.

use crate::error::{fail, ErrorKind, Result, ResultExt};
use ignore::{
    gitignore::{Gitignore, GitignoreBuilder},
    WalkBuilder,
};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};
use walkdir::WalkDir;

/// Whether a directory name is a build or VCS directory that is never scanned.
fn is_excluded_dir_name(name: &str) -> bool {
    matches!(name, "bin" | "obj" | ".git")
}
/// Selection policy beyond the input paths themselves.
pub(crate) struct Filters<'a> {
    /// Apply `.gitignore` and `.ignore` files at the root and below to directory walks.
    pub(crate) respect_ignore_files: bool,
    /// Root-relative include patterns; empty keeps every file.
    pub(crate) include: &'a [String],
    /// Root-relative exclude patterns.
    pub(crate) exclude: &'a [String],
}

/// Compile gitignore-style patterns rooted at `root`; `None` when there are none.
fn compile(root: &Path, role: &str, patterns: &[String]) -> Result<Option<Gitignore>> {
    if patterns.is_empty() {
        return Ok(None);
    }
    let mut builder = GitignoreBuilder::new(root);
    for pattern in patterns {
        if pattern.trim().is_empty() {
            fail!(
                InvalidInput,
                "invalid {role} pattern {pattern:?}: empty pattern"
            );
        }
        builder
            .add_line(None, pattern)
            .with_context(ErrorKind::InvalidInput, || {
                format!("invalid {role} pattern {pattern:?}")
            })?;
    }
    builder
        .build()
        .with_context(ErrorKind::InvalidInput, || {
            format!("invalid {role} patterns")
        })
        .map(Some)
}

/// Compiled include and exclude patterns.
struct Patterns {
    include: Option<Gitignore>,
    exclude: Option<Gitignore>,
}

impl Patterns {
    /// Whether a root-relative file passes the include and exclude rules.
    fn allows(&self, relative: &Path) -> bool {
        let matches =
            |set: &Gitignore| set.matched_path_or_any_parents(relative, false).is_ignore();
        self.include.as_ref().is_none_or(matches) && !self.exclude.as_ref().is_some_and(matches)
    }
}

/// Whether a file is a non-generated `.cs` source by name.
fn is_source_name(path: &Path) -> bool {
    if !path
        .extension()
        .is_some_and(|x| x.eq_ignore_ascii_case("cs"))
    {
        return false;
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    !name.ends_with(".g.cs") && !name.ends_with(".generated.cs") && !name.ends_with(".designer.cs")
}

/// Resolve inputs against the canonical root and collect eligible `.cs` files.
/// An empty input list selects the root; escaping the root is an error.
pub(crate) fn select(
    root: &Path,
    paths: &[PathBuf],
    filters: &Filters<'_>,
) -> Result<BTreeSet<PathBuf>> {
    let patterns = Patterns {
        include: compile(root, "include", filters.include)?,
        exclude: compile(root, "exclude", filters.exclude)?,
    };
    let mut files = BTreeSet::new();
    let mut keep = |path: &Path| -> Result<()> {
        let relative = path.strip_prefix(root).kind(ErrorKind::InvalidInput)?;
        if is_source_name(path) && patterns.allows(relative) {
            files.insert(path.to_path_buf());
        }
        Ok(())
    };
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
        let candidate = candidate
            .canonicalize()
            .context(ErrorKind::InvalidInput, "invalid input path")?;
        if !candidate.starts_with(root) {
            fail!(InvalidInput, "input path escapes root");
        }
        if candidate
            .strip_prefix(root)
            .kind(ErrorKind::InvalidInput)?
            .components()
            .any(|part| part.as_os_str().to_str().is_some_and(is_excluded_dir_name))
        {
            continue;
        }
        if !candidate.is_dir() {
            // An explicitly named file bypasses ignore files.
            keep(&candidate)?;
        } else if filters.respect_ignore_files {
            // Walk from the root so its ignore files apply, pruned to the selection.
            let walker = WalkBuilder::new(root)
                .hidden(false)
                .git_ignore(true)
                .ignore(true)
                .require_git(false)
                .parents(false)
                .git_global(false)
                .git_exclude(false)
                .follow_links(false)
                .filter_entry({
                    let candidate = candidate.clone();
                    move |e| {
                        let related =
                            e.path().starts_with(&candidate) || candidate.starts_with(e.path());
                        let file_type = e.file_type();
                        related
                            && (e.depth() == 0
                                || !(file_type.is_some_and(|t| t.is_symlink())
                                    || file_type.is_some_and(|t| t.is_dir())
                                        && is_excluded_dir_name(&e.file_name().to_string_lossy())))
                    }
                })
                .build();
            for entry in walker {
                let e = entry.kind(ErrorKind::Io)?;
                if e.file_type().is_some_and(|t| t.is_file()) && e.path().starts_with(&candidate) {
                    keep(e.path())?;
                }
            }
        } else {
            for entry in WalkDir::new(&candidate)
                .follow_links(false)
                .into_iter()
                .filter_entry(|e| {
                    let n = e.file_name().to_string_lossy();
                    e.depth() == 0
                        || !(e.file_type().is_symlink()
                            || e.file_type().is_dir() && is_excluded_dir_name(n.as_ref()))
                })
            {
                let e = entry.kind(ErrorKind::Io)?;
                if e.file_type().is_file() {
                    keep(e.path())?;
                }
            }
        }
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const ON: Filters<'static> = Filters {
        respect_ignore_files: true,
        include: &[],
        exclude: &[],
    };

    /// Create `files` (empty contents) under a fresh canonical temp root.
    fn tree(files: &[&str]) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        for file in files {
            let path = root.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "class C {}\n").unwrap();
        }
        (dir, root)
    }

    fn names(root: &Path, files: BTreeSet<PathBuf>) -> Vec<String> {
        files
            .iter()
            .map(|f| {
                f.strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect()
    }

    fn pick(root: &Path, paths: &[&str], filters: &Filters<'_>) -> Vec<String> {
        let paths: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
        names(root, select(root, &paths, filters).unwrap())
    }

    #[test]
    fn root_gitignore_applies_when_scanning_a_subdirectory() {
        let (_d, root) = tree(&["src/a.cs", "src/skip.cs", "src/sub/skip.cs", "other.cs"]);
        fs::write(root.join(".gitignore"), "skip.cs\n").unwrap();
        assert_eq!(pick(&root, &["src"], &ON), ["src/a.cs"]);
        assert_eq!(pick(&root, &[], &ON), ["other.cs", "src/a.cs"]);
    }

    #[test]
    fn subdirectory_selection_excludes_siblings() {
        let (_d, root) = tree(&["src/a.cs", "tests/b.cs"]);
        assert_eq!(pick(&root, &["tests"], &ON), ["tests/b.cs"]);
    }

    #[test]
    fn nested_gitignore_applies_below_its_directory_only() {
        let (_d, root) = tree(&["src/a.cs", "src/gen.cs", "gen.cs"]);
        fs::write(root.join("src/.gitignore"), "gen.cs\n").unwrap();
        assert_eq!(pick(&root, &[], &ON), ["gen.cs", "src/a.cs"]);
    }

    #[test]
    fn dot_ignore_files_apply() {
        let (_d, root) = tree(&["a.cs", "b.cs"]);
        fs::write(root.join(".ignore"), "b.cs\n").unwrap();
        assert_eq!(pick(&root, &[], &ON), ["a.cs"]);
    }

    #[test]
    fn negation_re_includes_a_file() {
        let (_d, root) = tree(&["keep.cs", "drop.cs"]);
        fs::write(root.join(".gitignore"), "*.cs\n!keep.cs\n").unwrap();
        assert_eq!(pick(&root, &[], &ON), ["keep.cs"]);
    }

    #[test]
    fn explicit_files_bypass_ignore_files_but_directories_do_not() {
        let (_d, root) = tree(&["src/a.cs", "src/skip.cs"]);
        fs::write(root.join(".gitignore"), "skip.cs\n").unwrap();
        assert_eq!(pick(&root, &["src/skip.cs"], &ON), ["src/skip.cs"]);
        assert_eq!(pick(&root, &["src"], &ON), ["src/a.cs"]);
    }

    #[test]
    fn disabling_ignore_files_selects_ignored_files() {
        let (_d, root) = tree(&["a.cs", "b.cs"]);
        fs::write(root.join(".gitignore"), "b.cs\n").unwrap();
        let off = Filters {
            respect_ignore_files: false,
            ..ON
        };
        assert_eq!(pick(&root, &[], &off), ["a.cs", "b.cs"]);
        assert_eq!(pick(&root, &[], &ON), ["a.cs"]);
    }

    #[test]
    fn ignore_files_above_the_root_are_not_applied() {
        let (_d, outer) = tree(&["project/a.cs", "project/b.cs"]);
        fs::write(outer.join(".gitignore"), "b.cs\n").unwrap();
        fs::write(outer.join(".ignore"), "a.cs\n").unwrap();
        let root = outer.join("project");
        assert_eq!(pick(&root, &[], &ON), ["a.cs", "b.cs"]);
    }

    #[test]
    fn build_directories_stay_skipped_with_ignore_files_on() {
        let (_d, root) = tree(&["a.cs", "bin/x.cs", "obj/y.cs", ".git/z.cs", "src/obj/w.cs"]);
        assert_eq!(pick(&root, &[], &ON), ["a.cs"]);
        assert!(pick(&root, &["bin"], &ON).is_empty());
        assert!(pick(&root, &["src/obj"], &ON).is_empty());
    }

    #[test]
    fn name_rules_still_apply_with_ignore_files_on() {
        let (_d, root) = tree(&["A.CS", "b.g.cs", "c.Generated.cs", "d.designer.cs", "e.txt"]);
        assert_eq!(pick(&root, &[], &ON), ["A.CS"]);
        assert_eq!(pick(&root, &["b.g.cs"], &ON), Vec::<String>::new());
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_skipped_with_ignore_files_on() {
        let (_d, root) = tree(&["a.cs", "real/b.cs"]);
        std::os::unix::fs::symlink(root.join("a.cs"), root.join("link.cs")).unwrap();
        std::os::unix::fs::symlink(root.join("real"), root.join("linkdir")).unwrap();
        assert_eq!(pick(&root, &[], &ON), ["a.cs", "real/b.cs"]);
    }

    #[test]
    fn escaping_the_root_is_an_error() {
        let (_d, outer) = tree(&["project/a.cs", "b.cs"]);
        let root = outer.join("project");
        let error = select(&root, &[outer.join("b.cs")], &ON).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::InvalidInput);
        assert_eq!(error.to_string(), "input path escapes root");
    }

    fn globs(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn include_keeps_only_matching_files() {
        let (_d, root) = tree(&["src/a.cs", "tests/b.cs", "c.cs"]);
        let include = globs(&["src/**"]);
        let filters = Filters {
            include: &include,
            ..ON
        };
        assert_eq!(pick(&root, &[], &filters), ["src/a.cs"]);
    }

    #[test]
    fn exclude_drops_matching_files_and_directories() {
        let (_d, root) = tree(&["src/a.cs", "src/Migrations/m.cs", "b.cs"]);
        let exclude = globs(&["**/Migrations/**"]);
        let filters = Filters {
            exclude: &exclude,
            ..ON
        };
        assert_eq!(pick(&root, &[], &filters), ["b.cs", "src/a.cs"]);
        let exclude = globs(&["Migrations/"]);
        let filters = Filters {
            exclude: &exclude,
            ..ON
        };
        assert_eq!(pick(&root, &[], &filters), ["b.cs", "src/a.cs"]);
    }

    #[test]
    fn include_and_exclude_combine() {
        let (_d, root) = tree(&["src/a.cs", "src/b.cs", "tests/c.cs"]);
        let include = globs(&["src/**"]);
        let exclude = globs(&["b.cs"]);
        let filters = Filters {
            include: &include,
            exclude: &exclude,
            ..ON
        };
        assert_eq!(pick(&root, &[], &filters), ["src/a.cs"]);
    }

    #[test]
    fn filters_apply_to_explicit_files_and_without_ignore_files() {
        let (_d, root) = tree(&["src/a.cs", "tests/c.cs"]);
        let exclude = globs(&["tests/**"]);
        let filters = Filters {
            respect_ignore_files: false,
            exclude: &exclude,
            ..ON
        };
        assert_eq!(pick(&root, &["tests/c.cs"], &filters), Vec::<String>::new());
        assert_eq!(pick(&root, &[], &filters), ["src/a.cs"]);
        let include = globs(&["src/**"]);
        let filters = Filters {
            include: &include,
            ..ON
        };
        assert_eq!(pick(&root, &["tests/c.cs"], &filters), Vec::<String>::new());
        assert_eq!(pick(&root, &["src/a.cs"], &filters), ["src/a.cs"]);
    }

    #[test]
    fn invalid_patterns_are_input_errors_naming_the_pattern() {
        let (_d, root) = tree(&["a.cs"]);
        for (role, bad) in [("include", "[z-a"), ("exclude", "{unclosed")] {
            let patterns = globs(&[bad]);
            let filters = if role == "include" {
                Filters {
                    include: &patterns,
                    ..ON
                }
            } else {
                Filters {
                    exclude: &patterns,
                    ..ON
                }
            };
            let error = select(&root, &[], &filters).unwrap_err();
            assert_eq!(error.kind(), ErrorKind::InvalidInput);
            let message = error.to_string();
            assert!(
                message.contains(&format!("invalid {role} pattern")) && message.contains(bad),
                "{message}"
            );
        }
    }

    #[test]
    fn blank_patterns_are_rejected() {
        let (_d, root) = tree(&["a.cs"]);
        let blank = globs(&["  "]);
        let filters = Filters {
            include: &blank,
            ..ON
        };
        let error = select(&root, &[], &filters).unwrap_err();
        assert!(error.to_string().contains("empty pattern"));
    }
}
