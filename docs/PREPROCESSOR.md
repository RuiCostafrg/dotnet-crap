# Conditional compilation fallback

`dotnet-crap` reads C# as written, with the Tree-sitter C# grammar
(`tree-sitter-c-sharp` 0.23.x). The grammar accepts preprocessor directives
only around whole members or statements, so some valid C# fails to parse:

1. An `#if` that splits a declaration or statement, such as a method header
   followed by `#if X { ... } #else ; #endif`.
2. A directive on the last line with no trailing line break, such as a final
   `#pragma warning restore 618`.
3. File-based app directives: `#:sdk`, `#:package`, `#:property`.

## What the scanner does

Every file is parsed as written first. A file that parses is scored from its
own text exactly as before: `--define` has no effect on it, both branches of
its `#if` groups are scored, and its report, hash, scores and line numbers do
not depend on this fallback.

Only when that parse has errors, the scanner resolves conditional compilation
and parses again. The rule is that the resolved text must be what the C#
compiler would compile; whenever the fallback cannot be sure of that, it
declines and the original error is reported.

- `#if`, `#elif`, `#else` and `#endif` are evaluated with the symbols from
  `--define` (none by default). `#define` and `#undef` in active text update
  the symbols in order. Expressions use identifiers, `true`, `false`, `!`,
  `==`, `!=`, `&&`, `||` and parentheses with C# precedence; a trailing `//`
  comment is allowed.
- Active text is lexed: `//` and `/* */` comments, character literals, regular,
  verbatim (`@"..."`) and raw (`"""..."""`) strings, and interpolated forms of
  each (`$"..."`, `$@"..."`, `@$"..."`, `$$"""..."""`), including the code in
  their holes. A line starting with `#` inside a multi-line comment or string
  is text, as the compiler reads it, and is kept. Inactive branches are skipped
  without lexing, as the compiler skips them.
- Directive lines and inactive branches are replaced by spaces. Other
  directives (`#region`, `#endregion`, `#pragma`, `#nullable`, `#line`,
  `#warning`, and `#error` in inactive text), `#:` lines and a first-line `#!`
  are blanked too.
- Every line break is kept and every other byte of a blanked line becomes one
  space, so byte offsets, line numbers and columns are unchanged. The source
  hash is still over the original bytes, and coverage lines bind to the
  original file.

Callables inside inactive branches are not reported. The scan reports the
original error, `invalid C# syntax in <path>`, as it would without the
fallback, when:

- the resolved text still has errors;
- a directive is one the compiler rejects: unbalanced or crossed `#if` and
  `#region` groups, an invalid `#if`/`#elif` expression or `#define`/`#undef`
  symbol (also in inactive text, where the compiler still checks them), an
  unknown directive (also in inactive text), an invalid `#nullable` setting,
  an active `#error`, or `#define`, `#undef` or `#:` after the first token of
  the file;
- active text cannot be lexed with confidence (see Limits);
- no line would change.

A file that does not parse fails the whole scan, as before. A file that parses
only through the fallback is then analysed like any other file, so it can
report a resource-limit error instead of the syntax error.

## The same code can score differently on the two paths

The grammar reads an `#if` inside a method body as part of the method, and an
`#if A && B` there adds one decision point for its `&&` on the parse-as-written
path. On the fallback path the directive line is blank, so the same method
scores one less. A file moves from one path to the other when an unrelated
part of it starts or stops parsing as written. This grammar quirk is tracked
for removal in a separate change.

## Symbols

```sh
dotnet-crap --root . --define NET8_0_OR_GREATER --define FEATURE_ASYNC
dotnet-crap --root . --define "DEBUG;TRACE"
dotnet-crap collect --root . --define NET8_0 --output coverage.json -- ...
```

`--define` is repeatable on `scan` and `collect`, and each value may be a
comma- or semicolon-separated list, as MSBuild `DefineConstants` are written.
A symbol is a letter or `_` followed by letters, digits or `_`, and is not
`true` or `false`; anything else, including an empty list entry, is rejected
with exit 2. The library takes the same list as `ScanOptions::defines` and
`CollectOptions::defines`.

Symbols are not read from project files. Choose the set that matches the build
you want to score; a file's `#if` groups that parse as written are not affected.

## Limits

- Only `\n`, with an optional preceding `\r`, is a line break. A file with a
  lone `\r` or with U+0085, U+2028 or U+2029, which the compiler also treats as
  line breaks, declines the fallback.
- Expressions nest at most 256 levels of parentheses and `!`; a deeper
  expression declines.
- The lexer declines on forms it does not follow: an unterminated comment or
  string, a `#` line inside an interpolation hole, a hole that continues past
  the line of a regular or single-line raw string, a `//` comment or `::` in a
  hole, a format clause that is not plain text on its line, and malformed
  quote, brace or `$` runs.
- `#line` and `#pragma` arguments are not checked.
- Symbol names are compared as written, without Unicode normalization, and
  Unicode escapes (`\u0041`) in directive identifiers are not supported and
  make the fallback decline. A
  symbol's letters and digits follow Rust's Unicode `Alphabetic` and numeric
  properties, an approximation of the C# identifier categories (see the
  [library notes](RUST_API.md)).
- Contextual keywords used as identifiers (`var with = ...`) and non-UTF-8
  sources are not addressed; those files still fail.
- While a file is resolved, its decoded and resolved texts are both held, so
  that file briefly uses twice its text.
