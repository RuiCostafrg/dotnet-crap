# Scoring accuracy fixtures

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
that all executable statements are covered. The denominator is exactly the
supplied points, per the existing native coverage contract.

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
Coverage points are parsed once into an ordered map and queried by callable line
range, and line bounds use one cached count per file. A final source hash check
detects changes before publication, but is not a filesystem transaction.

Nested-callable exclusions are stored as merged line intervals, not expanded
sets of every excluded line, so long nested spans do not multiply work by their
line lengths. Coverage lookups use those intervals and the ordered point map.
When coverage input is supplied, selected relative paths containing literal
backslashes or colons are rejected explicitly because version-1 native coverage
cannot represent them. Without coverage input, these source names are preserved.
Backslashes in the source root itself remain valid Unix filename characters.
