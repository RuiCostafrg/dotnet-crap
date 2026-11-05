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

The tag-triggered workflow builds artifacts with read-only repository contents
permission and has no publication job. Automatic publication is unavailable until
a future publication workflow is reviewed and host protections can be enforced;
the current repository plan cannot enforce required reviewers. Removing this job
does not prevent a maintainer who can change workflows from adding another one.
See `packaging/README.md` for the retained, tested publication helpers.

Report security issues privately through the repository's GitHub security
advisories, rather than posting sensitive details in a public issue.
