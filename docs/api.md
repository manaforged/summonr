# summonr API map

For agents and tools that run summonr as a child process. The commands, flags, exit codes, and JSON shapes on this page are the supported surface. The Rust items in the `summonr` library crate are public to the binary only. Do not link against them. The subcommand `askpass` and the flags `--sandbox-exec` and `--profile-tui` are internal entry points. Do not call them.

Binary: `summonr`. Source of truth: `src/main/cli.rs`.

## Core commands

The first argument can be an endpoint name: `summonr grok` is `summonr -e grok`.

| Command | Job |
| --- | --- |
| `summonr [ENDPOINT]` | Start an interactive session |
| `summonr [ENDPOINT] -p PROMPT`, `summonr [ENDPOINT] PROMPT` | Run one prompt and exit |
| `summonr [ENDPOINT] --goal TEXT` | Work until `.summonr/check.toml` passes and a judge call agrees |
| `summonr [ENDPOINT] --steps PLAN.md` | Run each numbered step under `## Steps`, and gate each step on its check |
| `summonr -c`, `summonr --session ID` | Resume the last session here, or a session by short id, id prefix, or name |
| `summonr sessions` | List sessions, tab-separated: id, name, directory, preview |
| `summonr export [ID\|NAME]` | Print a session as Markdown |
| `summonr profile [ID\|NAME]` | Print a time and token report from a session journal |
| `summonr diff [PATH] [--turn N\|last] [--stat\|--json]` | Print the patch against the worktree base |
| `summonr worktrees [--json]`, `summonr worktrees prune [--json]` | List session worktrees, or drop records of missing directories and print the removed paths |
| `summonr endpoints` | Probe every endpoint and show its key status |
| `summonr login [NAME] [--key]`, `summonr logout NAME` | List, store, or forget keys and subscription sign-ins |
| `summonr login NAME --url URL [--model M]`, `summonr logout NAME --remove` | Add or update your own OpenAI-compatible server, or remove it |
| `summonr modules` | List compiled modules and their state |
| `summonr workflows` | List configured workflows |
| `summonr mcp list\|get NAME [--json]`, `summonr mcp add NAME -- CMD\|--url URL`, `summonr mcp remove NAME` | List MCP servers with state, tools, and schema size; add or remove a `[mcp.NAME]` table |
| `summonr install PATH`, `summonr install --list`, `summonr uninstall NAME` | Install a plugin directory, list plugins, or remove one |
| `summonr acp` | Serve the Agent Client Protocol over stdio |
| `summonr mail [agents\|send\|inbox\|board]` | Read and write peer mail between agent sessions on this machine |
| `summonr classify ...` | Ask typed questions of a decisions endpoint |
| `summonr doctor [--logs]` | List per-run JSONL logs, or report incomplete and crashed runs |
| `summonr --help`, `summonr --version` | Print help, or the version and revision |

## Quick paths

Run one prompt with machine-readable events:

```sh
summonr anthropic -p --json "why does cargo test fail?"
```

Work until the check passes, inside a budget, then branch on the exit code:

```sh
mkdir -p .summonr
echo 'check = "cargo test"' > .summonr/check.toml
summonr anthropic --goal "make cargo test pass" --max-secs 1800 --json
echo $?
```

## Jobs to commands

| Job | Command or flag |
| --- | --- |
| Pick an endpoint or model | `-e NAME` or a leading `NAME`; `--model NAME`; `--effort none\|minimal\|low\|medium\|high\|xhigh\|max` |
| Set a budget | `--max-tokens N`, `--max-secs N` |
| Contain shell commands | `--sandbox`; `--no-net` denies sockets and needs `--sandbox`; `--add-dir DIR` adds a writable root |
| Allow only listed commands | `--strict` with `policy.toml` |
| Ignore workspace `AGENTS.md` | `--no-trust` |
| Skip the check gate for one run | `--no-verify` |
| Isolate a session in a git worktree | `--worktree` (branch `summonr/<id>`); `--here` is the default |
| Name or fork a session | `--name TEXT`; `--fork` forks the last session here, or `--session ID --fork` forks that session |
| Run workflow roles on their own models | `-w NAME`, from `[workflows.NAME]` |
| Review what a turn changed | `summonr diff --turn last --json` |
| Drive a session from an editor | `summonr acp` |
| Classify text | `summonr classify --set NAME [STATE]`, `--questions JSON\|@FILE`, `--noul QUESTION`, `--patch`, `--review --session ID`, `--eval FILE`, `--gate NUMBER` |
| Save a transcript | `summonr export [ID\|NAME]`; add `--digest` for the condensed form |

