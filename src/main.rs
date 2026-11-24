use anyhow::Result;
use clap::Parser;
use dotnet_crap::baseline;
use dotnet_crap::config::{self, Overrides};
use std::{io::Write, num::NonZeroUsize, path::PathBuf};

/// Config-file and file-selection options shared by scan and `collect`.
#[derive(clap::Args)]
struct SelectionArgs {
    /// Read this `dotnet-crap.json` instead of discovering one in the root
    #[arg(long, value_name = "PATH", conflicts_with = "no_config")]
    config: Option<PathBuf>,
    /// Do not read any `dotnet-crap.json`
    #[arg(long)]
    no_config: bool,
    /// Do not honor `.gitignore` and `.ignore` files
    #[arg(long)]
    no_ignore: bool,
    /// Keep only files matching this root-relative glob (repeatable)
    #[arg(long, value_name = "GLOB")]
    include: Vec<String>,
    /// Drop files matching this root-relative glob (repeatable)
    #[arg(long, value_name = "GLOB")]
    exclude: Vec<String>,
}

/// Turn a flag and its `--no-` counterpart into an optional explicit value.
fn flag(on: bool, off: bool) -> Option<bool> {
    if on {
        Some(true)
    } else if off {
        Some(false)
    } else {
        None
    }
}

#[derive(Parser)]
#[command(
    version,
    about = "Score C# callables using cyclomatic complexity and source-bound coverage",
    after_help = "Fresh coverage collection: dotnet-crap collect --help"
)]
struct Args {
    #[arg(long, default_value = ".")]
    root: PathBuf,
    /// Score threshold [default: 10, or the config value]
    #[arg(long)]
    threshold: Option<f64>,
    #[arg(long, default_value = "text", value_parser = ["text", "json", "sarif"])]
    format: String,
    #[arg(long)]
    output: Option<PathBuf>,
    /// Text report order: `location` (scan order) or `crap` (highest first)
    /// [default: location, or crap with --top]
    #[arg(long, value_parser = ["location", "crap"])]
    sort: Option<String>,
    /// List at most N callables in the text report
    #[arg(long, value_name = "N", value_parser = parse_count)]
    top: Option<NonZeroUsize>,
    /// List only callables above the threshold in the text report
    #[arg(long)]
    failing_only: bool,
    #[arg(long)]
    coverage: Option<PathBuf>,
    #[arg(long, overrides_with = "no_require_coverage")]
    require_coverage: bool,
    /// Override a config `require_coverage: true`
    #[arg(long, overrides_with = "require_coverage")]
    no_require_coverage: bool,
    #[arg(long, overrides_with = "no_fail_on_empty")]
    fail_on_empty: bool,
    /// Override a config `fail_on_empty: true`
    #[arg(long, overrides_with = "fail_on_empty")]
    no_fail_on_empty: bool,
    /// Worker threads for scanning [default: available parallelism]
    #[arg(long, value_name = "N", value_parser = parse_jobs)]
    jobs: Option<NonZeroUsize>,
    /// Conditional compilation symbol for files that do not parse as written
    /// (repeatable; comma- or semicolon-separated lists are accepted)
    #[arg(long = "define", value_name = "SYMBOL")]
    defines: Vec<String>,
    /// Skip files that still do not parse instead of failing the scan
    #[arg(long, overrides_with = "no_skip_unparsed")]
    skip_unparsed: bool,
    /// Override a config `skip_unparsed: true`
    #[arg(long, overrides_with = "skip_unparsed")]
    no_skip_unparsed: bool,
    /// Score the intact callables of files that still do not parse, instead
    /// of failing the scan or skipping the whole file
    #[arg(long, overrides_with = "no_partial_unparsed")]
    partial_unparsed: bool,
    /// Override a config `partial_unparsed: true`
    #[arg(long, overrides_with = "partial_unparsed")]
    no_partial_unparsed: bool,
    /// Compare with this baseline: only new or worse violations fail
    /// [default: the config `baseline`]
    #[arg(long, value_name = "PATH", conflicts_with_all = ["no_baseline", "write_baseline"])]
    baseline: Option<PathBuf>,
    /// Ignore a baseline set in the config
    #[arg(long)]
    no_baseline: bool,
    /// Write the current violations to PATH as a baseline; exits 0 unless
    /// `--require-coverage` or `--fail-on-empty` fails
    #[arg(long, value_name = "PATH")]
    write_baseline: Option<PathBuf>,
    /// Replace an existing `--write-baseline` file
    #[arg(long, requires = "write_baseline")]
    overwrite_baseline: bool,
    /// Match baseline entries of file OLD against file NEW (root-relative;
    /// repeatable)
    #[arg(long = "baseline-move", value_name = "OLD=NEW", value_parser = baseline::parse_move)]
    baseline_moves: Vec<(String, String)>,
    /// Score only selected files that differ from the merge base of REV and
    /// HEAD, including uncommitted and untracked files (needs `git`)
    #[arg(long, value_name = "REV")]
    changed_since: Option<String>,
    #[command(flatten)]
    selection: SelectionArgs,
    paths: Vec<PathBuf>,
}

