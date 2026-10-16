# Security policy

## Supported versions

Security fixes go into the latest release only. Update to the newest version before you report a problem.

## Report a vulnerability

Do not open a public issue for a vulnerability.

1. Open a private report at <https://github.com/manaforged/summonr/security/advisories/new>.
2. Include the summonr version (`summonr -V`), the operating system, and the steps to reproduce the problem.
3. Do not include API keys or session files that contain secrets.

You get a reply within 7 days. After a fix is released, the advisory is published with credit to you, unless you ask to stay anonymous.

## Network

summonr sends no telemetry. It connects only to:

- The endpoint you run, and the sign-in pages of a provider when you sign in to it.
- `https://models.dev/api.json` when the `/model` picker or `summonr endpoints` lists models.
- The search engines the web tools use, when the model searches the web or fetches a page.
- MCP servers, hooks, and gateways that you configure, such as Telegram for `summonr gateway telegram`.

## Local files

summonr keeps its keys in `~/.summonr/credentials.toml` (mode 0600). To reuse a sign-in you already have, it also reads OpenCode's `~/.local/share/opencode/auth.json` and Kimi Code's `~/.kimi-code/credentials/kimi-code.json` when they exist. It never writes them, and a key you store with `summonr login` wins over theirs.

## Scope

In scope: the `summonr` binary, its command and write policy, its credential storage, and the release artifacts on GitHub.

Out of scope: model output, and model providers' own services.
