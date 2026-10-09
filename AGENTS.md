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

`summonr login NAME` stores a key or signs in to a coding plan. `summonr endpoints` lists every endpoint with its key status. Any OpenAI-compatible server is one `[endpoint.NAME]` table in `~/.summonr/config.toml` with `base_url`, `model`, and `api_key`.

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

Useful flags: `--plan` for a read-only run, `--sandbox` to contain shell commands, `--worktree` to work on a separate git branch, and `--steps PLAN.md` to run a numbered plan with a check per step. `summonr diff --turn last --json` shows what the last turn changed. `summonr acp` serves the Agent Client Protocol for editors.

## Give it project instructions

When the workspace is trusted, summonr reads `AGENTS.md`, `CLAUDE.md`, and `SYSTEM.md` at the workspace root. A home `~/.summonr/SYSTEM.md` replaces the built-in preamble. A workspace `SYSTEM.md` is appended after it. `--no-trust` ignores the workspace files.

An implement child works in `<repo>-deltas/<id>`. That checkout shares the parent's `target/` and `node_modules`. The parent applies each finished child's edits on its own. When two children change the same lines, that checkout stays on disk. A research child stays on the parent workspace. `--worktree` still isolates the whole session in a git worktree.

## Extend it

summonr loads no third-party code into its process. Add capability from outside:

| Add | How |
| --- | --- |
| A skill | `~/.summonr/skills/NAME/SKILL.md` |
| An MCP server | `summonr mcp add NAME -- CMD`, or a `[mcp.NAME]` table in `~/.summonr/config.toml` |
| A hook | `[hooks]` in `~/.summonr/config.toml`: `pre_tool`, `post_tool`, `compact`, or `stop`, each a shell command (or a Claude Code hook with `dialect = "claude"`) that reads JSON on stdin |
| A CLI | Any program on `PATH`; the model calls it through `run_command` |
| A plugin | `summonr install PATH` for a plugin in the Claude Code layout |

[Extending summonr](https://raw.githubusercontent.com/manaforged/summonr/main/docs/guide/extending.md) has the details. The [reference](https://raw.githubusercontent.com/manaforged/summonr/main/docs/guide/reference.md) lists every flag, slash command, config key, and environment variable.

## Files

| Path | Holds |
| --- | --- |
| `~/.summonr/config.toml` | Endpoints, modules, MCP servers, hooks, sandbox, workflows |
| `~/.summonr/policy.toml` | Command and write policy |
| `~/.summonr/credentials.toml` | Stored keys |
| `~/.summonr/sessions/ID/` | Session logs |
| `.summonr/check.toml` | The workspace check: `check = "CMD"` or `checks = [...]` |
| `.summonr/policy.toml` | A workspace policy that can narrow the user policy, never widen it |

Set `SUMMONR_HOME` to move `~/.summonr`.
