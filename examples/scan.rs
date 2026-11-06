//! Scan a C# project through the library API without starting the CLI.
//!
//! Run from the package root with `cargo run --example scan -- .` (or pass a
//! directory containing C# source files).

use anyhow::Result;
use dotnet_crap::{scan, ReportFormat, ScanOptions};
use std::{env, path::PathBuf};

/// Scan the requested root directly through the library and print the text report.
fn main() -> Result<()> {
    let root = env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let options = ScanOptions {
        root,
        ..ScanOptions::default()
    };
    let result = scan(&options)?;
    print!("{}", result.render(ReportFormat::Text)?);
    Ok(())
}
