//! Regression baselines: a committed record of today's violations, and the
//! matching that tells later runs which violations are new or worse.
//!
//! See `docs/BASELINES.md` for the file format and the matching rules.
use crate::error::{Error, ErrorKind, Result};
use crate::report::{BaselineState, BaselineSummary};
use crate::Item;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, VecDeque},
    hash::Hash,
    path::Path,
};

/// The only baseline schema version this build reads and writes.
const SUPPORTED_VERSION: i64 = 1;

/// A committed baseline: the callables that failed the threshold when it was written.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Baseline {
    /// Schema version; must be 1.
    pub version: i64,
    /// The threshold in force when the baseline was written.
    pub threshold: f64,
    /// Failing callables, ordered by file, then line, then signature.
    pub entries: Vec<Entry>,
}

/// One accepted violation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    /// Root-relative source path.
    pub file: String,
    /// Syntax node kind.
    pub kind: String,
    /// Enclosing namespaces, types and callables, joined with `.`.
    pub scope: String,
    /// Name plus parameter types.
    pub signature: String,
    /// Index among same-file callables with the same kind, scope and signature.
    pub occurrence: usize,
    /// Hash of the callable's tokens.
    pub fingerprint: String,
    /// One-based starting line when written; informational only.
    pub line: usize,
    /// Cyclomatic complexity when written.
    pub complexity: u32,
    /// Coverage fraction when written, if there was evidence.
    pub coverage: Option<f64>,
    /// CRAP score when written.
    pub crap: f64,
}

impl Baseline {
    /// Capture the callables of `items` that exceed `threshold`, in file, line
    /// and signature order.
    pub fn from_items(items: &[Item], threshold: f64) -> Self {
        let mut entries: Vec<Entry> = items
            .iter()
            .filter(|item| item.crap > threshold)
            .map(|item| Entry {
                file: item.file.clone(),
                kind: item.kind.clone(),
                scope: item.scope.clone(),
                signature: item.signature.clone(),
                occurrence: item.occurrence,
                fingerprint: item.fingerprint.clone(),
                line: item.line,
                complexity: item.complexity,
                coverage: item.coverage,
                crap: item.crap,
            })
            .collect();
        entries
            .sort_by(|a, b| (&a.file, a.line, &a.signature).cmp(&(&b.file, b.line, &b.signature)));
        Self {
            version: SUPPORTED_VERSION,
            threshold,
            entries,
        }
    }

    /// Render the pretty JSON file text, ending in a newline.
    pub fn render(&self) -> Result<String> {
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| Error::new(ErrorKind::Render, e.to_string()))?;
        Ok(format!("{text}\n"))
    }
}

/// Build the error for a baseline file that cannot be used.
fn invalid(path: &Path, reason: impl std::fmt::Display) -> Error {
    Error::new(
        ErrorKind::InvalidInput,
        format!("invalid baseline {}: {reason}", path.display()),
    )
}

/// Read and validate a baseline file. Every failure is an `InvalidInput` error
/// whose message names `path`.
pub fn read(path: &Path) -> Result<Baseline> {
    let text = std::fs::read_to_string(path).map_err(|e| invalid(path, e))?;
    let baseline: Baseline = serde_json::from_str(&text).map_err(|e| invalid(path, e))?;
    if baseline.version != SUPPORTED_VERSION {
        return Err(invalid(
            path,
            format!(
                "unsupported baseline version {}; this dotnet-crap reads version {SUPPORTED_VERSION}",
                baseline.version
            ),
        ));
    }
    Ok(baseline)
}

/// Reject a baseline written under a different threshold, whose entries would
/// otherwise be compared against the wrong set of violations.
pub(crate) fn check_threshold(baseline: &Baseline, threshold: f64) -> Result<()> {
    if baseline.threshold == threshold {
        return Ok(());
    }
    Err(Error::new(
        ErrorKind::InvalidInput,
        format!(
            "baseline threshold {} does not match --threshold {threshold}",
            baseline.threshold
        ),
    ))
}

