# C# callable recognition

This document describes the callable forms scored by `dotnet-crap` and their
limits. It covers syntax as written by the Tree-sitter C# grammar (`tree-sitter-c-sharp`
0.23.x). It is not Roslyn semantics: overloads, generic constraints, and
conversions are not resolved, and no compilation or conditional-compilation
symbols are applied.

Complexity follows the convention in the [initial release contract](FEATURE_PARITY.md#initial-release-contract):
start at 1; add one for `if`, loops, `catch`, `?:`, non-default case labels,
non-discard switch-expression arms, `when`, `&&`, `||`, `??`, and `??=`.

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
  symbols a build would define.
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
