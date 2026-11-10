# Mutation survivor review

Source/test revision: `1cad955cbe5ec29590fb405174c495826de5f39f`.
Mutation workflow run: 42368570749 (complete CI run: 42368570770); both checked out
PR merge commit `76a45d2` (`1cad955` merged into base `8620f71`); cargo-mutants 27.1.0. Exact identities are retained in
`final/missed.txt` and `final/unviable.txt`; this review does not exclude any.
The gate counts timeouts as viable non-kills (there were none).

## All 24 missed cases

- **Staging parent filters (2):** `src/main.rs:128` and `src/collector.rs:142`,
  deleting `!` from the parent filter. Bare names can still stage successfully
  on this host; absolute destinations can also persist when cwd and destination
  share a filesystem. The mutation can change staging location and fail across
  filesystems, so this is not claimed globally equivalent. Current tests lack a
  controlled separate-filesystem destination; production retains destination-side
  staging.
- **XML byte-limit boundaries/races (8):** `cobertura.rs:65` (two multiplication
  changes), `:67` (two metadata comparisons), `:71` (two read-limit arithmetic
  changes), and `:72` (two length comparisons). Small valid reports plus the sparse
  oversized fixture do not distinguish lower-but-still-large limits, exact-limit
  acceptance, or growth after metadata inspection. Some metadata-check changes
  are caught by the later bounded read instead; others are real coverage gaps for
  valid large documents or growth races. Production retains the 16 MiB check,
  bounded read with one sentinel byte, and post-read check. Add large-valid and
  exact-boundary fixtures before claiming these cases equivalent.
- **Root fallback predicates (4):** `cobertura.rs:264` and three at `:265`.
  Selected exact root-relative paths often force the same fallback regardless of
  earlier predicates. Unselected root-relative files with missing prefixes, or a
  real unselected root namesake beside a selected prefixed file, can distinguish
  the mutations. These are test gaps, not blanket equivalence claims. Production
  retains the documented exact-prefix/root-fallback policy.
- **Candidate containment and resolution errors (2):** `cobertura.rs:299` changes
  `||` to `&&`; later root-prefix validation still rejects outside-root files,
  while directories fail the file check. It primarily changes rejection order.
  `:310` accepts every canonicalization error as NotFound, which can change
  handling of permission/NotADirectory failures for supplied snapshot identities.
  Not all such filesystem errors are covered on the recorded host.
- **Equivalent drive/scheme guard (1):** `cobertura.rs:341`. `drive_absolute`
  implies `scheme_like`, and the later conjunction is false otherwise, so this
  mutation is equivalent on every platform. It is not excluded from the score.
- **Windows/URI discrimination (3):** `cobertura.rs:345`, `:346`, `:347`.
  On the Linux mutation runner colon-bearing foreign paths still reject through
  another branch. Windows acceptance of host drive paths could distinguish these
  conditions. The real Windows CI path tests pass, but this was not a Windows
  mutation invocation; no cross-platform equivalence is asserted.
- **Absolute empty path components (1):** `cobertura.rs:373`, `&&` to `||`.
  Relative double-slash/dot fixtures reject, but an absolute filename with an
  internal empty component could be incorrectly allowed by this mutation.
  Production rejects it; add a host-absolute noncanonical spelling regression.
- **Native source-character preflight (1):** `collector.rs:69`, `||` to `&&`.
  Invalid source identities can be rejected again by mapping/native validation,
  so publication still fails for tested cases. The mutant can execute trusted
  code before that later rejection; current tests do not observe that preflight
  side effect for unusual source filenames.
- **Non-report attachments (1):** `collector.rs:109`, `&&` to `||`.
  Private directories in the unit fixtures contain only the expected regular
  report file. Other regular attachments would be incorrectly imported or change
  the count under this mutation. Add an ignored diagnostic attachment fixture;
  production still selects the exact Cobertura filename.
- **Destination metadata error classification (1):** `collector.rs:177` treats
  every metadata error as NotFound. Later staging/persistence often still fails,
  but unusual permission/name errors can alter whether the command executes.
  Production preserves the specific NotFound guard; those error classes need
  additional portable test coverage.

These gaps are retained in the denominator. The measured result passes the
required >90% gate; it is not proof that all malformed inputs or filesystem
interleavings have been exercised. Further test hardening and dense-workload
profiling remain explicit follow-up work, without weakening production guards.

## All 11 unviable cases

The retained build logs confirm compile failures, not test kills:

- Five library replacements require unsupported `Default` implementations for
  `Report`, `Path`, generic coverage values or `ScanResult`.
- Four parser default replacements require `Default` for `Document` or `Node`.
- Two comparison replacements put `<` directly after an `as u64` / `as usize`
  cast; Rust parses it as generic arguments and rejects the mutated syntax.

These are separate from caught/missed/timeouts. None are counted as caught.
