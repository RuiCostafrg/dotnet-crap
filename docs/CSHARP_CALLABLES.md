# C# callable recognition

This document describes the callable forms scored by `dotnet-crap` and their
limits. It covers syntax as written by the Tree-sitter C# grammar (`tree-sitter-c-sharp`
0.24.x). It is not Roslyn semantics: overloads, generic constraints, and
conversions are not resolved. Conditional-compilation symbols are applied only
to files that do not parse as written ([conditional compilation](PREPROCESSOR.md)).

Complexity starts at 1 and adds one for each `if`, loop, `catch`, ternary,
non-default case label, non-discard switch-expression arm, `when` guard,
`&&`, `||`, `??`, `??=`, and logical-pattern `and` or `or`. See
[logical patterns](#logical-patterns) for examples and the switch conventions.

## Supported callables

Each callable below is one report record with its own complexity and coverage
span. A callable without an implementation body is omitted.

| Source form | Report `kind` | Report name | Bodies |
| --- | --- | --- | --- |
| `static T operator +(A a, B b) { ... }` and `=> expr;` | `operator_declaration` | `operator +(A,B)` | block, expression |
| `operator checked *(A a, int k)` | `operator_declaration` | `operator checked *(A,int)` | block, expression |
| `operator !(A a)`, `operator true(A a)`, `operator false(A a)` | `operator_declaration` | `operator !(A)`, `operator true(A)`, `operator false(A)` | block, expression |
| `implicit operator T(S s) { ... }` / `explicit operator T(S s) => ...;` | `conversion_operator_declaration` | `implicit operator T(S)` / `explicit operator T(S)` | block, expression |
| `explicit operator checked T(S s)` | `conversion_operator_declaration` | `explicit operator checked T(S)` | block, expression |
| `~ClassName() { ... }` / `~ClassName() => expr;` | `destructor_declaration` | `~ClassName` | block, expression |
| `T this[P p] => expr;` | `indexer_declaration` | `this[P]` | expression only |

Existing forms are unchanged: methods, constructors, local functions, explicit
property and indexer accessors (`get`, `set`, `init`, `add`, `remove`),
expression-bodied properties, lambdas, and anonymous methods.

### Name rules

Names are built from the syntax tree. Runs of whitespace inside each part are
collapsed to one space, and parameter names and modifiers are not recorded. Other
spacing is kept as written: `Dictionary <string, int>` and `Dictionary<string,
int>` produce different names. Each name is built as follows:

- Parameter types are the `type` node text with runs of whitespace collapsed to
  one space. They are joined with `,` and have no parameter names or modifier
  keywords such as `ref`, `out`, `in`, or `params`. A `params` array keeps its
  array suffix, so `params int[] rest` becomes `int[]`.
- An operator name is `operator`, then `checked` when the operator is a checked
  operator, then the operator token, then `(` parameter types `)`.
- A conversion name is `implicit` or `explicit`, `operator`, `checked` when
  present, the target type as written, then `(` source parameter types `)`.
- A destructor name is `~` followed by the class name as written.
- An indexer name is `this[` parameter types `]`.

Tuple types keep their element names as written, so `(int a, int b)` remains
`(int a, int b)`.

Unary and binary operators with the same symbol, such as unary and binary `-`,
are distinguished by their parameter types. Same-named callables can still occur
in different types, and in both branches of a preprocessor conditional, because
the scanner reads source as written. The `line` field distinguishes them.

## Ownership and nested callables

Every callable is one record. A nested callable is not charged to its owner:

- A lambda, anonymous method, or local function inside an operator, conversion,
  destructor, or expression-bodied indexer is scored as its own record. Its
  decisions do not add to the enclosing callable.
- The enclosing callable's coverage ignores the lines in each nested callable's
  span, so a lambda's hit on a line does not count toward the indexer.
- Operators, conversions, destructors, and indexers cannot be nested inside
  another callable in valid C#, so ownership questions arise only from lambdas,
  anonymous methods, and local functions.

### Explicit indexer accessors

An indexer with an accessor list (`{ get { ... } set { ... } }`) is not itself a
callable. Each accessor body is a separate `accessor_declaration` record named
`get`, `set`, `init`, `add`, or `remove`, as with property accessors. Accessor
names are not qualified by the indexer signature, so two indexers with a `get`
accessor are identified by `line`. Changing this would change existing accessor
output, so it is deliberately left as is.

## Omitted declarations

These forms have no implementation and produce no record:

- `abstract`, `extern`, or interface operators, conversions, destructors, and
  indexers that end in `;` instead of a body, including `static abstract`
  operators in generic interfaces.
- Auto-implemented accessors (`get;`, `set;`, `init;`) and accessor lists with
  only such accessors, such as `int this[long i] { get; set; }`.
- Abstract property and indexer declarations.

## Explicit limits

- No overload resolution, type-binding, or semantic compilation. Names reflect
  the source text, and conversions are not matched to their targets.
- No top-level statements, primary constructors, or preprocessor policy. These
  are the subject of the later modern-syntax milestone.
- `checked` operators are scored separately from their unchecked counterparts.
  A checked operator requires an unchecked pair in valid C#, and both appear in
  the report.
- An indexer has no block body of its own. Block bodies appear only on explicit
  accessors, which keep their `get`/`set` names.
- Both branches of a `#if`/`#else` are scored as written, regardless of the
  symbols a build would define. Only a file that does not parse as written has
  its `#if` groups resolved with `--define` symbols, and then only the active
  branch is scored; see [conditional compilation](PREPROCESSOR.md).
- An `#if A && B` inside a method body adds no decision point for its `&&` on
  either path: the compiler resolves directive conditions, so only code inside
  the branches is scored.
- Coverage ownership is line based. A line containing both a parent's code and
  the start of a nested callable is excluded from the parent. This is the same
  same-line ambiguity documented for all callables and is not resolved here.
- Complexity is a syntax convention for these forms. It does not claim equality
  with a Roslyn or IDE metric.

## Verification

`tests/csharp_callables.rs` checks each supported form with hand-counted
complexity, nested-lambda and local-function ownership, coverage ownership on a
hand-numbered source, omitted declaration-only forms, expression and block
variants, source spacing normalization, text output, and malformed operators.
With `DOTNET_CRAP_E2E=1`, the fixtures are built with the .NET SDK so that the
scanner's expectations describe valid C#.

## Logical patterns

The scanner adds one decision for each explicit C# `and` or `or` pattern
combinator. The same convention applies to `&&` and `||`.

Each callable starts at one. Parentheses and relational/type/constant tests
add no separate decision, and `not` adds none, just like ordinary comparison
operators and Boolean negation. Nested combinators each count once, including
those inside property and list patterns. Pattern words inside strings and
comments do not contribute. Preprocessor directive conditions (`#if`/`#elif`)
add no decision for their `&&` or `||`, because the compiler resolves them.

Each non-default statement case contributes
one, each non-discard switch-expression arm contributes one, and each `when`
guard contributes one. Logical patterns and ordinary Boolean expressions in
those constructs add their own contributions. Only a bare discard counts as a discard arm.

### Physical examples and independent expectations

The dependency-free [modern-patterns fixture](../tests/fixtures/modern-patterns/) compiles and
executes these forms. `Patterns.cs` has the following hand-derived values:

| Callable | Complexity | Decisions beyond the base |
| --- | --- | --- |
| `Logical` | 3 | one `and`, one `or` |
| `Negated` | 2 | one `and`; surrounding `not` adds nothing |
| `Simple` | 1 | simple `not 0` adds nothing |
| `Property` | 3 | one `and`, one `or` in separate property subpatterns |
| `List` | 3 | one `and` in an element, one outer `or` |
| `Expression` | 5 | two non-discard arms, one `and`, one `or` |
| `Statement` | 6 | two case labels, one guard, one `and`, one `or` |
| `Guard` | 5 | one arm, one guard, one `&&`, one `and` |
| `Owned` | 3 | one arm and one ternary; nested callables excluded |
| `Owned`'s local `Local` | 3 | one arm and one `and` |
| `Owned`'s lambda | 2 | one `or` |
| `Text` | 1 | pattern words/operators are string contents |

The ownership regression supplies native line points on separate lines. The
parent has one hit and one miss, the local function has one miss, and the lambda
has one hit. Their coverage/CRAP pairs must be `0.5 / 4.125`, `0 / 12`, and
`1 / 2`. Nested callable decisions and line evidence remain excluded from the
parent. Same-line nesting is ambiguous.

`PatternBranch.cs` contains an `if` with one `and` and one `or`, so its single
method has complexity 4. The fixture records three points: the `if` line
and both returns. Partial execution hits the condition and one return, for
coverage `2/3` and CRAP `4² × (1/3)³ + 4 ≈ 4.59`. Full execution hits all
three points, for coverage `1` and CRAP `4`.
Tests require passing at each exact threshold and failing just below it.
These are explicit line probes, not Coverlet sequence-point completeness claims.

### Live verification

```sh
DOTNET_CRAP_E2E=1 cargo test --locked --test modern_patterns
```

The live test compiles and executes the physical project, checks representative
matching and non-matching inputs, imports the measured line hits with an exact
source hash, and checks source bytes again afterwards. The fixture defaults to
`net10.0`; existing CI selects `net10.0` or `net11.0` through
`DOTNET_CRAP_TEST_TFM` and asserts the actual runtime major.

## Scoring accuracy fixtures

Three tracked C# fixtures exercise different complexity and ownership rules. The
Rust tests use explicit known hit maps to check the arithmetic independently:

| Fixture | Complexity derivation | Partial coverage | Expected CRAP |
| --- | --- | --- | --- |
| Switch.cs / Pick | base 1 + two non-default case labels | 1/2 | 3 + 9 × (1/2)³ = 4.125 |
| Branches.cs / Run | base 1 + while + && + ?? + catch | 1/2 | 5 + 25 × (1/2)³ = 8.125 |
| Nested.cs / Outer | base 1 + if; local function is separate | 1/2 | 2 + 4 × (1/2)³ = 2.5 |

The nested Inner callable has complexity 1 and full evidence, yielding CRAP 1.
Its executed line is excluded from Outer. Tests assert exact complexity,
coverage, CRAP, passing threshold equality, and failure just below the threshold.

With `DOTNET_CRAP_E2E=1`, the SDK compiles and executes all three real fixtures.
Explicit C# instrumentation starts with zero counters for every instrumented
point and records hits at runtime. The first execution leaves one owned point
uncovered per outer callable; a second execution covers the remaining paths.
The Rust test converts these actual counters into the native coverage format,
binds exact source hashes captured before execution, verifies unchanged bytes,
and checks both partial and full reports plus strict gate boundaries. Fully
covered Pick, Run, Outer and Inner score 3, 5, 2 and 1 respectively.

This is a controlled instrumentation fixture, not a Coverlet collector or proof
that all executable statements are covered. The denominator is the supplied
points plus any derived executable line without one, per the
[native coverage contract](../examples/COVERAGE.md).

Source hashes bind exact original bytes, including BOMs. UTF-8 and BOM-marked
UTF-16 LE/BE are decoded for parsing; line numbers refer to decoded lines and
columns are one-based UTF-8 byte offsets in decoded text. Malformed encoding is
an input error. Selected non-UTF-8 paths are rejected rather than merged through
lossy replacement characters. Backslashes are separators only on Windows.

Collection and scoring use iterative traversal, with a one-million-node budget
per file traversal and per callable. Complexity overflow returns an error.
Independent unit tests lower each internal budget and verify rejection beyond
the limit, including exact-limit acceptance. The CLI supplies one million for
both budgets. Decode failures identify the source file, and terminal controls
in that error context are escaped before printing.
Coverage counts use prefix indexes over ordered line points. Line checks use
one cached count per file. A final source hash check
detects changes before publication, but is not a filesystem transaction.

Nested-callable exclusions are stored as merged line intervals, not expanded
sets of every excluded line, so long nested spans do not multiply work by their
line lengths. Coverage lookups use those intervals and the ordered point map.
When coverage input is supplied, selected relative paths containing literal
backslashes or colons are rejected explicitly because version-1 native coverage
cannot represent them. Without coverage input, these source names are preserved.
Backslashes in the source root itself remain valid Unix filename characters.