/// How one failing callable was classified, and the baseline score it was matched to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Classified {
    /// The outcome.
    pub state: BaselineState,
    /// The matched entry's CRAP score; absent for a new violation.
    pub was: Option<f64>,
}

/// The result of comparing the current failing callables with a baseline.
#[derive(Debug, Clone, PartialEq)]
pub struct Comparison {
    /// One slot per input item: the classification of a failing item, `None`
    /// for an item that passes.
    pub items: Vec<Option<Classified>>,
    /// Indices into the baseline's entries that match no failing item now:
    /// fixed, removed, or no longer over the threshold.
    pub resolved: Vec<usize>,
}

impl Comparison {
    /// Count each outcome.
    pub fn summary(&self) -> BaselineSummary {
        let mut summary = BaselineSummary {
            resolved: self.resolved.len(),
            ..BaselineSummary::default()
        };
        for classified in self.items.iter().flatten() {
            match classified.state {
                BaselineState::New => summary.new += 1,
                BaselineState::Worse => summary.worse += 1,
                BaselineState::Unchanged => summary.unchanged += 1,
                BaselineState::Improved => summary.improved += 1,
            }
        }
        summary
    }
}

/// A score in hundredths, the precision reports print and baselines compare at.
fn cents(score: f64) -> f64 {
    (score * 100.0).round()
}

/// Pair entries with items that share a key none of the other unmatched
/// entries or items share. Pairs are returned in entry order.
fn unique_pairs<K: Hash + Eq>(
    entries: &[usize],
    items: &[usize],
    entry_key: impl Fn(usize) -> K,
    item_key: impl Fn(usize) -> K,
) -> Vec<(usize, usize)> {
    let mut by_entry: HashMap<K, Vec<usize>> = HashMap::new();
    for &e in entries {
        by_entry.entry(entry_key(e)).or_default().push(e);
    }
    let mut by_item: HashMap<K, Vec<usize>> = HashMap::new();
    for &i in items {
        by_item.entry(item_key(i)).or_default().push(i);
    }
    let mut pairs: Vec<(usize, usize)> = by_entry
        .iter()
        .filter_map(
            |(key, es)| match (es.as_slice(), by_item.get(key)?.as_slice()) {
                (&[e], &[i]) => Some((e, i)),
                _ => None,
            },
        )
        .collect();
    pairs.sort_unstable();
    pairs
}

