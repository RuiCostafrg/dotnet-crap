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
and parses again:

- `#if`, `#elif`, `#else` and `#endif` are evaluated with the symbols from
  `--define` (none by default). `#define` and `#undef` in the file update the
  symbols in order. Expressions use identifiers, `true`, `false`, `!`, `==`,
  `!=`, `&&`, `||` and parentheses with C# precedence; a trailing `//` comment
  is allowed.
- Directive lines and inactive branches are replaced by spaces. Other
  directives (`#region`, `#endregion`, `#pragma`, `#nullable`, `#line`,
  `#warning`, `#error`), `#:` lines and a first-line `#!` are blanked too.
- Every line break is kept and every other byte of a blanked line becomes one
  space, so byte offsets, line numbers and columns are unchanged. The source
  hash is still over the original bytes, and coverage lines bind to the
  original file.

Callables inside inactive branches are not reported. If the resolved text
still has errors, or a directive is malformed (an unbalanced group, an invalid
expression or symbol, an unknown directive in active text), the scan reports
the original error, `invalid C# syntax in <path>`, as it would without the
fallback. A file that does not parse fails the whole scan, as before.

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

- Directives are recognised per line, as the first non-whitespace text of the
  line, without a lexer. A line starting with `#` inside a multi-line comment
  or string is treated as a directive. When that changes the meaning, the
  reparse usually fails and the original error is reported.
- Contextual keywords used as identifiers (`var with = ...`) and non-UTF-8
  sources are not addressed; those files still fail.
- Symbol names are compared as written, without Unicode normalization.
