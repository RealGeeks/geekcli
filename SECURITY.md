# Security policy

## Reporting a vulnerability

Please report vulnerabilities privately through GitHub: on this repository's
**Security** tab choose **Report a vulnerability**, or go straight to
<https://github.com/RealGeeks/geekcli/security/advisories/new>. Do not open a
public issue, pull request or discussion for a security problem.

Useful details: the geekcli version (`geekcli --version`), your operating
system, the command involved, and what an attacker could do. Never include a
real API key; revoke any key that may have been exposed under
**Admin → API keys** on the site.

We will acknowledge the report, keep you updated while we work on a fix, and
credit you in the advisory if you would like.

## Supported versions

Security fixes go into the latest release. Update with the install command
in the [README](README.md#install) or by downloading the newest
[release](https://github.com/RealGeeks/geekcli/releases).

## Scope

This repository covers the `geekcli` client and its install scripts. Problems
with a Real Geeks website or the Content API itself can be reported the same
way and will be passed to the right team.

## Verifying a download

Every release archive has a `.sha256` checksum beside it and a signed
build-provenance attestation. To check that an archive was built from this
repository by its release workflow:

```bash
gh attestation verify geekcli-<version>-<target>.tar.gz --repo RealGeeks/geekcli
```
