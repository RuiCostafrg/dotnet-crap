//! Callable recognition for operators, conversions, destructors and indexers.
//! Expected complexities are counted by hand with the documented convention:
//! start at 1; add 1 for each if, loop, catch, `?:`, non-default case label,
//! non-discard switch arm, `when`, `&&`, `||`, `??` and `??=`. Each nested
//! callable is scored only as its own record.

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{fs, path::Path, process::Command};

const CALLABLES_CS: &str = r##"using System;

public readonly struct Money
{
    public Money(decimal amount)
    {
        Amount = amount;
    }

    public decimal Amount { get; }

    public static Money operator +(Money a, Money b) => new Money(a.Amount + b.Amount);

    public static Money operator -(Money a, Money b)
    {
        if (a.Amount > 0 && b.Amount > 0)
        {
            return new Money(a.Amount - b.Amount);
        }

        return new Money(0);
    }

    public static bool operator !(Money m)
    {
        return m.Amount == 0;
    }

    public static bool operator true(Money m) => m.Amount != 0;

    public static bool operator false(Money m) => m.Amount == 0;

    public static Money operator *(Money a, int k) => new Money(a.Amount * k);

    public static Money operator checked *(Money a, int k)
    {
        return new Money(a.Amount * k);
    }

    public static implicit operator decimal(Money m) => m.Amount;

    public static explicit operator int(Money m)
    {
        return m.Amount > 0 ? (int)m.Amount : 0;
    }

    public static implicit operator Money(int value)
    {
        int Clamp(int x) => x < 0 ? 0 : x;
        return new Money(Clamp(value));
    }

    public static Money operator /(Money a, Money b)
    {
        if (b.Amount == 0)
        {
            return a;
        }

        Func<decimal, decimal> guard = x =>
            x > 0 && x < 10 ? x : 0;
        return new Money(guard(a.Amount) / b.Amount);
    }
}

public sealed class Table
{
    private readonly string[] rows = { "a", "b" };
    private readonly int[] values = new int[4];

    public string this[int i] => rows[i] ?? "";

    public int this[string key] => Score(key, s => s.Length > 3 ? 1 : 0);

    public int this[long index]
    {
        get
        {
            if (index < 0)
            {
                return 0;
            }

            return values[(int)index];
        }

        set => values[(int)index] = value;
    }

    private static int Score(string key, Func<string, int> f) => f(key);
}

public sealed class Resource
{
    private bool disposed;

    ~Resource()
    {
        if (!disposed)
        {
            disposed = true;
        }
    }
}

public sealed class Tracker
{
    public static int Count;

    ~Tracker() => Count--;
}
"##;

const NESTED_CS: &str = r##"using System;

public sealed class Nested
{
    private readonly int[] items = new int[2];

    public int this[int index] => Apply(index, value =>
    {
        int Local(int y) => y > 0 ? y : 0;
        return value > 0 && Local(value) > 1 ? 1 : 0;
    });

    public static Nested operator +(Nested left, Nested right)
    {
        if (left == null)
        {
            return right;
        }

        Func<int, int> choose = x => x > 0 || x < -5 ? x : 0;
        return choose(1) > 0 ? left : right;
    }

    ~Nested()
    {
        Func<bool> keep = () => items.Length > 0;
        if (!keep())
        {
            return;
        }
    }

    private static int Apply(int index, Func<int, int> f) => f(index);
}
"##;

const VARIANTS_CS: &str = r##"public readonly struct Pair
{
    public Pair(int value)
    {
        Value = value;
    }

    public int Value { get; }

    public string Label => Value > 0 ? "pos" : "neg";

    public static bool operator <(Pair a, Pair b) => a.Value < b.Value && b.Value > 0;

    public static bool operator >(Pair a, Pair b)
    {
        return a.Value > b.Value && b.Value > 0;
    }

    public static implicit operator int(Pair p) => p.Value;

    public static implicit operator Pair(int v)
    {
        return v > 0 ? new Pair(v) : new Pair(0);
    }
}

public sealed class Labels
{
    private readonly string[] names = { "a", "b" };

    public string this[int i] => names[i] ?? "none";

    public string this[string key]
    {
        get { return key ?? "none"; }
    }
}
"##;

const DECLARATIONS_CS: &str = r##"#pragma warning disable CS0626 // intentional extern declarations without implementations
public interface IAdd<T> where T : IAdd<T>
{
    static abstract T operator +(T left, T right);
    static abstract implicit operator T(int value);
    int this[int index] { get; }
    int Count { get; }
}