/// Compare the failing callables among `items` with a baseline's `entries`.
///
/// `moves` rewrites the `file` of entries, `(old, new)` by exact match, before
/// matching. Each step below considers only entries and items still unmatched:
///
/// 1. the same file, kind, scope, signature and occurrence;
/// 2. the same fingerprint and kind, when no other unmatched entry or item has
///    that fingerprint (a moved file, or a renamed callable with the same body);
/// 3. the same kind, scope, signature and occurrence in different files, when
///    no other unmatched entry or item has that identity (a moved file whose
///    body also changed).
///
/// Anything else is left unmatched, so an ambiguity never hides a violation.
pub fn compare(
    entries: &[Entry],
    items: &[Item],
    threshold: f64,
    moves: &[(String, String)],
) -> Comparison {
    let file_of = |e: usize| {
        let file = entries[e].file.as_str();
        moves
            .iter()
            .find(|(old, _)| old == file)
            .map_or(file, |(_, new)| new.as_str())
    };
    let failing: Vec<usize> = (0..items.len())
        .filter(|&i| items[i].crap > threshold)
        .collect();
    let mut matched: Vec<Option<usize>> = vec![None; items.len()];
    let mut taken = vec![false; entries.len()];

    // Step 1: exact identity, taking same-key entries in file order.
    let mut by_identity: HashMap<(&str, &str, &str, &str, usize), VecDeque<usize>> = HashMap::new();
    for (e, entry) in entries.iter().enumerate() {
        by_identity
            .entry((
                file_of(e),
                &entry.kind,
                &entry.scope,
                &entry.signature,
                entry.occurrence,
            ))
            .or_default()
            .push_back(e);
    }
    for &i in &failing {
        let item = &items[i];
        let key = (
            item.file.as_str(),
            item.kind.as_str(),
            item.scope.as_str(),
            item.signature.as_str(),
            item.occurrence,
        );
        if let Some(e) = by_identity.get_mut(&key).and_then(VecDeque::pop_front) {
            matched[i] = Some(e);
            taken[e] = true;
        }
    }

    // Steps 2 and 3 run over what is left, each seeing the previous step's matches.
    let unmatched = |taken: &[bool], matched: &[Option<usize>]| {
        let entries: Vec<usize> = (0..taken.len()).filter(|&e| !taken[e]).collect();
        let items: Vec<usize> = failing
            .iter()
            .copied()
            .filter(|&i| matched[i].is_none())
            .collect();
        (entries, items)
    };
    let (left_entries, left_items) = unmatched(&taken, &matched);
    let pairs = unique_pairs(
        &left_entries,
        &left_items,
        |e| entries[e].fingerprint.as_str(),
        |i| items[i].fingerprint.as_str(),
    );
    for (e, i) in pairs {
        if entries[e].kind == items[i].kind {
            matched[i] = Some(e);
            taken[e] = true;
        }
    }
    let (left_entries, left_items) = unmatched(&taken, &matched);
    let pairs = unique_pairs(
        &left_entries,
        &left_items,
        |e| {
            let entry = &entries[e];
            (
                entry.kind.as_str(),
                entry.scope.as_str(),
                entry.signature.as_str(),
                entry.occurrence,
            )
        },
        |i| {
            let item = &items[i];
            (
                item.kind.as_str(),
                item.scope.as_str(),
                item.signature.as_str(),
                item.occurrence,
            )
        },
    );
    for (e, i) in pairs {
        matched[i] = Some(e);
        taken[e] = true;
    }

    let classified = (0..items.len())
        .map(|i| {
            if items[i].crap <= threshold {
                return None;
            }
            Some(match matched[i] {
                None => Classified {
                    state: BaselineState::New,
                    was: None,
                },
                Some(e) => {
                    let was = entries[e].crap;
                    let state = match cents(items[i].crap).total_cmp(&cents(was)) {
                        std::cmp::Ordering::Greater => BaselineState::Worse,
                        std::cmp::Ordering::Equal => BaselineState::Unchanged,
                        std::cmp::Ordering::Less => BaselineState::Improved,
                    };
                    Classified {
                        state,
                        was: Some(was),
                    }
                }
            })
        })
        .collect();
    Comparison {
        items: classified,
        resolved: (0..entries.len()).filter(|&e| !taken[e]).collect(),
    }
}