## Exit codes

| Code | Meaning | Source |
| --- | --- | --- |
| 0 | Done. In goal and step modes, the work completed. | `WorkProgress::exit_code` in `src/durable/work/progress.rs` |
| 1 | Harness error: bad arguments, config error, or internal failure. The message goes to stderr as `summonr: <error>`. | `src/main.rs` |
| 2 | A check failed. In goal and step modes this includes a run that stopped short on a budget or an abort and then failed the check at exit. | `exit` in `src/main/modes.rs`, `Loop::stop_work` in `src/runtime/turn/checks.rs` |
| 3 | Stopped short: budget, stall, or aborted turn, with no check configured, or with a check that passed while the goal was not met. | `src/main/modes.rs`, `src/durable/work/progress.rs` |
| 130 | Interrupted by `SIGINT`, or Ctrl+C on Windows. | `interrupted` in `src/main.rs` |
| 143 | Terminated by `SIGTERM` (Unix). | `interrupted` in `src/main.rs` |
| 129 | Hung up by `SIGHUP` (Unix). | `interrupted` in `src/main.rs` |

In `--goal` and `--steps`, the check decides the exit code. When a run stops short, summonr runs the configured check once at exit. A failed check gives 2. A completed run gives 0. `-p` runs no check at exit.

A signal exit skips the `--worktree` finish: the worktree and its branch stay in place, with the run's files uncommitted.

An unknown option is exit code 1 with `unknown option`. `summonr mcp add` exits 1 when the new server does not answer; the entry stays in the config file, and the message names that file. On Unix, a closed stdout or stderr pipe ends summonr through `SIGPIPE` with no error message.

## Machine output

### `--json` on a session (`-p`, `--goal`, `--steps`)

summonr prints one JSON object per line on stdout. Each object has a `type` field in snake case. The first line is `{"type":"session","id":"<session id>"}`. The shapes come from `Event` in `src/tools.rs`.

| `type` | Fields |
| --- | --- |
| `session` | `id` |
| `tool_call` | `id`, `name`, `arguments` (string) |
| `tool_result` | `id`, `success`, `mutated`, `output`, optional `millis` |
| `assistant` | `text` |
| `delta` | `text` |
| `reasoning` | `text` |
| `note` | `text` |
| `usage` | `prompt_tokens`, `completion_tokens`, `reasoning_tokens`, `cached_tokens`, optional `millis`, `prefix_hash`, `cost` |
| `unreported` | `tokens` |
| `mail` | `from`, `text` |
| `aborted` | `code` (`time_budget`, `token_budget`, `interrupted`, `cancelled`, or `error`), `reason` (free text) |
| `merged` | `branch`, `base`, `files` |
| `progress` | `id` (the tool call), `progress`, optional `total`, `message` |
| `approval` | `detail`, optional `persist` |
| `elicit` | `form` (`message`, `fields`, and the MCP `server`, absent when the model asks), or null when the question ends |
| `verify` | `command`, `passed`. `{"command":"workspace changed after the check ran","passed":false}` means files changed after summonr captured the candidate it checked, so the verdict fails; it counts toward `checks_failed` |
| `receipt` | `receipt` |
| `work` | `progress`; its `checks_failed` agrees with the `verify` events |
| `child` | `id`, `event` (a nested event) |

A step that contains ``Run `CMD` `` is gated on `CMD` and on `.summonr/check.toml`. A step with no `Run` phrase is gated on `.summonr/check.toml` alone. The last step runs every check.

```markdown
## Steps

1. Add the `--limit` flag. Run `cargo test cli`.
2. Document the flag in README.md.
```

Step mode also prints `{"type":"step","index":N,"total":N,"state":"...","check":...}`. On a `start` event, `check` holds the step's `Run` commands joined with ` && `, or null when the step has none.

