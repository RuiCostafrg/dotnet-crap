# Security

Treat source code and coverage reports as untrusted inputs. The scanner parses
C#; it does not compile or execute it. Running the fixture or collecting coverage
does execute .NET code and should only be done for projects you trust.

Native coverage source hashes detect accidental stale input. They do not prove
that a report came from tests or from a trusted producer. Generate coverage in
your own pipeline and review changes to gate policy.

Release archives include SHA-256 checksums. A checksum detects corruption; it is
not independent proof of publisher identity. Pin versions and obtain assets from
this repository's GitHub Releases. The installer action verifies the checksum and
never disables TLS.

Publication uses the `release` GitHub environment for `v*` tags. The publish job
also checks that the tagged commit is reachable from `main`, but that check runs
from the tagged checkout. The enforcement against an untrusted tag is the
environment itself, so maintainers must configure these settings, which this
repository cannot enforce from its files: required reviewers on `release`, a
deployment policy limited to `v*` tags with a protected tag ruleset, and
`NUGET_API_KEY` stored as an environment secret of `release`, not as a repository
secret. No credential is stored in this repo. See `packaging/README.md`.

Report security issues privately through the repository's GitHub security
advisories, rather than posting sensitive details in a public issue.
