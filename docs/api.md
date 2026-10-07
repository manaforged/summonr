# summonr API map

For agents and tools that run summonr as a child process. The commands, flags, exit codes, and JSON shapes on this page are the supported surface. The Rust items in the `summonr` library crate are public to the binary only. Do not link against them. The subcommand `askpass` and the flags `--sandbox-exec` and `--profile-tui` are internal entry points. Do not call them.

Binary: `summonr`. Source of truth: `src/main/cli.rs`.

## Core commands

The first argument can be an endpoint name: `summonr grok` is `summonr -e grok`.

| Command | Job |
| --- | --- |
| `summonr [ENDPOINT]` | Start an interactive session |
| `summonr [ENDPOINT] -p PROMPT` | Run one prompt and exit |
| `summonr [ENDPOINT] --goal TEXT` | Work until `.summonr/check.toml` passes and a judge call agrees |
| `summonr [ENDPOINT] --steps PLAN.md` | Run each numbered step under `## Steps`, and gate each step on its check |
| `summonr -c`, `summonr --session ID` | Resume the last session here, or a session by id prefix or name |
| `summonr sessions` | List sessions, tab-separated: id, name, directory, preview |
| `summonr export [ID\|NAME]` | Print a session as Markdown |
| `summonr profile [ID\|NAME]` | Print a time and token report from a session journal |
| `summonr diff [PATH] [--turn N\|last] [--stat\|--json]` | Print the patch against the worktree base |
| `summonr worktrees [--json]`, `summonr worktrees prune` | List session worktrees, or drop records of missing directories |
| `summonr endpoints` | Probe every endpoint and show its key status |
| `summonr login [NAME] [--key]`, `summonr logout NAME` | List, store, or forget keys and subscription sign-ins |
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
| Read-only run | `--plan` |
| Contain shell commands | `--sandbox`; `--no-net` denies sockets and needs `--sandbox`; `--add-dir DIR` adds a writable root |
| Allow only listed commands | `--strict` with `policy.toml` |
| Ignore workspace `AGENTS.md` | `--no-trust` |
| Skip the check gate for one run | `--no-verify` |
| Isolate a session in a git worktree | `--worktree` (branch `summonr/<id>`); `--here` is the default |
| Name or fork a session | `--name TEXT`; `--fork` |
| Run workflow roles on their own models | `-w NAME`, from `[workflows.NAME]` |
| Review what a turn changed | `summonr diff --turn last --json` |
| Drive a session from an editor | `summonr acp` |
| Classify text | `summonr classify --set NAME [STATE]`, `--questions JSON\|@FILE`, `--noul QUESTION`, `--diff`, `--review --session ID`, `--eval FILE`, `--gate NUMBER` |
| Save a transcript | `summonr export [ID\|NAME]`; add `--digest` for the condensed form |

## Exit codes

| Code | Meaning | Source |
| --- | --- | --- |
| 0 | Done. In goal and step modes, the work completed. | `WorkProgress::exit_code` in `src/durable/work.rs` |
| 1 | Harness error: bad arguments, config error, or internal failure. The message goes to stderr as `summonr: <error>`. | `src/main.rs` |
| 2 | A check failed. | `exit` in `src/main/modes.rs` |
| 3 | Stopped short: budget, stall, or aborted turn. | `src/main/modes.rs`, `src/durable/work.rs` |
| 130 | Interrupted by `SIGINT`, or Ctrl+C on Windows. | `interrupted` in `src/main.rs` |
| 143 | Terminated by `SIGTERM` (Unix). | `interrupted` in `src/main.rs` |
| 129 | Hung up by `SIGHUP` (Unix). | `interrupted` in `src/main.rs` |

A signal exit skips the `--worktree` finish: the worktree and its branch stay in place, with the run's files uncommitted.

An unknown option is exit code 1 with `unknown option`.

## Machine output

### `--json` on a session (`-p`, `--goal`, `--steps`)

summonr prints one JSON object per line on stdout. Each object has a `type` field in snake case. The shapes come from `Event` in `src/tools.rs`.

| `type` | Fields |
| --- | --- |
| `tool_call` | `id`, `name`, `arguments` (string) |
| `tool_result` | `id`, `success`, `mutated`, `output`, optional `millis` |
| `assistant` | `text` |
| `delta` | `text` |
| `reasoning` | `text` |
| `note` | `text` |
| `usage` | `prompt_tokens`, `completion_tokens`, `reasoning_tokens`, `cached_tokens`, optional `millis`, `prefix_hash`, `cost` |
| `unreported` | `tokens` |
| `mail` | `from`, `text` |
| `aborted` | `reason` |
| `merged` | `branch`, `base`, `files` |
| `progress` | `id` (the tool call), `progress`, optional `total`, `message` |
| `approval` | `detail`, optional `persist` |
| `elicit` | `form` (an MCP server's `server`, `message`, and `fields`), or null when the question ends |
| `verify` | `command`, `passed` |
| `receipt` | `receipt` |
| `work` | `progress` |
| `child` | `id`, `event` (a nested event) |

Step mode also prints `{"type":"step","index":N,"total":N,"state":"...","check":...}`.

### `summonr diff --json`

One object: `{"base": string, "files": [{"path", "added", "removed", "new"}]}`.

### `summonr worktrees --json`

One array. Each row has `branch`, `base`, `path`, `ahead`, `dirty`, and `session`.

### `summonr classify --json`

One pretty-printed object: `model`, `answers`, and `usage` with `input_tokens`, `output_tokens`, and `cost`. With `--diff`, the output is an array of `{"path", "answers"}` rows.

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
- `.summonr/check.toml` holds `check = "CMD"` or `checks = ["CMD", ...]`. Use one key, not both. The checks decide exit code 2.
- A workspace `.summonr/policy.toml` can narrow the user policy. It can't widen it.
- A workspace `.summonr/config.toml` can turn a module off. It can't turn a module on. `mail` and `search` are off by default.
- The advertised tool list freezes after the first model round. A module row in `/settings` writes config for the next process.
- Each `[hooks]` value (`pre_tool`, `post_tool`, `compact`, `stop`) is a shell command, or a list run in order, that reads JSON on stdin. A `pre_tool` hook that exits nonzero blocks the call. Plugin hooks follow the Claude Code rules instead (see the extending guide).
- A project `.mcp.json` loads only after approval. `import_mcp = true` also loads `~/.claude.json` and `~/.codex/config.toml`.
- The sandbox is off by default. `--plan` denies writes. In a noninteractive run, sandbox escape, network grants, and writes outside the workspace are denied; `--no-net` stays in force after a command failure. Auto mode asks before it grants an extra write root.
- `AGENTS.md`, `CLAUDE.md`, and `SYSTEM.md` at the workspace root are read only when the workspace is trusted. A home `SYSTEM.md` replaces the built-in preamble. A workspace `SYSTEM.md` is appended after it.

The [reference](guide/reference.md) has every flag and config key. [Extending summonr](guide/extending.md) covers skills, MCP servers, hooks, and CLIs.
