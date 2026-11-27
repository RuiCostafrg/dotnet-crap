# Stream Cobertura in two passes

Replace the in-memory XML document tree with a two-pass reader to bound importer memory while preserving XML validation, error precedence, and existing diagnostics. Validate XML and count structural elements first, then stream coverage records with the complete structure known, so malformed XML takes precedence over semantic errors and late `<sources>` entries still resolve paths. A single pass would need to buffer pending records and defer semantic errors, trading a second tokenization pass for more state in the importer.

See [the mapping contract](../COVERLET.md#mapping-contract) and [the importer PR](https://github.com/RuiCostafrg/dotnet-crap/pull/47).