/// Parse a worker count, rejecting zero and non-numbers with one clear message.
fn parse_jobs(value: &str) -> std::result::Result<NonZeroUsize, String> {
    parse_count(value)
}
/// Parse a positive count, rejecting zero and non-numbers with one clear message.
fn parse_count(value: &str) -> std::result::Result<NonZeroUsize, String> {
    value
        .parse::<NonZeroUsize>()
        .map_err(|_| format!("`{value}` is not a positive integer; use 1 or more"))
}
use dotnet_crap::{scan, terminal_safe, ReportFormat, ScanOptions, TextSort, TextView};

/// The text listing the view flags ask for; `--top` alone lists the highest
/// scores first.
fn text_view(sort: Option<&str>, top: Option<NonZeroUsize>, failing_only: bool) -> TextView {
    let sort = match sort {
        Some("crap") => TextSort::Crap,
        Some(_) => TextSort::Location,
        None if top.is_some() => TextSort::Crap,
        None => TextSort::Location,
    };
    TextView {
        sort,
        failing_only,
        top,
    }
}

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
    /// Worker threads for scanning sources [default: available parallelism]
    #[arg(long, value_name = "N", value_parser = parse_jobs)]
    jobs: Option<NonZeroUsize>,
    /// Conditional compilation symbol for files that do not parse as written
    /// (repeatable; comma- or semicolon-separated lists are accepted)
    #[arg(long = "define", value_name = "SYMBOL")]
    defines: Vec<String>,
    /// Skip files that still do not parse instead of failing the collection
    #[arg(long, overrides_with = "no_skip_unparsed")]
    skip_unparsed: bool,
    /// Override a config `skip_unparsed: true`
    #[arg(long, overrides_with = "skip_unparsed")]
    no_skip_unparsed: bool,
    #[command(flatten)]
    selection: SelectionArgs,
}

/// Split `--define` values on `,` and `;`, as MSBuild `DefineConstants` lists
/// are written, trimming spaces. Empty entries are kept so they are rejected.
fn split_defines(values: Vec<String>) -> Vec<String> {
    values
        .iter()
        .flat_map(|v| v.split([',', ';']))
        .map(|s| s.trim().to_owned())
        .collect()
}