public abstract class Shape
{
    public abstract int this[long index] { get; set; }
    public abstract int Area { get; }
    public static extern Shape operator -(Shape value);
    extern ~Shape();
}
"##;

const EXISTING_CS: &str = r##"public sealed class Existing
{
    private int count;

    public Existing()
    {
    }

    public int Auto { get; set; }

    public int Arrow => true ? 1 : 0;

    public int Explicit
    {
        get { return count > 0 ? count : 0; }
        set { count = value; }
    }

    public int Method(int x) => x > 0 ? 1 : 0;
}
"##;

// Line numbers matter here: coverage ownership is line based.
const GRID_CS: &str = r##"using System;
public sealed class Grid
{
    public int this[int i] =>
        Score(i,
            x => x > 0 ? 1 : 0);
    private static int Score(int i, Func<int, int> f) => f(i);
    public static Grid operator +(Grid a, Grid b)
    {
        if (a == null)
        {
            return b;
        }
        return a;
    }
}
"##;

/// Invoke the built CLI with an isolated source root and capture its public output contract.
fn run(root: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_dotnet-crap"))
        .arg("--root")
        .arg(root)
        .args(args)
        .output()
        .unwrap()
}

/// Materialize C# fixtures in a temporary source directory owned by the caller.
fn write_files(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (name, text) in files {
        fs::write(dir.path().join(name), text).unwrap();
    }
    dir
}

