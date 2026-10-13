# Changelog

## Unreleased

First public release.

### Added

#### Install and endpoints

- Release binaries for macOS on Apple silicon, Linux (static musl builds), and Windows x86_64. `curl -fsSL https://summonr.dev/install.sh | sh` installs on macOS and Linux, and `irm https://summonr.dev/install.ps1 | iex` installs on Windows. Every release publishes `SHA256SUMS` and build attestations (`gh attestation verify <file> --repo manaforged/summonr`).
- 41 built-in endpoints, from Anthropic and OpenAI to coding plans and local servers. Each is one `[[preset]]` row in `src/endpoint/presets.toml`. New built-ins include the Qwen, MiniMax, Tencent, and Volcengine coding plans, the DashScope hosts, `nvidia`, `huggingface`, `cohere`, `siliconflow`, `ollama-cloud`, and `lmstudio`.
- Providers with several regions or plans (`zai`, `alibaba`, `kimi`, `minimax`, `siliconflow`, `anthropic`, `opencode`) list once. Each region and plan is a route; `summonr NAME` takes the provider or a route name such as `zai-cn`, and the retired names `glm`, `glm-intl`, and `zen` still work with their stored keys.
- `summonr login NAME --url URL [--model M]` adds your own OpenAI-compatible server, and `summonr logout NAME --remove` deletes it. `summonr login NAME --default` makes an endpoint the default.
- `summonr login openai` offers Sign in with ChatGPT, and `summonr login grok` offers a SuperGrok or X Premium+ sign-in, next to an API key. Sign-ins are stored apart from keys.
- The `claude` endpoint runs a Claude Pro or Max plan through your own Claude Code (`claude auth login`). summonr never reads the token, Claude Code never sees an `ANTHROPIC_API_KEY` from your environment, and summonr's tools, policy, sandbox, and checks apply as on any other endpoint.
- Default models follow each vendor's current list, and each vendor gets the reasoning, caching, and tool settings its docs name, such as `apply_patch` edits for GPT-5, GPT-6, and Codex models.

#### The terminal interface

- `/model`, `/login`, and `/usage` are full-screen pages that share a tab row. `/model` leaves out providers that do not answer and shows each model's context, price, release date, and abilities. `/login` groups providers as ready, needs attention, and not set up, and shows a provider's region and plan as buttons.
- Several sessions stay live in one terminal or window: `/new` starts one beside the current session, and `/resume` switches.
- Tool calls take one line each and fold into groups, an edit shows its diff under the call, and the diff pane reviews one hunk at a time (`a` accept, `r` revert, `n` note).
- `shift+tab` cycles reasoning effort, `ctrl+n` folds thinking and tool output, and the footer shows context, cache, and spend.
- Three themes, `space`, `nord`, and `dawn`, with `theme = "auto"` to follow the terminal background.
- The start screen draws a drifting starfield over a galaxy band that changes every launch.
- On macOS, key hints show `⌥` for alt.

#### Goals, checks, and automation

- `--goal` runs until the check passes and a second model call agrees the goal is met. With no check configured, summonr uses one it detects (`cargo test`, `go test ./...`, or `npm test`).
- `--steps PLAN.md` runs a numbered plan with a check per step; a step that says ``Run `CMD` `` gets `CMD` as its check.
- `summonr diff --turn N|last` shows what one turn changed.
- `summonr cron` runs a prompt or a goal on a schedule, and `summonr cron install` registers it with launchd, systemd, or Task Scheduler. `summonr gateway webhook` starts a job from `POST /run/NAME`, and `summonr gateway telegram` drives sessions from a paired Telegram chat.
- `--json` streams one event per line, and the `aborted` event names its cause.

#### Extensions

- MCP 2025-11-25 servers, with prompts, resources, forms, and progress. A server starts on its first call and stops when idle, and `/mcp` and `summonr mcp list|get|add|remove` manage servers.
- Plugins in the Claude Code layout: `summonr install PATH` adds skills, slash commands, agents, MCP servers, language servers, hooks, and `bin/`.
- Hooks for `pre_tool`, `post_tool`, `compact`, and `stop`, including Claude Code hooks with `dialect = "claude"`.
- Memory between sessions in `~/.summonr/memory/`, shown by `/memory`.
- The `browser` module (off by default) drives a real Chrome profile, and web search falls back across Bing, DuckDuckGo, Yahoo, and Brave.
- The model can call `ask` to put up to four questions to you in a picker.

### Changed

- Commands run without a prompt. Only irreversible destruction, such as wiping the home or project folder, `mkfs`, or `terraform destroy`, asks first; `confirm = "risky"` in `policy.toml` brings back prompts for `sudo`, force pushes, and protected-path writes.
- Plan mode is removed. `/goal`, `--goal`, and `--steps` replace it.
- Search is its own `search` tool, and common `grep -r`, `rg`, and `find -name` commands are answered by it.
- In `--goal` and `--steps`, the check decides the exit code.
- `enter` during a turn sends the message into the running turn.
- Compaction clears old tool output first and asks for a summary only when that is not enough.

### Fixed

- Cancelling a turn no longer kills background children, and a resumed session gets its full time and token budget again.
- Dragging across the transcript copies exactly the text crossed.
- MCP calls survive timeouts, server restarts, and expired remote sessions.
- Long tool-argument streams, keep-alive-only streams, and rejected OAuth tokens no longer stall or fail a turn.
- Windows commands report readable errors, a locked file no longer stops a run, and child sessions work.

### Security

