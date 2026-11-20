# NuGet.org publishing

`dotnet-crap` is published on NuGet.org. Version 0.2.0 was the first, pushed
2026-11-19 by `publish-nuget.yml` run 44478346577 from tag `v0.2.0` (source
`2267334808d014b76e363f5ceead63916f68ec11`) after owner approval of the `release`
environment. The immutable GitHub release `v0.1.0` is explicitly excluded and is
not on NuGet.org. This publisher does not change Cargo versions; version bumps
and releases still require separate authorization.

## Supported authentication and first package

[NuGet trusted publishing](https://learn.microsoft.com/en-us/nuget/nuget-org/trusted-publishing)
supports GitHub OIDC via `NuGet/login`, issuing a one-hour API key immediately
before pushing. Policies identify the repository owner, repository, workflow
**filename** and optional GitHub environment. Policy scopes distinguish creating
new packages from publishing versions of existing packages and accept package
globs. This permits a first package with an appropriately scoped policy; a manual
API-key bootstrap is not inherently required.

[NuGet package publication and ownership](https://learn.microsoft.com/en-us/nuget/nuget-org/publish-a-package)
assign ownership to the publishing account, not the nuspec authors. Before first
publication the owner must sign in to NuGet.org, select the actual individual or
organization owner, and verify `dotnet-crap` availability and any prefix
restrictions in that account's package upload/policy UI. A flat-container 404
neither reserves the ID nor proves ownership; unlisted/reserved/conflicting IDs
may still block publication. For an existing ID, obtain confirmed ownership or
accepted co-ownership before continuing. Do not upload a placeholder version.

## Current configuration

1. **Actions variables.** `NUGET_USER` holds the owner's NuGet profile name (not an
   email). `DOTNET_CRAP_NUGET_PUBLISH=true` enables the publisher; the separate
   `DOTNET_CRAP_PUBLISH` flag does not. No persistent API-key secret exists.
2. **Trusted publishing policy** on NuGet.org: owner `RuiCostafrg`, repository
   `dotnet-crap`, workflow `publish-nuget.yml`, environment `release`, package
   exactly `dotnet-crap`. Scope is "Stage packages" only (switched on
   2026-11-20, before `v0.2.1`, the first tag containing the staging workflow);
   no push, no unlist. The workflow runs from the selected tag, so tags `v0.2.0`
   and older, which push directly, **cannot** publish, by design: direct push is
   exactly what the policy removes.
   The only such tag on NuGet.org, `v0.2.0`, is already published, and a rerun
   would fail as a duplicate anyway. Never widen the policy back to Push to rerun
   an old tag; release a new version instead.
3. **Actions allowlist.** SHA enforcement stays on. Besides GitHub-owned Actions
   and the Rust installers, it allows exactly two `NuGet/login` pins, both the
   reviewed v1 code: `8d196754b4036150537f80ac539e15c2f1028841` (the commit, used
   by `main` since #58) and `ebc737b6fc418a6ca0073cf116ec8dc156d8b81e` (the v1
   annotated tag object, still referenced by the workflow inside tag `v0.2.0`).
   Remove the tag-object pin once no release tag that may be republished uses it.
4. **`release` environment.** Owner reviewer `RuiCostafrg`, no admin bypass, only
   `v*` tags. Confirm those protections and immutable releases remain enabled.
5. Nothing triggers automatically on push, release, schedule or another workflow.

## Publishing a release

Dispatch `publish-nuget.yml` **on the selected immutable release tag**, not `main`.
For example, for `v0.2.0`, choose that tag in the Actions
UI and supply `version=0.2.0` and the full lowercase 40-character source SHA.
Use the peeled commit SHA (the commits API), not an annotated tag object SHA.
The tag must contain this workflow and verifier. Dispatching on `main` is skipped;
it could not enter the tag-only environment. Both the original and rerun actors
must be `RuiCostafrg`; the owner must additionally approve the `release` job.
GitHub documents [dispatch ref selection](https://docs.github.com/en/actions/how-tos/manage-workflow-runs/manually-run-a-workflow).

The verifier requires checkout/dispatch/remote tag SHA agreement, ancestry on
`origin/main`, matching Cargo version, stable canonical version, exact release
tag, `immutable=true`, and no draft/prerelease. It downloads only the released
`dotnet-crap.VERSION.nupkg` and its `.sha256` into a fresh directory. It requires:

- [GitHub release attestation verification](https://cli.github.com/manual/gh_release_verify)
  and [release asset verification](https://cli.github.com/manual/gh_release_verify-asset)
  for both files; missing support in the runner's `gh` fails closed.
- [Build provenance verification](https://cli.github.com/manual/gh_attestation_verify)
  for both files, pinned to this repository's `release.yml` certificate identity,
  exact tag and source SHA, GitHub OIDC issuer and GitHub-hosted runner.
- Exact package SHA256 plus internal nuspec ID `dotnet-crap`, version and
  `DotnetTool` package type. Source identity is enforced by the signed provenance,
  not inferred from unsigned nuspec metadata.

Only then does `NuGet/login` exchange OIDC. The stage step rechecks the checksum
and uploads that exact file with `dotnet package stage push`, with no pack,
signing, modification, install or execution of package contents in the
credential-bearing job. The command requires GA .NET SDK 11.0.100 (pinned in the
workflow); the API key comes from the `NUGET_API_KEY` environment variable, not
the command line. Permissions are contents/read, attestations/read and
id-token/write; no GitHub write permission is granted. Staging alone makes no
version public or restorable by anyone.

## Promoting a staged release

Staging is only half of publishing. After a successful run, the owner signs in to
NuGet.org with 2FA, reviews the staged package and version, and promotes it
manually; the website is the only promotion path. Unpromoted staged content
expires after a period NuGet does not document, so promote promptly. The workflow
writes the staged version to the run summary. Run the `verify-published.yml`
NuGet job only after promotion: until then the package is intentionally absent
from the public flat container.

**If the stage step fails**, NuGet may still have accepted the upload (for
example, the response was lost). NuGet offers no API to inspect staged packages,
so before any rerun the owner checks the staged packages on NuGet.org:

- Version **not staged**: fix the cause and rerun the workflow on the same tag.
- Version **already staged**: do not rerun (a duplicate stage is expected to
  fail). Review it on NuGet.org (package ID, version, validation status) and
  promote it. The upload can only have come from this workflow's verified file,
  because the trusted-publishing key is issued to this workflow on the release
  tag after verification. `verify-published.yml` then proves that the promoted
  package matches the authenticated release package entry by entry; if it does
  not, unlist that version and release a new one.

The workflow writes these instructions to the run summary when staging fails.

Every registry conflict fails: **no `--skip-duplicate`**, including retries with
identical bytes; stage behaviour for duplicate or conflicting content is
undocumented, and the step fails on any non-zero exit. Thus a different archive
can never be silently accepted as a successful publication. NuGet repository
signing can change the downloaded registry archive; do not normalize ZIPs or
treat that difference as proof of exact byte identity. On conflict, stop and
investigate manually. Do not delete/rebuild or overwrite a version. Network,
authentication and missing-attestation failures also stop before staging. The old
`scripts/publish-nuget.sh` is not used by this workflow and retains its historical
behavior for its existing consumers.

## Validation

Run `python3 -m unittest discover -s tests -p 'test_nuget_publisher.py'` and
`actionlint .github/workflows/publish-nuget.yml`. Offline contracts exercise source,
release and package rejection, simulated cryptographic verification failures,
and actual stage-step conflict propagation. These do not prove live OIDC exchange,
ID ownership, NuGet acceptance, promotion or cross-platform installation. Existing
release packaging tests install the package before release; this publisher never
rebuilds it. After promotion, `verify-published.yml` installs the package from
NuGet.org itself (see `docs/PUBLISHED_VERIFICATION.md`).

First live run (2026-11-19): run 44478346577 passed every step (release and
provenance verification, OIDC exchange, push). NuGet.org then served `0.2.0` in its
flat container, and `dotnet tool install dotnet-crap --version 0.2.0` from
NuGet.org on the ARM64 Termux host ran and reported `dotnet-crap 0.2.0`.

Version 0.2.1 was the first staged release: `publish-nuget.yml` run
[44714375635](https://github.com/RuiCostafrg/dotnet-crap/actions/runs/44714375635)
staged it on 2026-11-20 under the "Stage packages" policy, the owner promoted it
on NuGet.org, and `verify-published.yml` run
[44728168904](https://github.com/RuiCostafrg/dotnet-crap/actions/runs/44728168904)
verified every channel, including the NuGet.org package on all five tool RIDs.

Local preparation evidence (2026-11-13): all 192 Python contracts passed, with two
optional tests skipped; actionlint 1.7.12 passed for the new workflow. The existing
`v0.1.0` package was downloaded read-only, release-asset and build-provenance
verified against source `2565412e3d2326d79e98963ed0b8df7de3020659`, checksum-checked,
and installed from an isolated local feed. Version output and Fixture scans at
thresholds 100/0 produced the expected exits 0/1. This ARM64 Termux host required
its explicit `DOTNET_ROOT` and `DOTNET_SYSTEM_GLOBALIZATION_INVARIANT=1`; this is
one local installation observation, not five-platform verification. It does not
run the publisher for `v0.1.0`, which remains excluded, or modify that release.
