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
permission and has no publication job. Host protections are now enforced: owner
review and mandatory CI on `main`, owner-only and immutable `v*` tags, an allowlist
of SHA-pinned Actions, secret scanning with push protection, and a `release`
environment that requires the owner's approval for `v*` tags. See
[docs/SECURITY_PARITY.md](docs/SECURITY_PARITY.md). Automatic publication remains
unavailable until a publication job that uses that environment is reviewed. The
owner's admin role can still bypass review, and a maintainer who can change
workflows can still add a job.
See `packaging/README.md` for the retained, tested publication helpers.

Report security issues privately through the repository's GitHub security
advisories, rather than posting sensitive details in a public issue.
