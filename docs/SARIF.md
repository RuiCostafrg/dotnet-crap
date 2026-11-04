# SARIF output

Use `--format sarif` to write SARIF 2.1.0, optionally with `--output report.sarif`.
Gate failures retain the CLI's normal exit codes and still produce a report.

- `DNCRAP001`: callable CRAP strictly above the threshold.
- `DNCRAP002`: callable missing coverage, only with `--require-coverage`.
- `DNCRAP003`: empty selection, only with `--fail-on-empty`.

Passing scans produce an empty results array. A callable can have both score and
completeness findings. Coverage semantics and safe output replacement are the same
as text/JSON. No coverage collector is introduced by this format.

Locations use percent-encoded, root-relative UTF-8 URI paths. `%SRCROOT%` resolves
to the absolute source-root file URI in `originalUriBaseIds`. Regions contain
one-based start/end lines. Columns are deliberately omitted: existing scanner
columns count UTF-8 bytes, which do not match SARIF Unicode column conventions.
The existing JSON shape and scanner locations remain unchanged.

Fingerprints use SHA-256 over rule, path, callable kind/name, and the occurrence
number among callables sharing that path/kind/name. They remain stable when scores,
thresholds or coverage change. They are not semantic C# overload identities:
reordering same-name overloads or anonymous callables can change their association;
renames and moves produce new fingerprints. Baseline identity/move matching is a
separate planned feature. No timestamps or random identifiers enter the output.