/// Require a successful JSON scan before exposing the parsed report to assertions.
fn json_report(root: &Path) -> Value {
    let output = run(root, &["--format", "json", "--threshold", "1000"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

/// Project report records into comparable callable kind, name and complexity tuples.
fn callables(report: &Value) -> Vec<(String, String, u64)> {
    report["functions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            (
                i["kind"].as_str().unwrap().to_owned(),
                i["name"].as_str().unwrap().to_owned(),
                i["complexity"].as_u64().unwrap(),
            )
        })
        .collect()
}

/// Own the hand-calculated expected callable tuples for comparison with CLI output.
fn expected(rows: &[(&str, &str, u64)]) -> Vec<(String, String, u64)> {
    rows.iter()
        .map(|(kind, name, complexity)| ((*kind).to_owned(), (*name).to_owned(), *complexity))
        .collect()
}

#[test]
/// Check the complete callable inventory and independently counted decisions for new C# forms.
fn operator_conversion_destructor_and_indexer_complexity() {
    let dir = write_files(&[("Callables.cs", CALLABLES_CS)]);
    let report = json_report(dir.path());
    // Source order is the report order; a missing or duplicated callable fails here.
    assert_eq!(
        callables(&report),
        expected(&[
            ("constructor_declaration", "Money", 1),
            ("operator_declaration", "operator +(Money,Money)", 1),
            ("operator_declaration", "operator -(Money,Money)", 3),
            ("operator_declaration", "operator !(Money)", 1),
            ("operator_declaration", "operator true(Money)", 1),
            ("operator_declaration", "operator false(Money)", 1),
            ("operator_declaration", "operator *(Money,int)", 1),
            ("operator_declaration", "operator checked *(Money,int)", 1),
            (
                "conversion_operator_declaration",
                "implicit operator decimal(Money)",
                1
            ),
            (
                "conversion_operator_declaration",
                "explicit operator int(Money)",
                2
            ),
            (
                "conversion_operator_declaration",
                "implicit operator Money(int)",
                1
            ),
            ("local_function_statement", "Clamp", 2),
            ("operator_declaration", "operator /(Money,Money)", 2),
            ("lambda_expression", "lambda_expression", 3),
            ("indexer_declaration", "this[int]", 2),
            ("indexer_declaration", "this[string]", 1),
            ("lambda_expression", "lambda_expression", 2),
            ("accessor_declaration", "get", 2),
            ("accessor_declaration", "set", 1),
            ("method_declaration", "Score", 1),
            ("destructor_declaration", "~Resource", 2),
            ("destructor_declaration", "~Tracker", 1),
        ])
    );
}

#[test]
/// Verify enclosing operators and conversions do not absorb nested callable decisions.
fn nested_callables_are_charged_only_to_their_owner() {
    let dir = write_files(&[("Nested.cs", NESTED_CS)]);
    let report = json_report(dir.path());
    // The indexer's `?:` and `&&` live in the lambda and the local function, so
    // the indexer keeps complexity 1; the lambda's own `?:` and `&&` yield 3, and
    // the local function is not added to the lambda.
    assert_eq!(
        callables(&report),
        expected(&[
            ("indexer_declaration", "this[int]", 1),
            ("lambda_expression", "lambda_expression", 3),
            ("local_function_statement", "Local", 2),
            ("operator_declaration", "operator +(Nested,Nested)", 3),
            ("lambda_expression", "lambda_expression", 3),
            ("destructor_declaration", "~Nested", 2),
            ("lambda_expression", "lambda_expression", 1),
            ("method_declaration", "Apply", 1),
        ])
    );
}

#[test]
/// Compare equivalent expression-bodied and block-bodied callable scores.
fn expression_and_block_bodies_score_the_same_logic() {
    let dir = write_files(&[("Variants.cs", VARIANTS_CS)]);
    let report = json_report(dir.path());
    let items = callables(&report);
    assert_eq!(
        items,
        expected(&[
            ("constructor_declaration", "Pair", 1),
            ("property_declaration", "Label", 2),
            ("operator_declaration", "operator <(Pair,Pair)", 2),
            ("operator_declaration", "operator >(Pair,Pair)", 2),
            (
                "conversion_operator_declaration",
                "implicit operator int(Pair)",
                1
            ),
            (
                "conversion_operator_declaration",
                "implicit operator Pair(int)",
                2
            ),
            ("indexer_declaration", "this[int]", 2),
            ("accessor_declaration", "get", 2),
        ])
    );
    // Expression form of `<` and block form of `>` contain the same `&&`.
    let complexity_of = |name: &str| items.iter().find(|i| i.1 == name).unwrap().2;
    assert_eq!(
        complexity_of("operator <(Pair,Pair)"),
        complexity_of("operator >(Pair,Pair)")
    );
}

#[test]
/// Exclude declarations and auto-accessors that have no implemented callable body.
fn declaration_only_and_auto_forms_are_omitted() {
    let dir = write_files(&[("Declarations.cs", DECLARATIONS_CS)]);
    let report = json_report(dir.path());
    assert!(report["functions"].as_array().unwrap().is_empty());
    assert_eq!(report["files_scanned"], 1);
    assert_eq!(run(dir.path(), &["--fail-on-empty"]).status.code(), Some(1));
}

#[test]
/// Preserve the existing method, constructor, property and accessor report contract.
fn existing_forms_keep_their_names_and_complexity() {
    let dir = write_files(&[("Existing.cs", EXISTING_CS)]);
    let report = json_report(dir.path());
    assert_eq!(
        callables(&report),
        expected(&[
            ("constructor_declaration", "Existing", 1),
            ("property_declaration", "Arrow", 2),
            ("accessor_declaration", "get", 2),
            ("accessor_declaration", "set", 1),
            ("method_declaration", "Method", 2),
        ])
    );
}

#[test]
/// Check the fixture's declaration whitespace variants retain their expected display names.
fn source_spacing_does_not_change_names() {
    let dir = write_files(&[(
        "Spacing.cs",
        "class N {\n  public static N operator   +  (N  a ,  N b) => a;\n  public int this [ int   i ] => i;\n}\n",
    )]);
    let report = json_report(dir.path());
    assert_eq!(
        callables(&report),
        expected(&[
            ("operator_declaration", "operator +(N,N)", 1),
            ("indexer_declaration", "this[int]", 1),
        ])
    );
}

#[test]
/// Require readable operator, conversion, destructor and indexer names in text reports.
fn text_output_names_callables_descriptively() {
    let dir = write_files(&[("Callables.cs", CALLABLES_CS)]);
    let output = run(dir.path(), &["--threshold", "1000"]);
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("Callables.cs:"), "{text}");
    assert!(
        text.contains(" operator checked *(Money,int) complexity=1 "),
        "{text}"
    );
    assert!(text.contains(" ~Tracker complexity=1 "), "{text}");
    assert!(text.contains(" this[string] complexity=1 "), "{text}");
}

