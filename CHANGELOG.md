# Changelog

## Unreleased

- `task` children keep the parent's shell and language-server tools, run
  up to 32 rounds under a 15-minute deadline, and stream every tool call,
  result, and answer into the parent transcript under the child id.
  Cancelling a batch removes child worktrees that hold no change.
  `task` also takes `background`, `child`, and `kill`: start a child and
  return its id, poll its state and last action, or stop it. A turn defers
  while a background child runs and stops it at the deferral limit.
- Context overflow recovery clears the oldest tool results first and keeps
  the newest two. A failed auto-compaction is not retried until the next
  turn.
- `summon room`: Claude Code and Codex join the bus through
  `summon room serve`, a stdio MCP server with join, agents, send, inbox,
  wait, board, status, and leave. `summon room host codex` runs Codex as a
  peer that answers mail on one thread. `summon room setup claude|codex`
  installs the `/summon-room` skill and the MCP entry. Roster records and
  mail carry the agent kind.

## 0.1.0 - 2026-08-23

First alpha of the Mage harness under the Summoner name.

- Command is `summon` or `summoner` (same binary). Crate is `summoner`.
- Connector owns OpenAI `chat/completions` (HTTP, SSE) and native
  Anthropic Messages / Gemini wires. Credentials: API key, RFC 6749
  token, or RFC 8628 device login. CoT is `reasoning_content`. The
  `task` runs up to four depth-one child sessions concurrently. Writable
  children use separate Git worktrees. Summoner combines their patches and
  removes the worktrees before the parent verifies once. Child sessions have
  no command, language-server, web, or background tools. `/model` lists every
  resolvable seat.
- Work: plan, execute, verify. `--steps` and `--goal` hold the job.
- Git worktrees for `--goal` / `--steps` when the directory is a repo.
  Launch does not sweep live trees. `prune` only forgets missing dirs.
- TUI (Sigil), policy, sandbox, session journal.
- Commands and post-edit checks run in the OS sandbox with network denied
  unless the user passes `--net` or `--no-sandbox`.
- Turn receipts return every completion check separately from model text.
  Diagnostics use one locked JSONL file per run; `logs` lists them and
  `doctor` reports crashes or incomplete runs. Malformed goal verdicts
  stop the Work instead of silently continuing it.

Not in this alpha: crates.io (Sigil is a git dependency), Windows,
renaming `~/.mage`.
