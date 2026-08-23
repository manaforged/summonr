# Changelog

## 0.1.0 - 2026-08-23

First alpha of the Mage harness under the Summoner name.

- Command is `summon` or `summoner` (same binary). Crate is `summoner`.
- Connector owns OpenAI `chat/completions` (HTTP, SSE) and native
  Anthropic Messages / Gemini wires. Credentials: API key, RFC 6749
  token, or RFC 8628 device login. CoT is `reasoning_content`. The
  `agent` tool runs one nested agent. `diagnostics` and `definition`
  use one language server. `/model` lists every resolvable seat.
- Work: plan, execute, verify. `--steps` and `--goal` hold the job.
- Git worktrees for `--goal` / `--steps` when the directory is a repo.
  Launch does not sweep live trees. `prune` only forgets missing dirs.
- TUI (Sigil), policy, sandbox, session journal.

Not in this alpha: crates.io (Sigil is a git dependency), Windows,
renaming `~/.mage`.
