# Isolate release publication

Build and test artifacts with a read-only token, then pass a verified digest and the artifacts to a fresh owner-approved job that can attest and publish them without checking out or executing release code. NuGet and Homebrew publish through their own approval flows using those immutable assets, so a build or candidate installer never needs the destination's write privileges. The extra approval and verification steps keep distribution channels tied to one reviewed build.

See [the release workflow](../DISTRIBUTION.md), [NuGet publishing](../NUGET_PUBLISHING.md), and [published verification](../PUBLISHED_VERIFICATION.md).
