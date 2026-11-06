//! Execute an explicitly trusted test command and publish fresh native evidence.
use crate::{scan, ScanOptions};
use anyhow::{bail, Context, Result};
use std::{collections::BTreeMap, ffi::OsString, fs, io::Write, path::PathBuf, process::Command};
use walkdir::WalkDir;

/// Collection policy. The caller is responsible for trusting the entire command
/// and pinning its Coverlet dependency. No shell is inserted by this API.
#[derive(Debug, Clone)]
pub struct CollectOptions {
    /// Source selection; coverage must be absent.
    pub scan: ScanOptions,
    /// New native v1 evidence destination. Existing destinations are rejected.
    pub output: PathBuf,
    /// Executable and argv, containing exactly one standalone `{results}` token.
    /// Use it as the value of `dotnet test --results-directory` and explicitly
    /// select XPlat Code Coverage with Cobertura format.
    pub command: Vec<OsString>,
    /// Exact expected number of test-project/target Cobertura reports; must be positive.
    pub expected_reports: usize,
}

/// Snapshot selected bytes, run trusted tests in the canonical source root, then
/// validate all fresh Cobertura reports before atomically creating `output`.
/// Errors leave no new coverage artifact. This operation never runs during scan.
pub fn collect(options: &CollectOptions) -> Result<()> {
    if options.expected_reports == 0 {
        bail!("expected report count must be positive");
    }
    if options.scan.coverage.is_some() {
        bail!("collection cannot use existing coverage");
    }
    if options.command.len() < 2
        || options.command[1..]
            .iter()
            .filter(|a| *a == "{results}")
            .count()
            != 1
    {
        bail!("test command requires exactly one standalone {{results}} argument");
    }
    validate_destination(&options.output)?;
    let snapshot = scan(&options.scan)?;
    let mut sources = BTreeMap::new();
    let mut hashes = BTreeMap::new();
    for (path, hash) in &snapshot.source_hashes {
        let relative = path
            .strip_prefix(snapshot.root())?
            .to_str()
            .context("non-UTF-8 source path")?
            .replace('\\', "/");
        if relative.contains(':') || relative.contains('\\') {
            bail!("unsupported source path for native coverage");
        }
        let bytes = fs::read(path)?;
        sources.insert(relative.clone(), crate::decode_source(&bytes)?);
        hashes.insert(relative, hash.clone());
    }
    if sources.is_empty() {
        bail!("collection requires selected sources");
    }
    snapshot.verify_sources()?;
    let results = tempfile::Builder::new()
        .prefix("dotnet-crap-coverlet-")
        .tempdir()?;
    let status = Command::new(&options.command[0])
        .args(options.command[1..].iter().map(|arg| {
            if arg == "{results}" {
                results.path().as_os_str()
            } else {
                arg.as_os_str()
            }
        }))
        .current_dir(snapshot.root())
        .status()
        .context("starting trusted test command")?;
    if !status.success() {
        bail!("trusted test command failed: {status}");
    }
    snapshot.verify_sources()?;
    let mut reports = Vec::new();
    for entry in WalkDir::new(results.path()).follow_links(false) {
        let entry = entry?;
        if entry.file_type().is_symlink() {
            bail!("collector results cannot contain symbolic links");
        }
        if entry.file_name() == "coverage.cobertura.xml" && entry.file_type().is_file() {
            reports.push(entry.into_path());
        }
    }
    reports.sort();
    if reports.len() != options.expected_reports {
        bail!(
            "expected {} fresh coverage.cobertura.xml reports, found {}",
            options.expected_reports,
            reports.len()
        );
    }
    let lines = crate::cobertura::parse_reports(snapshot.root(), &sources, &reports)?;
    let files = lines
        .into_iter()
        .map(|(name, points)| {
            let sha256 = hashes[&name].clone();
            (
                name,
                crate::CoverageFile {
                    sha256,
                    lines: points
                        .into_iter()
                        .map(|(line, hits)| (line.to_string(), hits))
                        .collect(),
                },
            )
        })
        .collect();
    let evidence = crate::Coverage { version: 1, files };
    let parent = options
        .output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(std::path::Path::new("."));
    let mut stage = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer_pretty(&mut stage, &evidence)?;
    stage.write_all(b"\n")?;
    stage.as_file().sync_all()?;
    // Exercise the native validator and scoring on exactly the staged evidence.
    let mut verify = options.scan.clone();
    verify.coverage = Some(stage.path().to_owned());
    scan(&verify)?;
    snapshot.verify_sources()?;
    validate_destination(&options.output)?;
    stage
        .persist_noclobber(&options.output)
        .context("publishing native coverage")?;
    Ok(())
}

/// Refuse existing destinations (including hardlinks/symlinks) and C# names.
fn validate_destination(path: &std::path::Path) -> Result<()> {
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("cs"))
    {
        bail!("coverage output cannot overwrite C# source");
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(std::path::Path::new("."));
    if !parent.is_dir() {
        bail!("coverage destination parent must be an existing directory");
    }
    match fs::symlink_metadata(path) {
        Ok(_) => bail!("coverage destination must not already exist"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}
