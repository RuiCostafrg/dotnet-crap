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

On Unix, filenames containing literal backslashes or colons are supported when
no coverage input is supplied; SARIF percent-encodes these characters. Scans with
native coverage reject these names because version 1 cannot address them.

Fingerprints use SHA-256 over rule, path, callable kind/name, and the occurrence
number among callables sharing that path/kind/name. They remain stable when scores,
thresholds or coverage change. They are not semantic C# overload identities:
reordering same-name overloads or anonymous callables can change their association;
renames and moves produce new fingerprints. Baseline identity/move matching is a
separate planned feature. No timestamps or random identifiers enter the output.

## Validation evidence

Locked debug/release tests, formatting, clippy with warnings denied and live .NET
fixture tests passed. `reports/sarif-mutation.json` records the tested source
commit/digests, exact invocation and each caught mutant. With cargo-mutants 27.1.0,
all 15 mutants generated for `src/sarif.rs` were caught; none were missed, timed out
or unviable. The focused tests are the binary unit tests and `tests/sarif.rs`.
This renderer-only run does not establish the repository-wide mutation score.

The initial attempt to copy the build cache failed with a filesystem read-link
error on this Android runner. The completed run used `--copy-target false` and
rebuilt dependencies in its isolated scratch directory; no production mutations
were excluded within the selected renderer file.