/// Parse a `--baseline-move OLD=NEW` value into its two root-relative paths.
pub fn parse_move(value: &str) -> Result<(String, String)> {
    match value.split_once('=') {
        Some((old, new)) if !old.is_empty() && !new.is_empty() => {
            Ok((old.to_owned(), new.to_owned()))
        }
        _ => Err(Error::new(
            ErrorKind::InvalidInput,
            format!("invalid baseline move {value:?}: expected OLD=NEW with both paths non-empty"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A current callable with the given identity, fingerprint and score.
    #[allow(clippy::too_many_arguments)]
    fn item(
        file: &str,
        scope: &str,
        signature: &str,
        occurrence: usize,
        fingerprint: &str,
        crap: f64,
    ) -> Item {
        Item {
            file: file.into(),
            name: signature.split('(').next().unwrap().into(),
            kind: "method_declaration".into(),
            line: 1,
            column: 1,
            end_line: 2,
            complexity: 5,
            coverage: None,
            crap,
            scope: scope.into(),
            signature: signature.into(),
            baseline: None,
            fingerprint: fingerprint.into(),
            occurrence,
            baseline_crap: None,
        }
    }

    /// The baseline entry written for `item`.
    fn entry(item: &Item) -> Entry {
        Baseline::from_items(std::slice::from_ref(item), 0.0)
            .entries
            .remove(0)
    }

    fn states(c: &Comparison) -> Vec<Option<BaselineState>> {
        c.items.iter().map(|x| x.map(|x| x.state)).collect()
    }

    use BaselineState::{Improved, New, Unchanged, Worse};

    const T: f64 = 10.0;

    #[test]
    fn exact_identity_matches_and_ignores_line_and_body() {
        let was = item("A.cs", "N.A", "M(int)", 0, "f1", 12.0);
        let mut now = item("A.cs", "N.A", "M(int)", 0, "other", 12.0);
        now.line = 99;
        let c = compare(&[entry(&was)], &[now], T, &[]);
        assert_eq!(states(&c), [Some(Unchanged)]);
        assert!(c.resolved.is_empty());
    }

    #[test]
    fn overloads_match_by_signature_not_order() {
        let a = item("A.cs", "N.A", "M(int)", 0, "f1", 12.0);
        let b = item("A.cs", "N.A", "M(string)", 0, "f2", 20.0);
        // Same overloads in the opposite order, bodies edited, `string` worse.
        let now = [
            item("A.cs", "N.A", "M(string)", 0, "g2", 25.0),
            item("A.cs", "N.A", "M(int)", 0, "g1", 12.0),
        ];
        let c = compare(&[entry(&a), entry(&b)], &now, T, &[]);
        assert_eq!(states(&c), [Some(Worse), Some(Unchanged)]);
        assert_eq!(c.items[0].unwrap().was, Some(20.0));
        assert_eq!(c.items[1].unwrap().was, Some(12.0));
    }

    #[test]
    fn identical_signatures_match_by_occurrence() {
        let first = item("A.cs", "N.A", "lambda_expression()", 0, "f1", 11.0);
        let second = item("A.cs", "N.A", "lambda_expression()", 1, "f2", 30.0);
        let now = [
            item("A.cs", "N.A", "lambda_expression()", 0, "g1", 11.0),
            item("A.cs", "N.A", "lambda_expression()", 1, "g2", 30.0),
        ];
        let c = compare(&[entry(&first), entry(&second)], &now, T, &[]);
        assert_eq!(states(&c), [Some(Unchanged), Some(Unchanged)]);
        // A differently numbered occurrence is not the entry's occurrence.
        let c = compare(&[entry(&second)], &now[..1], T, &[]);
        assert_eq!(states(&c), [Some(New)]);
        assert_eq!(c.resolved, [0]);
    }

    #[test]
    fn a_renamed_file_matches_by_fingerprint() {
        let was = item("Old.cs", "N.A", "M(int)", 0, "same", 12.0);
        let now = item("New.cs", "N.A", "M(int)", 0, "same", 12.0);
        let c = compare(&[entry(&was)], &[now], T, &[]);
        assert_eq!(states(&c), [Some(Unchanged)]);
        assert!(c.resolved.is_empty());
    }

    #[test]
    fn a_renamed_or_moved_callable_matches_by_fingerprint() {
        let was = item("A.cs", "N.A", "Old(int)", 0, "same", 12.0);
        let renamed = item("A.cs", "N.A", "New(int)", 0, "same", 14.0);
        let c = compare(&[entry(&was)], &[renamed], T, &[]);
        assert_eq!(states(&c), [Some(Worse)]);
        let moved = item("A.cs", "N.B", "Old(int)", 0, "same", 12.0);
        let c = compare(&[entry(&was)], &[moved], T, &[]);
        assert_eq!(states(&c), [Some(Unchanged)]);
    }

    #[test]
    fn a_fingerprint_match_needs_the_same_kind() {
        let was = item("A.cs", "N.A", "Old(int)", 0, "same", 12.0);
        let mut now = item("B.cs", "N.B", "New(int)", 0, "same", 12.0);
        now.kind = "local_function_statement".into();
        let c = compare(&[entry(&was)], &[now], T, &[]);
        assert_eq!(states(&c), [Some(New)]);
        assert_eq!(c.resolved, [0]);
    }

    #[test]
    fn a_moved_file_with_an_edited_body_matches_by_identity() {
        let was = item("Old.cs", "N.A", "M(int)", 0, "before", 12.0);
        let now = item("New.cs", "N.A", "M(int)", 0, "after", 13.0);
        let c = compare(&[entry(&was)], &[now], T, &[]);
        assert_eq!(states(&c), [Some(Worse)]);
        assert!(c.resolved.is_empty());
    }

    #[test]
    fn identity_across_files_needs_the_same_kind_and_occurrence() {
        let was = item("Old.cs", "N.A", "M(int)", 0, "before", 12.0);
        let mut other_kind = item("New.cs", "N.A", "M(int)", 0, "after", 12.0);
        other_kind.kind = "local_function_statement".into();
        let other_occurrence = item("New.cs", "N.A", "M(int)", 1, "after", 12.0);
        let other_scope = item("New.cs", "N.B", "M(int)", 0, "after", 12.0);
        let c = compare(
            &[entry(&was)],
            &[other_kind, other_occurrence, other_scope],
            T,
            &[],
        );
        assert_eq!(states(&c), [Some(New), Some(New), Some(New)]);
        assert_eq!(c.resolved, [0]);
    }

    #[test]
    fn an_ambiguous_fingerprint_stays_new() {
        // Two entries and two items share a body: neither pairing is certain.
        let a = item("A.cs", "N.A", "X(int)", 0, "same", 12.0);
        let b = item("B.cs", "N.B", "Y(int)", 0, "same", 12.0);
        let now = [
            item("C.cs", "N.C", "P(int)", 0, "same", 12.0),
            item("D.cs", "N.D", "Q(int)", 0, "same", 12.0),
        ];
        let c = compare(&[entry(&a), entry(&b)], &now, T, &[]);
        assert_eq!(states(&c), [Some(New), Some(New)]);
        assert_eq!(c.resolved, [0, 1]);
        // One entry against two items is ambiguous too, as is the reverse.
        let c = compare(&[entry(&a)], &now, T, &[]);
        assert_eq!(states(&c), [Some(New), Some(New)]);
        let c = compare(&[entry(&a), entry(&b)], &now[..1], T, &[]);
        assert_eq!(states(&c), [Some(New)]);
        assert_eq!(c.resolved, [0, 1]);
    }

    #[test]
    fn an_ambiguous_identity_across_files_stays_new() {
        let a = item("A.cs", "N.A", "M(int)", 0, "fa", 12.0);
        let b = item("B.cs", "N.A", "M(int)", 0, "fb", 12.0);
        let now = item("C.cs", "N.A", "M(int)", 0, "fc", 12.0);
        let c = compare(&[entry(&a), entry(&b)], std::slice::from_ref(&now), T, &[]);
        assert_eq!(states(&c), [Some(New)]);
        assert_eq!(c.resolved, [0, 1]);
        let now = [
            item("C.cs", "N.A", "M(int)", 0, "fc", 12.0),
            item("D.cs", "N.A", "M(int)", 0, "fd", 12.0),
        ];
        let c = compare(&[entry(&a)], &now, T, &[]);
        assert_eq!(states(&c), [Some(New), Some(New)]);
    }

    #[test]
    fn exact_matches_leave_a_shared_fingerprint_unambiguous() {
        // Two callables share a body; one is still in place, the other moved.
        // The exact match removes the first pair, so the second is unique.
        let a = item("A.cs", "N.A", "X(int)", 0, "same", 12.0);
        let b = item("B.cs", "N.B", "Y(int)", 0, "same", 12.0);
        let now = [
            item("A.cs", "N.A", "X(int)", 0, "same", 12.0),
            item("Z.cs", "N.Z", "Y(int)", 0, "same", 15.0),
        ];
        let c = compare(&[entry(&a), entry(&b)], &now, T, &[]);
        assert_eq!(states(&c), [Some(Unchanged), Some(Worse)]);
    }

    #[test]
    fn baseline_moves_rewrite_entry_files_before_matching() {
        let a = item("Old.cs", "N.A", "M(int)", 0, "fa", 12.0);
        let now = item("New.cs", "N.A", "M(int)", 0, "fa", 12.0);
        // Two same-key entries make identity and fingerprint ambiguous, so only
        // the move can pair them.
        let twin = item("Twin.cs", "N.A", "M(int)", 0, "fa", 12.0);
        let moves = [("Old.cs".to_owned(), "New.cs".to_owned())];
        let c = compare(
            &[entry(&a), entry(&twin)],
            std::slice::from_ref(&now),
            T,
            &[],
        );
        assert_eq!(states(&c), [Some(New)]);
        let c = compare(
            &[entry(&a), entry(&twin)],
            std::slice::from_ref(&now),
            T,
            &moves,
        );
        assert_eq!(states(&c), [Some(Unchanged)]);
        assert_eq!(c.resolved, [1]);
        // The move matches exact file names only.
        let moves = [("Old".to_owned(), "New.cs".to_owned())];
        let c = compare(
            &[entry(&a), entry(&twin)],
            std::slice::from_ref(&now),
            T,
            &moves,
        );
        assert_eq!(states(&c), [Some(New)]);
    }

    #[test]
    fn scores_compare_at_two_decimals() {
        let was = item("A.cs", "N.A", "M()", 0, "f", 12.004);
        let at = |crap| item("A.cs", "N.A", "M()", 0, "f", crap);
        let entries = [entry(&was)];
        let class = |crap| states(&compare(&entries, &[at(crap)], T, &[]))[0];
        // 12.004 and 12.0049 both print as 12.00; 12.006 prints 12.01.
        assert_eq!(class(12.0049), Some(Unchanged));
        assert_eq!(class(11.996), Some(Unchanged));
        assert_eq!(class(12.006), Some(Worse));
        assert_eq!(class(11.99), Some(Improved));
        assert_eq!(class(30.0), Some(Worse));
        assert_eq!(class(10.5), Some(Improved));
        let c = compare(&entries, &[at(11.5)], T, &[]);
        assert_eq!(c.items[0].unwrap().was, Some(12.004));
    }

    #[test]
    fn passing_items_are_unclassified_and_unmatched_entries_are_resolved() {
        let fixed = item("A.cs", "N.A", "Fixed()", 0, "f1", 14.0);
        let gone = item("A.cs", "N.A", "Gone()", 0, "f2", 14.0);
        let stays = item("A.cs", "N.A", "Stays()", 0, "f3", 14.0);
        let now = [
            // Now under the threshold, equality passing.
            item("A.cs", "N.A", "Fixed()", 0, "g1", 10.0),
            item("A.cs", "N.A", "Stays()", 0, "f3", 14.0),
            item("A.cs", "N.A", "Fresh()", 0, "g4", 40.0),
        ];
        let entries = [entry(&fixed), entry(&gone), entry(&stays)];
        let c = compare(&entries, &now, T, &[]);
        assert_eq!(states(&c), [None, Some(Unchanged), Some(New)]);
        assert_eq!(c.items[2].unwrap().was, None);
        assert_eq!(c.resolved, [0, 1]);
        assert_eq!(
            c.summary(),
            BaselineSummary {
                new: 1,
                worse: 0,
                unchanged: 1,
                improved: 0,
                resolved: 2
            }
        );
    }

    #[test]
    fn the_summary_counts_every_outcome() {
        let c = Comparison {
            items: vec![
                Some(Classified {
                    state: Worse,
                    was: Some(1.0),
                }),
                Some(Classified {
                    state: Worse,
                    was: Some(1.0),
                }),
                Some(Classified {
                    state: Improved,
                    was: Some(1.0),
                }),
                Some(Classified {
                    state: New,
                    was: None,
                }),
                Some(Classified {
                    state: Unchanged,
                    was: Some(1.0),
                }),
                None,
            ],
            resolved: vec![0],
        };
        assert_eq!(
            c.summary(),
            BaselineSummary {
                new: 1,
                worse: 2,
                unchanged: 1,
                improved: 1,
                resolved: 1
            }
        );
    }

    #[test]
    fn writing_keeps_only_failing_callables_in_file_line_signature_order() {
        let mk = |file: &str, line: usize, sig: &str, crap: f64| {
            let mut i = item(file, "N.A", sig, 0, "f", crap);
            i.line = line;
            i
        };
        let items = [
            mk("b.cs", 1, "A()", 30.0),
            mk("a.cs", 9, "B()", 30.0),
            mk("a.cs", 3, "Z()", 30.0),
            mk("a.cs", 3, "Y()", 30.0),
            mk("a.cs", 5, "Pass()", 10.0),
        ];
        let baseline = Baseline::from_items(&items, 10.0);
        assert_eq!((baseline.version, baseline.threshold), (1, 10.0));
        let order: Vec<(&str, usize, &str)> = baseline
            .entries
            .iter()
            .map(|e| (e.file.as_str(), e.line, e.signature.as_str()))
            .collect();
        assert_eq!(
            order,
            [
                ("a.cs", 3, "Y()"),
                ("a.cs", 3, "Z()"),
                ("a.cs", 9, "B()"),
                ("b.cs", 1, "A()")
            ]
        );
        let e = &baseline.entries[0];
        assert_eq!(
            (
                e.kind.as_str(),
                e.scope.as_str(),
                e.occurrence,
                e.complexity
            ),
            ("method_declaration", "N.A", 0, 5)
        );
        assert_eq!(
            (e.fingerprint.as_str(), e.coverage, e.crap),
            ("f", None, 30.0)
        );
    }

    #[test]
    fn a_baseline_round_trips_through_its_file_text() {
        let items = [item("a.cs", "N.A", "M(int)", 2, "abc", 12.5)];
        let baseline = Baseline::from_items(&items, 10.0);
        let text = baseline.render().unwrap();
        assert!(text.ends_with("}\n"));
        assert!(text.contains("\"threshold\": 10.0"), "{text}");
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("b.json");
        std::fs::write(&path, &text).unwrap();
        assert_eq!(read(&path).unwrap(), baseline);
    }

    fn rejected(text: &str) -> String {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("b.json");
        std::fs::write(&path, text).unwrap();
        let error = read(&path).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::InvalidInput);
        let message = error.to_string();
        assert!(message.contains(&path.display().to_string()), "{message}");
        message
    }

    #[test]
    fn reading_rejects_bad_files_naming_them() {
        assert!(rejected("{ nope").contains("invalid baseline"));
        assert!(
            rejected(r#"{"version":1,"threshold":10,"entries":[],"x":1}"#)
                .contains("unknown field")
        );
        assert!(rejected(r#"{"version":2,"threshold":10,"entries":[]}"#)
            .contains("unsupported baseline version 2; this dotnet-crap reads version 1"));
        assert!(rejected(r#"{"threshold":10,"entries":[]}"#).contains("missing field `version`"));
        let entry = r#"{"file":"a.cs","kind":"k","scope":"","signature":"M()","occurrence":0,"fingerprint":"f","line":1,"complexity":1,"coverage":null,"crap":1.0,"extra":0}"#;
        assert!(rejected(&format!(
            r#"{{"version":1,"threshold":10,"entries":[{entry}]}}"#
        ))
        .contains("unknown field `extra`"));
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("absent.json");
        let error = read(&missing).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::InvalidInput);
        assert!(error.to_string().contains("absent.json"));
    }

    #[test]
    fn the_threshold_must_match() {
        let baseline = Baseline::from_items(&[], 10.0);
        check_threshold(&baseline, 10.0).unwrap();
        let error = check_threshold(&baseline, 5.5).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::InvalidInput);
        assert_eq!(
            error.to_string(),
            "baseline threshold 10 does not match --threshold 5.5"
        );
    }

    #[test]
    fn moves_parse_old_equals_new() {
        assert_eq!(
            parse_move("src/A.cs=lib/A.cs").unwrap(),
            ("src/A.cs".to_owned(), "lib/A.cs".to_owned())
        );
        // Only the first `=` separates; the rest belongs to the new path.
        assert_eq!(
            parse_move("a=b=c").unwrap(),
            ("a".to_owned(), "b=c".to_owned())
        );
        for bad in ["", "a", "=b", "a="] {
            let error = parse_move(bad).unwrap_err();
            assert_eq!(error.kind(), ErrorKind::InvalidInput, "{bad:?}");
            assert!(error.to_string().contains("expected OLD=NEW"), "{bad:?}");
        }
    }
}