/// Dispatch the reserved collection command while leaving ordinary scan argv unchanged.
fn execute() -> Result<bool> {
    if std::env::args_os().nth(1).is_some_and(|a| a == "collect") {
        let args = CollectArgs::parse_from(
            std::env::args_os()
                .take(1)
                .chain(std::env::args_os().skip(2)),
        );
        let loaded = config::discover(
            &args.root,
            args.selection.config.as_deref(),
            args.selection.no_config,
        )?;
        let resolved = config::resolve(
            loaded.as_ref(),
            &Overrides {
                skip_unparsed: flag(args.skip_unparsed, args.no_skip_unparsed),
                respect_ignore_files: args.selection.no_ignore.then_some(false),
                include: args.selection.include,
                exclude: args.selection.exclude,
                defines: split_defines(args.defines),
                ..Overrides::default()
            },
        );
        dotnet_crap::collector::collect(&dotnet_crap::collector::CollectOptions {
            root: args.root,
            paths: args.paths,
            output: args.output,
            command: args.test_command,
            expected_reports: args.expected_reports,
            timeout: std::time::Duration::from_secs(args.timeout_seconds),
            jobs: args.jobs,
            defines: resolved.defines,
            skip_unparsed: resolved.skip_unparsed,
            respect_ignore_files: resolved.respect_ignore_files,
            include: resolved.include,
            exclude: resolved.exclude,
        })?;
        Ok(false)
    } else {
        run(Args::parse())
    }
}

/// Adapt scan results to CLI output while rejecting input aliases and staging writes.
fn run(args: Args) -> Result<bool> {
    let loaded = config::discover(
        &args.root,
        args.selection.config.as_deref(),
        args.selection.no_config,
    )?;
    let resolved = config::resolve(
        loaded.as_ref(),
        &Overrides {
            threshold: args.threshold,
            require_coverage: flag(args.require_coverage, args.no_require_coverage),
            fail_on_empty: flag(args.fail_on_empty, args.no_fail_on_empty),
            skip_unparsed: flag(args.skip_unparsed, args.no_skip_unparsed),
            partial_unparsed: flag(args.partial_unparsed, args.no_partial_unparsed),
            respect_ignore_files: args.selection.no_ignore.then_some(false),
            include: args.selection.include,
            exclude: args.selection.exclude,
            defines: split_defines(args.defines),
            baseline: args.baseline,
            no_baseline: args.no_baseline,
        },
    );
    // Writing a baseline replaces comparing with one, even a configured one.
    let baseline = if args.write_baseline.is_some() {
        None
    } else {
        resolved.baseline
    };
    let view = text_view(args.sort.as_deref(), args.top, args.failing_only);
    if args.format != "text" && (args.sort.is_some() || args.top.is_some() || args.failing_only) {
        anyhow::bail!("--sort, --top and --failing-only apply only to --format text");
    }
    if args.changed_since.is_some() && args.write_baseline.is_some() {
        anyhow::bail!("--write-baseline records the whole selection; it cannot be combined with --changed-since");
    }
    if let (Some(output), Some(written)) = (&args.output, &args.write_baseline) {
        if dotnet_crap::output::same_destination(output, written)? {
            anyhow::bail!("--output and --write-baseline name the same file");
        }
    }
    let mut result = scan(&ScanOptions {
        root: args.root,
        threshold: resolved.threshold,
        coverage: args.coverage,
        require_coverage: resolved.require_coverage,
        fail_on_empty: resolved.fail_on_empty,
        paths: args.paths,
        jobs: args.jobs,
        defines: resolved.defines,
        skip_unparsed: resolved.skip_unparsed,
        partial_unparsed: resolved.partial_unparsed,
        respect_ignore_files: resolved.respect_ignore_files,
        include: resolved.include,
        exclude: resolved.exclude,
        baseline,
        baseline_moves: args.baseline_moves,
        changed_since: args.changed_since,
    })?;
    if args.write_baseline.is_some() {
        result.accept_scores();
    }
    let format = match args.format.as_str() {
        "json" => ReportFormat::Json,
        "sarif" => ReportFormat::Sarif,
        _ => ReportFormat::Text,
    };
    let output = if format == ReportFormat::Text {
        result.render_text(&view)
    } else {
        result.render(format)?
    };
    result.verify_sources()?;
    if let Some(path) = args.output {
        dotnet_crap::output::write_report(&result, &output, &path)?;
    } else {
        std::io::stdout().lock().write_all(output.as_bytes())?;
    }
    if let Some(path) = args.write_baseline {
        let entries = result.write_baseline(&path, args.overwrite_baseline)?;
        eprintln!(
            "wrote baseline with {entries} entries to {}",
            path.display()
        );
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
