# Changelog

## 0.1.0

First public release.

### Added

#### Install and endpoints

- Release binaries for macOS on Apple silicon, Linux (static musl builds), and Windows x86_64. `curl -fsSL https://summonr.dev/install.sh | sh` installs on macOS and Linux, and `irm https://summonr.dev/install.ps1 | iex` installs on Windows. Every release publishes `SHA256SUMS` and build attestations (`gh attestation verify <file> --repo manaforged/summonr`).
- 41 built-in endpoints, from Anthropic and OpenAI to coding plans and local servers. Each is one `[[preset]]` row in `src/endpoint/presets.toml`. Built-ins include the Qwen, MiniMax, Tencent, and Volcengine coding plans, the DashScope hosts, `nvidia`, `huggingface`, `cohere`, `siliconflow`, `ollama-cloud`, and `lmstudio`.
- Providers with several regions or plans (`zai`, `alibaba`, `kimi`, `minimax`, `siliconflow`, `anthropic`, `opencode`) list once. Each region and plan is a route; `summonr NAME` takes the provider or a route name such as `zai-cn`.
- `summonr login NAME --url URL [--model M]` adds your own OpenAI-compatible server, and `summonr logout NAME --remove` deletes it. `summonr login NAME --default` makes an endpoint the default.
- `summonr login openai` offers Sign in with ChatGPT, and `summonr login grok` offers a SuperGrok or X Premium+ sign-in, next to an API key. Sign-ins are stored apart from keys.
- The `claude` endpoint runs a Claude Pro or Max plan through your own Claude Code (`claude auth login`). summonr never reads the token, Claude Code never sees an `ANTHROPIC_API_KEY` from your environment, and summonr's tools, policy, and checks apply as on any other endpoint.
- Default models follow each vendor's current list, and each vendor gets the reasoning, caching, and tool settings its docs name, such as `apply_patch` edits for GPT-5, GPT-6, and Codex models.

#### The terminal interface

- `/model`, `/login`, and `/usage` are full-screen pages that share a tab row. `/model` leaves out providers that do not answer and shows each model's context, price, release date, and abilities. `/login` groups providers as ready, needs attention, and not set up, and shows a provider's region and plan as buttons.
- Several sessions stay live in one terminal or window: `/new` starts one beside the current session, and `/resume` switches.
- Tool calls take one line each and fold into groups, an edit shows its diff under the call, and the diff pane reviews one hunk at a time (`a` accept, `r` revert, `n` note).
- `shift+tab` cycles reasoning effort, `ctrl+n` folds thinking and tool output, and the footer shows context, cache, and spend.
- Three themes, `space`, `nord`, and `dawn`, with `theme = "auto"` to follow the terminal background.
- The start screen draws a drifting starfield over a galaxy band that changes every launch.
- On macOS, key hints show `⌥` for alt.
- `enter` during a turn sends the message into the running turn.
- Compaction clears old tool output first and asks for a summary only when that is not enough.

#### Goals, checks, and automation

- `--goal` runs until the check passes and a second model call agrees the goal is met. With no check configured, summonr uses one it detects (`cargo test`, `go test ./...`, or `npm test`).
- `--steps PLAN.md` runs a numbered plan with a check per step; a step that says ``Run `CMD` `` gets `CMD` as its check.
- `summonr diff --turn N|last` shows what one turn changed.
- `summonr cron` runs a prompt or a goal on a schedule, and `summonr cron install` registers it with launchd, systemd, or Task Scheduler. `summonr gateway webhook` starts a job from `POST /run/NAME`, and `summonr gateway telegram` drives sessions from a paired Telegram chat.
- `--json` streams one event per line, and the `aborted` event names its cause.
- In `--goal` and `--steps`, the check decides the exit code.
- Commands run without a prompt. Only irreversible destruction, such as wiping the home or project folder, `mkfs`, or `terraform destroy`, asks first; `confirm = "risky"` in `policy.toml` brings back prompts for `sudo`, force pushes, and protected-path writes.

#### Extensions

- MCP 2025-11-25 servers, with prompts, resources, forms, and progress. A server starts on its first call and stops when idle, and `/mcp` and `summonr mcp list|get|add|remove` manage servers.
- Plugins in the Claude Code layout: `summonr install PATH` adds skills, slash commands, agents, MCP servers, language servers, hooks, and `bin/`.
- Hooks for `pre_tool`, `post_tool`, `compact`, and `stop`, including Claude Code hooks with `dialect = "claude"`.
- Memory between sessions in `~/.summonr/memory/`, shown by `/memory`.
- The `browser` module (off by default) drives a real Chrome profile, and web search falls back across Bing, DuckDuckGo, Yahoo, and Brave.
- The model can call `ask` to put up to four questions to you in a picker.
- The `search` tool searches the workspace, and it answers common `grep -r`, `rg`, and `find -name` commands.

### Security

- Secret values from `.env` files, cloud credential files, and private keys show as `[secret]` in every tool result.
- Under `--strict`, writes to `~/.ssh`, `~/.gnupg`, and `~/.summonr` are refused in any path form, such as a Windows verbatim path or a symlinked home. Without it, summonr runs as your account with no sandbox.
- The `edit` and `write` tools never change `.summonr/check.toml` or `.summonr/policy.toml`.
- `~/.summonr` is created with mode `0700`, and `~/.summonr/credentials.toml` with mode `0600`.
- A plan sign-in must return the nonce summonr sent, and a workspace `SYSTEM.md` loads only when the workspace is trusted.