#[test]
/// Keep nested callable coverage points out of their enclosing callable's denominator.
fn coverage_is_owned_by_the_innermost_callable() {
    let dir = write_files(&[("Grid.cs", GRID_CS)]);
    let hash = hex::encode(Sha256::digest(GRID_CS.as_bytes()));
    // Lines 4-5 belong to the indexer, line 6 to the lambda, line 7 to Score and
    // lines 8-15 to the operator. Line 2 belongs to no callable and must not count.
    let coverage = dir.path().join("coverage.json");
    fs::write(
        &coverage,
        format!(
            r#"{{"version":1,"files":{{"Grid.cs":{{"sha256":"{hash}","lines":{{"2":0,"4":1,"5":0,"6":1,"7":0,"8":1,"9":1,"10":1,"11":1,"12":0,"13":0,"14":1,"15":1}}}}}}}}"#
        ),
    )
    .unwrap();
    let output = run(
        dir.path(),
        &[
            "--coverage",
            coverage.to_str().unwrap(),
            "--format",
            "json",
            "--threshold",
            "1000",
            "--require-coverage",
        ],
    );
    assert!(output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["coverage_complete"], true);
    let items = report["functions"].as_array().unwrap();
    let find = |kind: &str, name: &str| {
        items
            .iter()
            .find(|i| i["kind"] == kind && i["name"] == name)
            .unwrap_or_else(|| panic!("missing {kind} {name}"))
    };
    // The lambda's hit on line 6 must not raise the indexer's 1/2 coverage.
    let indexer = find("indexer_declaration", "this[int]");
    assert_eq!(indexer["complexity"], 1);
    assert_eq!(indexer["coverage"], 0.5);
    assert!((indexer["crap"].as_f64().unwrap() - 1.125).abs() < 1e-9);

    let lambda = find("lambda_expression", "lambda_expression");
    assert_eq!(lambda["complexity"], 2);
    assert_eq!(lambda["coverage"], 1.0);
    assert!((lambda["crap"].as_f64().unwrap() - 2.0).abs() < 1e-9);

    let score = find("method_declaration", "Score");
    assert_eq!(score["coverage"], 0.0);

    // Operator lines 8-15 are eight owned lines, six of which are positive.
    let operator = find("operator_declaration", "operator +(Grid,Grid)");
    assert_eq!(operator["complexity"], 2);
    assert_eq!(operator["coverage"], 0.75);
    assert!((operator["crap"].as_f64().unwrap() - 2.0625).abs() < 1e-9);
}

#[test]
/// Reject malformed operator syntax with exit 2 and no partial report.
fn malformed_operator_is_an_input_error_without_report() {
    let dir = write_files(&[(
        "Bad.cs",
        "class Bad { public static int operator +(int a, int b) }\n",
    )]);
    let output = run(dir.path(), &[]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
}

#[test]
/// Compile declaration fixtures with the real .NET SDK when live E2E validation is enabled.
fn declaration_fixtures_are_valid_csharp() {
    if std::env::var_os("DOTNET_CRAP_E2E").is_none() {
        return;
    }
    let dir = write_files(&[
        ("Callables.cs", CALLABLES_CS),
        ("Nested.cs", NESTED_CS),
        ("Variants.cs", VARIANTS_CS),
        ("Declarations.cs", DECLARATIONS_CS),
        ("Existing.cs", EXISTING_CS),
        ("Grid.cs", GRID_CS),
        ("Signatures.cs", SIGNATURES_CS),
    ]);
    fs::write(
        dir.path().join("Fixtures.csproj"),
        "<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><OutputType>Library</OutputType><TargetFramework>net10.0</TargetFramework></PropertyGroup></Project>",
    )
    .unwrap();
    let status = Command::new("dotnet")
        .arg("build")
        .arg("--nologo")
        .arg(dir.path())
        .status()
        .unwrap();
    assert!(status.success(), "fixtures must compile with the .NET SDK");
    // The same compiled sources must score identically under the Rust scanner.
    let report = json_report(dir.path());
    assert_eq!(report["files_scanned"], 7);
}

const SIGNATURES_CS: &str = r#"public readonly struct Token
{
    private readonly int value;
    public Token(int value) => this.value = value;
    public static explicit operator int(Token x) => x.value;
    public static explicit operator checked int(Token x) => checked(x.value);
}
public sealed class Many
{
    public int this[int index, params int[] rest] => rest.Length > 0 ? rest[0] : index;
}
"#;

#[test]
/// Distinguish checked conversions and params indexers by their syntax-derived signatures.
fn signature_names_distinguish_checked_conversions_and_params_indexers() {
    let dir = write_files(&[("Signatures.cs", SIGNATURES_CS)]);
    assert_eq!(
        callables(&json_report(dir.path())),
        expected(&[
            ("constructor_declaration", "Token", 1),
            (
                "conversion_operator_declaration",
                "explicit operator int(Token)",
                1
            ),
            (
                "conversion_operator_declaration",
                "explicit operator checked int(Token)",
                1
            ),
            ("indexer_declaration", "this[int,int[]]", 2),
        ])
    );
}
