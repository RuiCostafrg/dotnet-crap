//! Scan a C# project through the library API without starting the CLI.
//!
//! Run from the package root with `cargo run --example scan -- .` (or pass a
//! directory containing C# source files).

use anyhow::Result;
use dotnet_crap::{scan, terminal_safe, ReportFormat, ScanOptions};
use std::{env, io::Write, path::PathBuf};

/// Scan the requested root directly through the library and print the text report.
fn run() -> Result<()> {
    let root = env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let options = ScanOptions {
        root,
        ..ScanOptions::default()
    };
    let result = scan(&options)?;
    let output = result.render(ReportFormat::Text)?;
    result.verify_sources()?;
    std::io::stdout().lock().write_all(output.as_bytes())?;
    Ok(())
}

/// Escape untrusted diagnostic paths before writing errors to the terminal.
fn main() {
    if let Err(error) = run() {
        eprintln!("error: {}", terminal_safe(&format!("{error:#}")));
        std::process::exit(2);
    }
}
