//! Execute an explicitly trusted test command and publish fresh native evidence.
use crate::{scan, ScanOptions};
use anyhow::{bail, Context, Result};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, ExitStatus},
    time::{Duration, Instant},
};
use walkdir::WalkDir;

/// Collection policy. The caller is responsible for trusting the entire command
/// and pinning its Coverlet dependency. No shell is inserted by this API.
#[derive(Debug, Clone)]
pub struct CollectOptions {
    /// Root containing selected sources. Collection does not apply scoring gates.
    pub root: PathBuf,
    /// Selected files/directories, relative to root; empty selects the root.
    pub paths: Vec<PathBuf>,
    /// New native v1 evidence destination. Existing destinations are rejected.
    pub output: PathBuf,
    /// Executable and argv, containing exactly one standalone `{results}` token.
    /// Use it as the value of `dotnet test --results-directory` and explicitly
    /// select XPlat Code Coverage with Cobertura format.
    pub command: Vec<OsString>,
    /// Exact expected report-file count (not project identity); must be positive.
    pub expected_reports: usize,
    /// Deadline for the immediate trusted test process; must be nonzero.
    pub timeout: Duration,
}

/// Snapshot selected bytes, run trusted tests in the canonical source root, then
/// validate all fresh Cobertura reports before atomically creating `output`.
/// Errors leave no new coverage artifact. This operation never runs during scan.
pub fn collect(options: &CollectOptions) -> Result<()> {
    if options.expected_reports == 0 {
        bail!("expected report count must be positive");
    }
    if options.timeout.is_zero() {
        bail!("test timeout must be positive");
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
    let selection = ScanOptions {
        root: options.root.clone(),
        paths: options.paths.clone(),
        ..Default::default()
    };
    let snapshot = scan(&selection)?;
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
    let mut results_builder = tempfile::Builder::new();
    results_builder.prefix("dotnet-crap-coverlet-");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        results_builder.permissions(fs::Permissions::from_mode(0o700));
    }
    let results = results_builder.tempdir()?;
    let mut command = Command::new(&options.command[0]);
    command
        .args(options.command[1..].iter().map(|arg| {
            if arg == "{results}" {
                results.path().as_os_str()
            } else {
                arg.as_os_str()
            }
        }))
        .current_dir(snapshot.root());
    let status = run_command(&mut command, options.timeout)?;
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
    let mut verify = selection;
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

/// Bound the immediate child, reaping it on timeout and on wait failures.
fn run_command(command: &mut Command, timeout: Duration) -> Result<ExitStatus> {
    let mut child = command.spawn().context("starting trusted test command")?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => {}
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error).context("waiting for trusted test command");
            }
        }
        if start.elapsed() >= timeout {
            let _ = child.kill();
            child.wait().context("reaping timed-out test command")?;
            bail!("trusted test command timed out");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
