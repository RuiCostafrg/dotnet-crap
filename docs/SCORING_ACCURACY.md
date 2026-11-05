# Scoring accuracy fixtures

These fixtures use synthetic line hits in the Rust CLI tests, not collected coverage. Switch has base 1 plus two case labels: complexity 3; 1/2 covered gives 3 + 9/8 = 4.125. Branches has base 1, while, &&, ?? and catch: complexity 5; 1/2 covered gives 5 + 25/8 = 8.125. Nested's outer method has complexity 1, excluding the local function and its line; declaration and return lines are 1/2 covered, giving 1 + 1/8 = 1.125. Equality with threshold passes; a lower threshold fails.

Source hashes bind the exact original bytes, including byte order marks. UTF-8 and BOM-marked UTF-16 are decoded for parsing; line numbers refer to decoded lines and columns are one-based UTF-8 byte offsets in the decoded text. A final source hash check detects changes before report publication, but is not a filesystem transaction.
