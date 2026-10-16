# summonr for coding agents

Point your coding agent at this file when it sets up summonr, runs it from a script or CI, or extends it. summonr is a terminal coding agent that works until your check command passes.

## Install

```sh
curl -fsSL https://summonr.dev/install.sh | sh
```

On Windows, run `irm https://summonr.dev/install.ps1 | iex` in PowerShell. From source: `cargo install --git https://github.com/manaforged/summonr --locked`.

## Give it a key

Set the provider's environment variable and name the endpoint:

```sh
export ANTHROPIC_API_KEY=...
summonr anthropic -p "say hi"
```

With a Claude Pro or Max plan, sign in to Claude Code with `claude auth login` and use the `claude` endpoint: `summonr claude -p "say hi"`. summonr runs Claude Code for each turn and keeps its own tools, policy, and checks.

`summonr login NAME` stores a key or signs in to a coding plan. `summonr endpoints` lists every endpoint with its key status. Add any OpenAI-compatible server with `summonr login NAME --url URL [--model M]`, and remove it with `summonr logout NAME --remove`. The same server can be one `[endpoint.NAME]` table in `~/.summonr/config.toml` with `base_url`, `model`, and `api_key`.

## Drive it from a script

Read [docs/api.md](https://raw.githubusercontent.com/manaforged/summonr/main/docs/api.md) first. It lists the supported commands, flags, exit codes, and JSON event shapes. Do not link against the `summonr` Rust crate; the CLI is the interface.

One prompt, with one JSON event per line on stdout:

```sh
summonr anthropic -p --json "why does cargo test fail?"
```

Work until a check passes, inside a time budget:

```sh
mkdir -p .summonr
echo 'check = "cargo test"' > .summonr/check.toml
summonr anthropic --goal "make cargo test pass" --max-secs 1800 --json
```

| Exit code | Meaning |
| --- | --- |
| 0 | Done. In goal and step modes, the work completed. |
| 1 | Harness error: bad arguments, config, or an internal failure. |
| 2 | The check failed, including when the budget ran out. |
| 3 | Stopped short with no check configured, or the check passed but the goal was not met. |

Run on a schedule or from a web request:

```sh
summonr cron add nightly --cron "0 3 * * *" --goal "make cargo test pass"
summonr cron install          # once: the OS starts due jobs
summonr cron log nightly      # what the last run did and whether the check passed
```

`summonr cron add NAME --manual -p PROMPT` with `summonr cron token NAME` and `summonr gateway webhook` starts a job from `POST /run/NAME`. `summonr gateway telegram` drives sessions from a paired Telegram chat.

summonr runs commands as your account with no sandbox, so run unattended goals in a container when they should not touch the rest of the machine. Useful flags: `--worktree` to work in a separate git worktree (on exit 0 it commits, fast-forwards the branch you started from, and deletes its own branch; on failure it keeps the worktree), and `--steps PLAN.md` to run a numbered plan with a check per step. `summonr diff --turn last --json` shows what the last turn changed. `summonr acp` serves the Agent Client Protocol for editors.

## Give it project instructions

When the workspace is trusted, summonr reads `AGENTS.md`, `CLAUDE.md`, and `SYSTEM.md` at the workspace root. Your user-level files load in every session: `~/.agents/AGENTS.md`, `~/.claude/CLAUDE.md`, and `~/.config/devin/AGENTS.md`, from the account's home directory. A home `~/.summonr/SYSTEM.md` replaces the built-in preamble. A workspace `SYSTEM.md` is appended after it. `--no-trust` ignores the workspace files. Memory files under `~/.summonr/memory` load into trusted sessions too; `memory = false` under `[modules]` turns them off.

An implement child works in `<repo>-children/<id>`. That checkout shares the parent's `target/` and `node_modules`. The parent applies each finished child's edits on its own. When two children change the same lines, that checkout stays on disk, and summonr never deletes it. A research child stays on the parent workspace. `--worktree` still isolates the whole session in a git worktree.

## Extend it

summonr loads no third-party code into its process. Add capability from outside:

| Add | How |
| --- | --- |
| A skill | `~/.summonr/skills/NAME/SKILL.md` |
| An MCP server | `summonr mcp add NAME -- CMD`, or a `[mcp.NAME]` table in `~/.summonr/config.toml` |
| A hook | `[hooks]` in `~/.summonr/config.toml`: `pre_tool`, `post_tool`, `compact`, or `stop`, each a shell command (or a Claude Code hook with `dialect = "claude"`) that reads JSON on stdin |
| A CLI | Any program on `PATH`; the model calls it through `run_command` |
| A plugin | `summonr install PATH` for a Claude Code layout directory: skills, commands, agents, MCP servers, hooks, language servers, and `bin/` |

[Extending summonr](https://raw.githubusercontent.com/manaforged/summonr/main/docs/guide/extending.md) has the details. The [reference](https://raw.githubusercontent.com/manaforged/summonr/main/docs/guide/reference.md) lists every flag, slash command, config key, and environment variable.

## Files

| Path | Holds |
| --- | --- |
| `~/.summonr/config.toml` | Endpoints, modules, MCP servers, hooks, write roots, workflows |
| `~/.summonr/policy.toml` | Command and write policy |
| `~/.summonr/credentials.toml` | Stored keys |
| `~/.summonr/sessions/ID/` | Session logs |
| `.summonr/check.toml` | The workspace check: `check = "CMD"` or `checks = [...]` |
| `.summonr/policy.toml` | A workspace policy that can narrow the user policy, never widen it |

Set `SUMMONR_HOME` to move `~/.summonr`.
