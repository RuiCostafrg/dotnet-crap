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
/// How a failing callable compares with the baseline it was matched against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BaselineState {
    /// No baseline entry matches: a violation that is not accepted.
    New,
    /// Matched an entry whose CRAP score was lower (compared at two decimals).
    Worse,
    /// Matched an entry with the same CRAP score (compared at two decimals).
    Unchanged,
    /// Matched an entry whose CRAP score was higher but still fails.
    Improved,
}
/// Counts of each baseline outcome in one scan.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct BaselineSummary {
    /// Failing callables with no matching baseline entry.
    pub new: usize,
    /// Failing callables whose score rose above the baseline's.
    pub worse: usize,
    /// Failing callables whose score equals the baseline's.
    pub unchanged: usize,
    /// Failing callables whose score fell but still fails.
    pub improved: usize,
    /// Baseline entries that match no failing callable now.
    pub resolved: usize,
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
    /// Enclosing namespaces, types and callables, joined with `.`.
    pub scope: String,
    /// Name plus parameter types, such as `Parse(string,int)`.
    pub signature: String,
    /// Baseline outcome of a failing callable; absent without a baseline and
    /// for callables that pass.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub baseline: Option<BaselineState>,
    /// Hash of the callable's tokens, written only into baselines.
    #[serde(skip)]
    pub(crate) fingerprint: String,
    /// Index among same-file callables with the same kind, scope and signature.
    #[serde(skip)]
    pub(crate) occurrence: usize,
    /// CRAP score of the matched baseline entry.
    #[serde(skip)]
    pub(crate) baseline_crap: Option<f64>,
}
/// A selected source skipped because it does not parse.
#[derive(Debug, Serialize)]
pub struct SkippedFile {
    /// Root-relative source path.
    pub path: String,
    /// Syntax error message that would have failed the scan.
    pub message: String,
}
/// A selected source that did not parse, of which only the intact callables
/// were scored.
#[derive(Debug, Serialize)]
pub struct PartialFile {
    /// Root-relative source path.
    pub path: String,
    /// Syntax error message that would have failed the scan.
    pub message: String,
    /// Callables found in the file but not scored because their syntax, or
    /// that of an enclosing callable, has an error.
    pub unscored: usize,
}
/// Ordered scan results and the resulting policy decision.
#[derive(Debug, Serialize)]
pub struct Report {
    /// JSON report format version; always 1 in this release line.
    pub version: u32,
    /// Score threshold used for the policy; equality passes.
    pub threshold: f64,
    /// Number of selected source files.
    pub files_scanned: usize,
    /// Selected sources skipped because they did not parse, in scan order.
    #[serde(rename = "skipped_files", skip_serializing_if = "Vec::is_empty")]
    pub skipped: Vec<SkippedFile>,
    /// Selected sources that did not parse and were scored only partially,
    /// in scan order.
    #[serde(rename = "partial_files", skip_serializing_if = "Vec::is_empty")]
    pub partial: Vec<PartialFile>,
    /// Whether every callable has coverage evidence.
    pub coverage_complete: bool,
    /// Callables in deterministic scan order.
    pub functions: Vec<Item>,
    /// Whether any configured policy failed.
    pub failed: bool,
    /// Baseline comparison counts; absent unless a baseline was used.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub baseline: Option<BaselineSummary>,
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
/// The ` [state]` suffix of a failing callable's text line, empty without a baseline outcome.
fn baseline_suffix(item: &Item) -> String {
    let was = item.baseline_crap.unwrap_or_default();
    match item.baseline {
        None => String::new(),
        Some(BaselineState::New) => " [new]".into(),
        Some(BaselineState::Worse) => format!(" [worse, was {was:.2}]"),
        Some(BaselineState::Unchanged) => " [unchanged]".into(),
        Some(BaselineState::Improved) => format!(" [improved, was {was:.2}]"),
    }
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
            "{}:{} {} complexity={} coverage={} CRAP={:.2}{}\n",
            terminal_safe(&i.file),
            i.line,
            terminal_safe(&i.name),
            i.complexity,
            i.coverage
                .map_or_else(|| "absent".into(), |v| format!("{:.1}%", v * 100.0)),
            i.crap,
            baseline_suffix(i)
        ));
    }
    if let Some(b) = &report.baseline {
        s.push_str(&format!(
            "baseline: {} new, {} worse, {} unchanged, {} improved, {} resolved\n",
            b.new, b.worse, b.unchanged, b.improved, b.resolved
        ));
    }
    if !report.partial.is_empty() {
        s.push_str(&format!(
            "partially scored {} unparsed file(s):\n",
            report.partial.len()
        ));
        for file in &report.partial {
            s.push_str(&format!(
                "  {}: {}; {} callable(s) not scored\n",
                terminal_safe(&file.path),
                terminal_safe(&file.message),
                file.unscored
            ));
        }
    }
    if !report.skipped.is_empty() {
        s.push_str(&format!(
            "skipped {} unparsed file(s):\n",
            report.skipped.len()
        ));
        for file in &report.skipped {
            s.push_str(&format!(
                "  {}: {}\n",
                terminal_safe(&file.path),
                terminal_safe(&file.message)
            ));
        }
    }
    s
}
