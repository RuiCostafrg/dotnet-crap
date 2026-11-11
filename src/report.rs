//! Typed report model and text rendering.

use serde::Serialize;

/// Supported report output formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportFormat {
    /// Human-readable terminal output.
    Text,
    /// Structured JSON report.
    Json,
    /// SARIF 2.1.0 report.
    Sarif,
}
/// A scored callable and its source location.
#[derive(Debug, Serialize)]
pub struct Item {
    /// Root-relative source path.
    pub file: String,
    /// Display name derived from the callable syntax.
    pub name: String,
    /// Syntax node kind.
    pub kind: String,
    /// One-based starting line.
    pub line: usize,
    /// One-based starting byte column.
    pub column: usize,
    /// One-based ending line.
    pub end_line: usize,
    /// Cyclomatic complexity score.
    pub complexity: u32,
    /// Fraction of supplied coverage points hit, or absent without evidence.
    pub coverage: Option<f64>,
    /// Calculated CRAP score.
    pub crap: f64,
}
/// Ordered scan results and the resulting policy decision.
#[derive(Debug, Serialize)]
pub struct Report {
    /// Score threshold used for the policy; equality passes.
    pub threshold: f64,
    /// Number of selected source files.
    pub files_scanned: usize,
    /// Whether every callable has coverage evidence.
    pub coverage_complete: bool,
    /// Callables in deterministic scan order.
    pub functions: Vec<Item>,
    /// Whether any configured policy failed.
    pub failed: bool,
}
/// Escape control characters before writing untrusted source names or errors to a terminal.
pub fn terminal_safe(s: &str) -> String {
    let mut text = String::with_capacity(s.len());
    for character in s.chars() {
        if character.is_control() {
            text.extend(character.escape_default());
        } else {
            text.push(character);
        }
    }
    text
}
/// Render the human-readable text report using the scan's coverage policy.
pub(crate) fn render_text(report: &Report, require_coverage: bool) -> String {
    let mut s = String::new();
    if report.functions.is_empty() {
        s.push_str("No implemented C# callables found.\n");
    }
    if require_coverage && !report.coverage_complete {
        s.push_str("Coverage missing for one or more callables.\n");
    }
    for i in &report.functions {
        s.push_str(&format!(
            "{}:{} {} complexity={} coverage={} CRAP={:.2}\n",
            terminal_safe(&i.file),
            i.line,
            terminal_safe(&i.name),
            i.complexity,
            i.coverage
                .map_or_else(|| "absent".into(), |v| format!("{:.1}%", v * 100.0)),
            i.crap
        ));
    }
    s
}
