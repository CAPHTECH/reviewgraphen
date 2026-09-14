# Security policy

## Supported versions

Security fixes are provided for the latest published `0.1.x` release. Before
the first public release, only the current `main` branch is supported.

## Reporting a vulnerability

Do not disclose a suspected vulnerability in a public issue. Use GitHub's
private vulnerability reporting for this repository. If that surface is not
available, contact the repository owner privately through the CAPHTECH GitHub
organization before sharing reproduction details.

Include the affected version or commit, platform, impact, and the smallest safe
reproduction you can provide. Do not include third-party credentials or source
code you are not authorized to share.

An acknowledgement is targeted within seven days. A remediation timeline is
set after impact and exploitability are confirmed; this document does not
promise that an unverified report is a vulnerability.

## Scope

The supported security boundary is the one documented for each platform. In
particular, macOS does not silently substitute weaker path-based durability for
the Linux-only Store contract. Raw model prose is non-authority, arbitrary
reviewer shell execution is prohibited, and stale Evidence cannot sign off a
current snapshot.
