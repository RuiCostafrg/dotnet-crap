use anyhow::Result;
use clap::Parser;
use std::{io::Write, path::PathBuf};

#[derive(Parser)]
#[command(
    version,
    about = "Score C# callables using cyclomatic complexity and source-bound coverage",
    after_help = "Fresh coverage collection: dotnet-crap collect --help"
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
    #[arg(long, default_value_t = 900)]
    timeout_seconds: u64,
}

/// Dispatch the reserved collection command while leaving ordinary scan argv unchanged.
fn execute() -> Result<bool> {
    if std::env::args_os().nth(1).is_some_and(|a| a == "collect") {
        let args = CollectArgs::parse_from(
            std::env::args_os()
                .take(1)
                .chain(std::env::args_os().skip(2)),
        );
        dotnet_crap::collector::collect(&dotnet_crap::collector::CollectOptions {
            root: args.root,
            paths: args.paths,
            output: args.output,
            command: args.test_command,
            expected_reports: args.expected_reports,
            timeout: std::time::Duration::from_secs(args.timeout_seconds),
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
        dotnet_crap::output::write_report(&result, &output, &path)?;
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
