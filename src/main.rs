use anyhow::{bail, Result};
use clap::Parser;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};
use walkdir::WalkDir;

#[derive(Parser)]
#[command(
    version,
    about = "Score C# callables using cyclomatic complexity and source-bound coverage"
)]
struct Args {
    #[arg(long, default_value = ".")]
    root: PathBuf,
    #[arg(long, default_value_t = 10.0)]
    threshold: f64,
    #[arg(long, default_value = "text", value_parser = ["text", "json", "sarif"])]
    format: String,
    #[arg(long)]
    output: Option<PathBuf>,
    #[arg(long)]
    coverage: Option<PathBuf>,
    #[arg(long)]
    require_coverage: bool,
    #[arg(long)]
    fail_on_empty: bool,
    paths: Vec<PathBuf>,
}
use dotnet_crap::{scan, terminal_safe, ReportFormat, ScanOptions};

/// Collect fresh evidence with an explicitly trusted argv after `--`.
#[derive(Parser)]
#[command(
    name = "dotnet-crap collect",
    about = "Collect fresh Coverlet Cobertura as native v1 evidence"
)]
struct CollectArgs {
    #[arg(long, default_value = ".")]
    root: PathBuf,
    #[arg(long)]
    output: PathBuf,
    #[arg(long = "path")]
    paths: Vec<PathBuf>,
    #[arg(last = true, required = true)]
    test_command: Vec<std::ffi::OsString>,
    #[arg(long, default_value_t = 1)]
    expected_reports: usize,
}

fn execute() -> Result<bool> {
    if std::env::args_os().nth(1).is_some_and(|a| a == "collect") {
        let args = CollectArgs::parse_from(
            std::env::args_os()
                .take(1)
                .chain(std::env::args_os().skip(2)),
        );
        dotnet_crap::collector::collect(&dotnet_crap::collector::CollectOptions {
            scan: ScanOptions {
                root: args.root,
                paths: args.paths,
                ..Default::default()
            },
            output: args.output,
            command: args.test_command,
            expected_reports: args.expected_reports,
        })?;
        Ok(false)
    } else {
        run(Args::parse())
    }
}

/// Adapt scan results to CLI output while rejecting input aliases and staging writes.
fn run(args: Args) -> Result<bool> {
    let result = scan(&ScanOptions {
        root: args.root,
        threshold: args.threshold,
        coverage: args.coverage,
        require_coverage: args.require_coverage,
        fail_on_empty: args.fail_on_empty,
        paths: args.paths,
    })?;
    let format = match args.format.as_str() {
        "json" => ReportFormat::Json,
        "sarif" => ReportFormat::Sarif,
        _ => ReportFormat::Text,
    };
    let output = result.render(format)?;
    result.verify_sources()?;
    if let Some(path) = args.output {
        if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("cs"))
        {
            bail!("report output cannot overwrite C# source");
        }
        if fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
            bail!("report output cannot be a symbolic link");
        }
        if path.exists() {
            for input in result.input_paths().iter() {
                if same_file::is_same_file(&path, input)? {
                    bail!("report output aliases an input");
                }
            }
            // Also protect ignored/generated/unselected sources. Do not follow
            // directory links while looking for aliases.
            for entry in WalkDir::new(result.root()).follow_links(false) {
                let entry = entry?;
                if entry
                    .path()
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("cs"))
                    && entry.path().is_file()
                    && same_file::is_same_file(&path, entry.path())?
                {
                    bail!("report output aliases C# source");
                }
            }
        }
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut staged = tempfile::NamedTempFile::new_in(parent)?;
        staged.write_all(output.as_bytes())?;
        staged.as_file().sync_all()?;
        result.verify_sources()?;
        staged.persist(path)?;
    } else {
        std::io::stdout().lock().write_all(output.as_bytes())?;
    }
    Ok(result.report().failed)
}
/// Parse CLI arguments and translate pass, policy failure and input errors into exits 0, 1 and 2.
fn main() {
    let code = match execute() {
        Ok(false) => 0,
        Ok(true) => 1,
        Err(e) => {
            eprintln!("error: {}", terminal_safe(&format!("{e:#}")));
            2
        }
    };
    std::process::exit(code);
}
