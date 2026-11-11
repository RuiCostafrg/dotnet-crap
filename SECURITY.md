# Security

Treat source code and coverage reports as untrusted inputs. The scanner parses
C#; it does not compile or execute it. Running the fixture or collecting coverage
does execute .NET code and should only be done for projects you trust.

Native coverage source hashes detect accidental stale input. They do not prove
that a report came from tests or from a trusted producer. Generate coverage in
your own pipeline and review changes to gate policy.

Release archives include SHA-256 checksums. A checksum detects corruption; it is
not proof of publisher identity. Published release assets and npm tarballs carry
GitHub build-provenance attestations from `.github/workflows/release.yml` at the
version tag; verify them with `gh attestation verify <file> --repo
RuiCostafrg/dotnet-crap`. The GitHub Action verifies the attestation (exact
signer identity, tag, no self-hosted runners) and the checksum before running the
binary, and never disables TLS.

The release workflow is hand-maintained and splits privileges (see
[docs/DISTRIBUTION.md](docs/DISTRIBUTION.md)). Planning, building, packing and
every installation check run with a read-only token, no environment and no
secrets. Only two jobs can publish: one creates the attested GitHub release and
one stages npm packages with trusted publishing (OIDC, no npm token). Both wait
for owner approval in the `release` environment, check out no code, run no
compiled binary, and are disabled unless the repository variable
`DOTNET_CRAP_PUBLISH` is `true`. That variable must stay unset until tag and
branch rulesets and the environment's required reviewer are enforced. The
Homebrew tap pulls and verifies the attested formula itself; this repository
holds no tap token. Removing these controls requires a maintainer who can change
workflows, so workflow changes need review like code.

Report security issues privately through the repository's GitHub security
advisories, rather than posting sensitive details in a public issue.
