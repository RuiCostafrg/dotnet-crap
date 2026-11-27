# Bind collected coverage to source bytes

Normalize fresh Cobertura from trusted test commands into the same path, hash, and line evidence format that scans already consume, so scoring is independent of the collector and stale evidence can be rejected. Parse and hash selected files before running tests, validate staged evidence against that snapshot with the shared scan validator, and recheck source bytes before publishing to preserve the changed-source guarantee without a second full parse. This requires extra source reads and strict path matching because timestamps and file names alone cannot bind evidence to the scanned bytes.

See [collection and mapping](../COVERLET.md) and [native evidence](../../examples/COVERAGE.md).
