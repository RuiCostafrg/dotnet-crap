# Logical C# pattern scoring

This accuracy extension is introduced in PR #41. Published `v0.1.0` predates
it; the new scores apply to the updated scanner source and its future releases.

The scanner adds one decision for each explicit C# `and` or `or` pattern
combinator. This extends its existing convention for `&&` and `||`; it is not
a claim of equivalence with Roslyn, IDE metrics or a compiler's lowered code.

Each callable starts at one. Parentheses and relational/type/constant tests
add no separate decision, and `not` adds none, just like ordinary comparison
operators and Boolean negation. Nested combinators each count once, including
those inside property and list patterns. Pattern words inside strings and
comments do not contribute.

Switch rules remain unchanged: each non-default statement case contributes
one, each non-discard switch-expression arm contributes one, and each `when`
guard contributes one. Logical patterns and ordinary Boolean expressions in
those constructs add their own contributions. This change does not normalize
`var` or parenthesized fallback arms into bare discards.

## Physical examples and independent expectations

The dependency-free project in `tests/fixtures/modern-patterns` compiles and
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
parent exactly as before; same-line nesting remains ambiguous.

`PatternBranch.cs` contains an `if` with one `and` and one `or`, so its single
method has complexity 4. Explicit runtime probes on its two return lines retain
zero-hit entries. Partial execution hits one return: coverage `1/2` and CRAP
`4² × (1/2)³ + 4 = 6`. Full execution hits both: coverage `1` and CRAP `4`.
Tests require passing at each exact threshold and failing just below it.
These are explicit line probes, not Coverlet sequence-point completeness claims.

## Validation and boundaries

```sh
DOTNET_CRAP_E2E=1 cargo test --locked --test modern_patterns
```

The live test compiles and executes the physical project, checks representative
matching and non-matching inputs, imports the measured line hits with an exact
source hash, and checks source bytes again afterwards. The fixture defaults to
`net10.0`; existing CI selects `net10.0` or `net11.0` through
`DOTNET_CRAP_TEST_TFM` and asserts the actual runtime major.

At implementation time the ordinary regressions reproduce the previous
under-counting before the scorer change. SDK, platform and mutation results
must be recorded at the final reviewed revision; unsupported cases are not
certified by these examples. Top-level entry-point ownership, primary
constructors, synthesized record members, fallback-pattern normalization and
conditional-compilation policy remain separate work. Analysis remains based
on parsed source, not a configured Roslyn compilation.

### Local implementation evidence

Production code and fixtures at `cf9083711533bb8b4fd2df8c23556aaddc9204ea`
passed formatting, warnings-denied all-target clippy, locked debug/release tests,
and the complete live SDK/Coverlet suite on Linux ARM64 with SDK 10.0.109.
The live run executed the new partial/full probes and existing xUnit, NUnit and
MSTest collection, including deliberate failure-path tests. The 187 Python
contracts passed with two existing local skips; actionlint passed. An initial
benchmark test exceeded its existing five-second outer timeout on the shared
host; the unchanged focused retry and full Python retry both passed.

Subsequent local commits change documentation only. Hosted .NET 10/11 platform
results and full mutation evidence remain required before integration.
