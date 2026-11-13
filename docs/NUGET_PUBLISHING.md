# NuGet.org publisher preparation

This is a reviewable proposal, not evidence of NuGet publication or package ID
ownership. No account, repository setting, secret, environment approval, tag or
release is created by this change. The immutable GitHub release `v0.1.0` is
explicitly excluded. This publisher does not change Cargo versions; version bumps
and releases require separate authorization. Old schedules remain disabled.

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

## Owner configuration still necessary — do not apply as part of this PR

1. Confirm the NuGet account/profile name and intended package owner. Organization
   membership must be active. Set the non-secret `NUGET_USER` Actions variable
   to the login profile name (not an email); no persistent API-key secret is used.
2. Create a NuGet trusted publishing policy for repository owner `RuiCostafrg`,
   repository `dotnet-crap`, workflow filename `publish-nuget.yml`, environment
   `release`. Restrict the glob to **exactly `dotnet-crap`**. Permit creation of
   a new package only if this is the first package; otherwise permit new versions
   for the verified owner. Review policy activation/expiry warnings in NuGet's UI.
3. The documented repository Actions allowlist currently allows GitHub-owned
   Actions and selected Rust installers. Propose adding **only**
   `NuGet/login@ebc737b6fc418a6ca0073cf116ec8dc156d8b81e` (reviewed v1, Node 24),
   if it is not already allowed. Keep SHA enforcement and all existing restrictions.
   This workflow cannot run until that action is allowed; no settings are changed here.
4. Keep the existing `release` environment: owner reviewer `RuiCostafrg`, no admin
   bypass, only `v*` tags. No new environment or branch exception is necessary.
   Independently confirm those protections and immutable releases remain enabled.
5. After separate authorization, merge this workflow, make a future version/release
   whose tag includes it, then set the **separate repository variable** `DOTNET_CRAP_NUGET_PUBLISH=true`.
   The existing `DOTNET_CRAP_PUBLISH` flag does not enable this publisher. Leave
   the new variable absent/false until explicitly authorized. Nothing triggers
   automatically on push, release, schedule or another workflow.

## Authorized execution contract (future use only)

Dispatch `publish-nuget.yml` **on the selected immutable release tag**, not `main`.
For example, for a separately approved `v0.2.0`, choose that tag in the Actions
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

Only then does `NuGet/login` exchange OIDC. The push rechecks the checksum and
uses that exact file, with no pack, signing, modification, install or execution of
package contents in the credential-bearing job. Permissions are contents/read,
attestations/read and id-token/write; no GitHub write permission is granted.

Every registry conflict fails: **no `--skip-duplicate`**, including retries with
identical bytes. Thus a different archive can never be silently accepted as a
successful publication. NuGet repository signing can change the downloaded
registry archive; do not normalize ZIPs or treat that difference as proof of exact
byte identity. On conflict, stop and investigate manually. Do not delete/rebuild
or overwrite a version. Network, authentication and missing-attestation failures
also stop before push. The old `scripts/publish-nuget.sh` is not used by this
workflow and retains its historical behavior for its existing consumers.

## Validation

Run `python3 -m unittest discover -s tests -p 'test_nuget_publisher.py'` and
`actionlint .github/workflows/publish-nuget.yml`. Offline contracts exercise source,
release and package rejection, simulated cryptographic verification failures,
and actual push-step conflict propagation. These do not prove live OIDC exchange,
ID ownership, NuGet acceptance or cross-platform installation. Existing release
packaging tests install the package before release; this publisher never rebuilds
it. Owner setup and the first authorized live run remain outstanding.

Local preparation evidence (2026-11-13): all 192 Python contracts passed, with two
optional tests skipped; actionlint 1.7.12 passed for the new workflow. The existing
`v0.1.0` package was downloaded read-only, release-asset and build-provenance
verified against source `2565412e3d2326d79e98963ed0b8df7de3020659`, checksum-checked,
and installed from an isolated local feed. Version output and Fixture scans at
thresholds 100/0 produced the expected exits 0/1. This ARM64 Termux host required
its explicit `DOTNET_ROOT` and `DOTNET_SYSTEM_GLOBALIZATION_INVARIANT=1`; this is
one local installation observation, not five-platform verification. It does not
run the publisher for `v0.1.0`, which remains excluded, or modify that release.