When a goal or steps run stops short, the `aborted` event is not the last line: summonr runs the exit check when the work is not already finished, then prints its `verify` events and a final `work` event. Read the exit code to learn the result.

### `summonr diff --json`

One object: `{"base": string, "files": [{"path", "added", "removed", "new"}]}`. With `--turn`, `base` is `"turn N"` and `session` holds the session id.

### `summonr worktrees --json`

One array. Each row has `branch`, `base`, `path`, `ahead`, `dirty`, and `session`.

### `summonr classify --json`

One pretty-printed object: `model`, `answers`, and `usage` with `input_tokens`, `output_tokens`, and `cost`. With `--patch`, the output is an array of `{"path", "answers"}` rows.

### `summonr export`

Markdown on stdout. It is not JSON.

### `summonr acp`

JSON-RPC over stdio. `initialize` answers `"protocolVersion": 1`. The methods are `session/new` (needs `cwd`) and `session/prompt`. summonr sends `session/update` notifications and accepts a `session/cancel` notification. Source: `src/acp.rs`.

## Conventions

- User config is under `~/.summonr`. Set `SUMMONR_HOME` to move it.
  - `~/.summonr/config.toml`: endpoints, `[modules]`, `[mcp.NAME]`, `[hooks]`, `[sandbox]`, `[workflows.NAME]`.
  - `~/.summonr/policy.toml`: command and write policy.
  - `~/.summonr/credentials.toml`: stored keys.
  - `~/.summonr/sessions/<id>/session.jsonl`: the session log.
  - `~/.summonr/skills/<name>/SKILL.md`, `~/.summonr/bin/<name>/`.
- Key lookup order: the endpoint's environment variable, then `credentials.toml`, then peer stores (`~/.local/share/opencode/auth.json`, `~/.kimi-code/credentials/kimi-code.json`), then `api_key` in `config.toml`, then the built-in default. An endpoint with no key fails with the missing-key message unless it is local or keyless. `SUMMONR_API_KEY` applies only with `SUMMONR_BASE_URL`.
- `.summonr/check.toml` holds `check = "CMD"` or `checks = ["CMD", ...]`. Use one key, not both. The checks decide exit code 2. The `edit` and `write` tools refuse `.summonr/check.toml` and `.summonr/policy.toml` in every mode. When the check file changes during a run anyway, summonr restores it, a `note` event says so, and the run keeps the check it loaded at start.
- A file that summonr's own stdout or stderr is redirected to, for example `--json > ci/events.jsonl` inside the repository, never counts as a change.
- A workspace `.summonr/policy.toml` can narrow the user policy. It can't widen it.
- A workspace `.summonr/config.toml` can turn a module off. It can't turn a module on. `mail`, `search`, and `browser` are off by default.
- The advertised tool list freezes after the first model round. A module row in `/settings` writes config for the next process.
- Each `[hooks]` value (`pre_tool`, `post_tool`, `compact`, `stop`) is a shell command, a table `{ command = "...", dialect = "claude" }`, or a list of either, run in order. Each hook reads JSON on stdin. A `pre_tool` hook that exits nonzero blocks the call. A hook with `dialect = "claude"`, and every plugin hook, follows the Claude Code rules instead (see the reference).
- A project `.mcp.json` loads only after approval. `import_mcp = true` also loads `~/.claude.json` and `~/.codex/config.toml`.
- The sandbox is off by default. Without `--sandbox`, an unattended `--goal` run can read and run anything your account can, including files outside the workspace; run CI and other unattended goals with `--sandbox`. In a noninteractive run, sandbox escape, network grants, and writes outside the workspace are denied; `--no-net` stays in force after a command failure. Auto mode asks before it grants an extra write root.
- `AGENTS.md`, `CLAUDE.md`, and `SYSTEM.md` at the workspace root are read only when the workspace is trusted. A home `SYSTEM.md` replaces the built-in preamble. A workspace `SYSTEM.md` is appended after it.

The [reference](guide/reference.md) has every flag and config key. [Extending summonr](guide/extending.md) covers skills, MCP servers, hooks, and CLIs.