- Secret values from `.env` files, cloud credential files, and private keys show as `[secret]` in every tool result.
- Writes to `~/.ssh`, `~/.gnupg`, and `~/.summonr` are refused in every path form, including Windows verbatim paths and a symlinked home.
- The model cannot write `.summonr/check.toml` or `.summonr/policy.toml`.
- `~/.summonr` is created with mode `0700`, and `~/.summonr/credentials.toml` with mode `0600`.
- A plan sign-in must return the nonce summonr sent, and a workspace `SYSTEM.md` loads only when the workspace is trusted.

## 0.1.0-rc.1 - 2026-09-23

### Added

- Release binaries for macOS (arm64, x86_64) and Linux (x86_64, arm64), each with a SHA-256 file, and `scripts/install.sh` to download, check, and install one.
- The `/model` picker opens from cache in about 0.3 s and checks each provider in the background. It groups current, pinned (`alt+p`), recent, and providers, and shows one state word per provider with a one-line fix. `F5` rechecks a failed provider. `summonr endpoints` uses the same state words and the same discovery path.
- Sign-in happens inside the dialog. A pasted key is checked against the provider before it is saved. OAuth shows the device URL, code, and elapsed time. `/login` filters and groups providers, `/login NAME` opens the key prompt, and `alt+d` signs out.
- Endpoints read keys from OpenCode's `auth.json` and Kimi's `kimi-code.json`. `summonr auth` names the source.
- Built-in endpoints `together`, `groq`, `cerebras`, `mistral`, `muse`, `zen`, `go`, and `opencode`. `responses` is a selectable wire.
- A sandboxed write or network denial asks `[ allow ]`, `[ always ]`, or `[ deny ]`.
- `web` fetches as markdown, text, or html.
- A fenced `chart` block, or mermaid `xychart`, renders as labeled bars.
- `theme` accepts `space`, `fantasy`, `nord`, `gruvbox`, `solarized`, and `light`.
- CLI verbs `summonr resume`, `summonr install`, and `summonr doctor --logs`.

### Changed

- summonr depends on the one Sigil package `sigil-ui` (feature `terminal`) instead of `sigil-gui` plus `sigil-tui`.
- The terminal interface runs on Sigil's terminal loop. Sigil restores the terminal on exit, panic, and SIGTERM, SIGHUP, or SIGINT.
- The `/` popup and `/help` list 30 commands, grouped by job. Old names still work for one release; see Renamed commands.
- `ctrl+d` only quits. `alt+d` deletes a session in the sessions picker, signs out in `/login`, and sets the default in `/model`. `alt+p` pins a model. `F5` is the only picker refresh; `ctrl+r` stays prompt history.
- `↑` on an empty draft pulls queued input back; `ctrl+q` no longer does.
- A key stored with `summonr login` wins over peer keys. An endpoint with no key fails with the missing-key message.
- The model sees five core tools: `read`, `edit`, `write`, `run_command`, and `todo`. Modules add `skill`, `lsp`, `web`, `task`, and `bus`. `read` search takes `literal`, `caseSensitive`, and `include`; `edit` and `write` take a `files` batch.
- Chat runs at once. `shift+tab` and `/plan` enter plan mode, which is read-only and saves the plan to `plan.md` in the session directory. `/plan accept` runs the plan in the same turn.
- The `task` module is off by default.
- `--goal`, `--steps`, and `implement` children edit the launch directory. `--worktree` isolates the session.
- Sandboxed network is on by default; `--no-net` denies it.
- Anthropic: models before Claude 4.6 get a manual thinking budget, newer models get adaptive thinking. Defaults are `claude-sonnet-5` and `claude-opus-5-5`.
- Gemini tool schemas use `parametersJsonSchema`. Effort maps to `thinkingBudget` on 2.5 and `thinkingLevel` on 3.
- Dialogs size from the terminal and repaint on resize. Copy is `ctrl+b` everywhere.
- A malformed `config.toml` stops startup with one error.
- `src/limits.rs` holds every timeout. Transport retries run in leyline's `RetryPolicy`. A stream cut mid-body retries once.
- The per-turn git snapshot runs in the background. The transcript repaints only changed entries, and an idle session skips paints.
- Multi-path `read` shares one 50 KB cap. The tool schema is frozen for the session.
- Release builds use fat LTO, one codegen unit, and strip.
- Crate and command are `summonr`. `summon` and `summoner` stay PATH aliases.

### Fixed

- Anthropic tool results carry `is_error` and images. Usage counts cache reads and the final `message_delta`. Refusals show the provider text.
- Gemini keeps text-part signatures, reports blocked prompts, sends the real function name, and honors `retryDelay` on 429.
- Responses shows stream errors, streams commentary and reasoning summaries as reasoning, and keeps a `max_output_tokens` stop as a length stop. Resumed sessions keep message phase.
- Error frames and cut streams fail instead of returning a partial reply. Thinking replays only to the model that produced it.
- `--json` keeps its event order, and an abort emits Aborted then Receipt.
- Compaction and tool-argument repair are saved in the journal, and compaction resets read dedup.
- Plan mode entered mid-session turns on the command sandbox.
- Clipboard paste attaches the full image on macOS.

### Removed

- `--max-turns`. Use `--max-tokens` or `--max-secs`.
- The header fps readout and the separate reasoning band.
- AppKit from the default terminal build.

### Renamed commands

| Old | New |
| --- | --- |
| `/auth` | `/login` |
| `/quit` | `/exit` |
| `/rollback` | `/undo files` |
| `/branch` | `/fork` |
| `/tree` | `/resume` |
| `/session`, `/stats` | `/status` |
| `/profile` | `/usage session` |
| `/net` | `/permissions net` |
| `/write` | `/permissions write` |
| `/layout` | `/pane` |
| `/theme`, `/tools`, `/reasoning`, `/receipts`, `/bus`, `/modules` | `/settings` |
