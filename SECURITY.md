# Security policy

## Supported versions

Security fixes go into the latest release only. Update to the newest
version before you report a problem.

## Report a vulnerability

Do not open a public issue for a vulnerability.

1. Open a private report at
   <https://github.com/manaforged/summonr/security/advisories/new>.
2. Include the summonr version (`summonr -V`), the operating system, and
   the steps to reproduce the problem.
3. Do not include API keys or session files that contain secrets.

You get a reply within 7 days. After a fix is released, the advisory is
published with credit to you, unless you ask to stay anonymous.

## Scope

In scope: the `summonr` binary, its sandbox (`--sandbox`), its
credential storage, and the release artifacts on GitHub.

Out of scope: model output, and model providers' own services.
