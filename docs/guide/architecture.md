# Architecture

summonr is a terminal coding agent. `summonr NAME` selects a configured endpoint; `summonr` uses your default. The connector owns HTTP and SSE (`src/provider.rs`) and credentials (`src/credentials.rs`, `src/endpoint/auth.rs`, and `crates/auth`). Endpoints on the Completions wire (including `opencode-zen`, `go`, and `opencode`) speak OpenAI `chat/completions`. Anthropic Messages and Gemini use their own wires in `src/provider/wire/anthropic.rs` and `src/provider/wire/gemini.rs`.

Reasoning text uses `reasoning_content`. Provider continuation data stays on the tool call in the session log. Anthropic tool turns preserve their ordered content blocks, including signed thinking and redacted thinking. Gemini tool calls retain their thought signatures. Native continuation data is reused only for the matching wire and model and is omitted from Completions requests. User images are sent through all three wires.

Work is how summonr holds a coding job so the model does not have to (`src/durable/work.rs`).

## What Work is

Work is one coding job summonr holds so the model does not have to (`src/durable/work.rs`):

- an objective
- a policy: one turn, numbered steps, or a standing goal
- a stage: Plan, Execute, Verify, or Review
- steps, their checks, and (when verifying) a candidate capture of the workspace

`/steps` and `--steps` select the steps and set `RunMode::Work`. When Work is terminal, the interface returns to Auto. When a goal ends, the session returns to the mode it had before. Goal (`/goal`, `--goal`) is the loop on one objective.

`summonr --steps` and `summonr --goal` start Work from the CLI. A plain `summonr` turn still opens an internal Work record if the session has none (`src/runtime/turn.rs`). That is Auto, not the Work UI mode.

The model never owns this. It sees one step, or one objective, and the tools. summonr re-reads the rest.

## Tools

The advertised schema is `crates/types/src/tools.toml`; `src/tools/catalog.rs` fills runtime limits and applies policy and module gates. Dispatch still accepts the old names.

| Tool | Job |
| --- | --- |
| `read` | File as tagged lines, directory, or image |
| `search` | Contents (`pattern`) or file names (`glob`) under any readable path |
| `edit` | Hash-anchored replace, insert, delete, or a batch |
| `write` | Create or overwrite |
| `run_command` | Shell |
| `todo` | The itemized step list summonr tracks, in steps and goal runs |

On when the module is on: `skill`, `lsp`, `web` (the `search` module, off by default), `task` (interactive sessions only), `ask` (interactive sessions only, not in children), and the mail tools `agents`, `send`, `inbox`, and `board`. `task` is on by default and `mail` is off by default; `/settings` switches either.

## What summonr holds between turns

summonr keeps the plan between turns. Each turn is "do this item" plus whatever check you put in the plan as ``Run `command` ``.

summonr does not ask the model whether the job is done. The gate is `.summonr/check.toml`: each command exits 0 or the turn fails. In goal mode a judge completion only answers continue or stop, and only after the gates are already green (`src/goal.rs`).

Crash the process. Run the same command. Resume reads the session log back into the conversation and `work.json` back into Work, then aborts the turn the dead process left open (`src/durable/driver.rs`). Resume checks the saved plan and workspace against the requested job. If the gate list changes during a turn, summonr selects the current checks before verification.

## Where it lives in the tree

| Piece | File |
|---|---|
| Work, Plan, stages, `work.json` | `src/durable/work.rs` |
| Session driver: the one seam `Loop` talks to | `src/durable/driver.rs` |
| Conversation log, `session.jsonl` | `src/durable/log.rs` |
| Changed files (Git listing or directory walk, content fingerprints) | `src/durable/tree.rs` |
| Conversation that writes the log | `src/runtime/turn.rs` |
| Numbered plan | `src/steps.rs` |
| RunMode Plan → Work | `src/runtime.rs` `next`, `Loop::set_mode` |
| Standing goal + judge | `src/goal.rs` |
| Check commands | `src/verify.rs` |
| Tool identity and schema templates | `crates/types/src/catalog.rs`, `crates/types/src/tools.toml` |
| Runtime tool availability and schema limits | `src/tools/catalog.rs` |
| argv (`summonr grok`, `--steps`, `--goal`) | `src/main/cli.rs` |

A session is `~/.summonr/sessions/<id>/session.jsonl` (append-only messages) plus `work.json` (the job, rewritten atomically after every transition). The changed-file manifest reads Git without writing it. Outside Git, it walks the directory. Both paths fingerprint file content, permissions, and symlink targets in addition to recording size and modification time.

On disk the names are summonr: `~/.summonr/sessions/`, `.summonr/jobs/`, `SUMMONR_*`.

## Layers

summonr's modules sit in seven layers. A module imports only modules in its own layer or a lower one, and imports inside a layer never form a cycle. [`layers.json`](../../layers.json) is the map.

| Layer | Holds | Modules |
|---|---|---|
| core | helpers with no summonr types. The `summonr-core` crate in `crates/core` holds errors, text, shell syntax, file walks and the summonr home paths (`dirs`); terminal width formatting stays in `display` | `display`, `language`, `hashline`, `herdr` |
| types | the shared vocabulary: conversation messages, tool identity and output, settings types; Cargo enforces its core-only dependency | `summonr-types` in `crates/types` |
| settings | reading and merging configuration | `config`, `prefs`, `modules`, `workflow`, `limits`, `policy` |
| platform | processes, secrets, credentials, session storage, RPC | `process`, `supervise`, `secret`, `askpass`, `credentials`, `durable`, `rpc` |
| services | providers, endpoints, diffs, worktrees, MCP, plugins, skills, mail, web | the other domain modules |
| orchestration | the tool runner, the turn loop, goals, scheduling, the gateway | `tools`, `runtime`, `goal`, `review`, `scout`, `cron`, `gateway`, `acp`, `update`, `splash` |
| surface | the terminal UI and the CLI entry point | `tui`, `main`, `lib` |

The `debt` list in `layers.json` names the imports that break this order today. It only shrinks: remove an entry in the change that removes its import. The largest groups and their fixes:

| Debt | Fix |
|---|---|
| `process`, `rpc`, `provider`, `steps` import `tools::command::ProcessGroup` and the command exit helpers | move them into `process` |
| `durable` and others import provider behavior and tools through application exports | shared messages, events, work progress, and receipts now live in `summonr-types`; move the remaining behavior to its owner |
| `verify`, `session`, `diff`, `mcp` still import tool identity and output through `tools` | use their owner, `summonr-types`, before extracting these crates |
| `mcp`, `session` import `tui::tool_arg`; `provider` imports `splash::host_of`; `text`, `walk` import `policy` helpers | move each helper to the lowest layer that uses it |

## Crates

summonr is moving from one crate to a flat workspace: every crate sits in `crates/`, holds one job, and depends only on crates above it in this list. Cargo enforces the order, so `layers.json` goes away when the last crate lands.

| Crate | Job | Modules today | State |
|---|---|---|---|
| `summonr-core` | errors, text, shell syntax, file walks, home paths | `error`, `text`, `shell`, `walk`, `dirs` | landed |
| `summonr-types` | messages, tool identity and schemas, events, work progress, receipts, forms, settings types | `crates/types`; depends only on core | landed |
| `summonr-store` | atomic files and stamps | `durable::store` | planned |
| `summonr-config` | reading and merging configuration, limits, policy | `config`, `prefs`, `modules`, `workflow`, `limits`, `policy` | planned |
| `summonr-platform` | processes, secrets, credentials, RPC | `process`, `supervise`, `secret`, `askpass`, `credentials`, `rpc` | planned |
| `summonr-provider` | model wires, endpoints, the catalog | `provider`, `endpoint`, `catalog`, `classify`, `recent` | planned |
| `summonr-workspace` | diffs, worktrees, checks, checkpoints | `diff`, `hunks`, `worktree`, `verify`, `steps`, `checkpoint` | planned |
| `summonr-extensions` | MCP, plugins, skills, hooks, memory, prompts, mail | `mcp`, `plugins`, `skills`, `hooks`, `memory`, `prompt`, `mail` | planned |
| `summonr-tools` | the tool runner and its tools | `tools`, `browser`, `web`, `jobs`, `command`, `child` | planned |
| `summonr-session` | session state and history | `durable` (driver, log, work, tree), `session` | planned |
| `summonr-runtime` | the turn loop and goals | `runtime`, `goal`, `review`, `scout` | planned |
| `summonr-gateway` | scheduling, chat gateways, ACP | `gateway`, `cron`, `acp` | planned |
| `summonr-tui` | the terminal and desktop UI | `tui`, `splash`, `display`, `herdr`, `language`, `hashline` | planned |
| `summonr` | the binary and CLI | `main`, `lib`, `update` | planned |

Before a crate can move, its upward imports must go. Measured against this map, these remain after the shared vocabulary moves into `summonr-types`:

| Import | Fix |
|---|---|
| `browser`, `child`, `memory`, `checkpoint`, `diff`, `worktree` use `durable` beyond its store | take what they need through `summonr-types` or a trait the session crate implements |
| `config`, `diff`, `mail`, `tools`, `worktree` use `session` | same |
| `durable::store` uses `config` and `limits` | pass the limits it reads as arguments |
| `credentials` uses `endpoint` and `provider` | move the endpoint key lookup into `summonr-provider` |
| `mail`, `mcp`, `worktree` use `tools` | move the shared tool types into `summonr-types` |
| `tools` uses `cron` | move the schedule tool next to `cron` |

## Worktrees

`--goal` and `--steps` edit the directory you launched from. Pass `--worktree` to call `git worktree add` (`src/worktree.rs`). Branch `summonr/<id>`, directory `<repo>-worktrees/<id>`. On exit 0 the harness `merge --ff-only` into the branch you started on, then `worktree remove`. Dirty trees and non-fast-forward merges stay. `summonr worktrees prune` only drops records for directories that are already gone. `--here` is the default. Interactive sessions stay in cwd unless you pass `--worktree`.

Each `implement` child gets a child checkout in `<repo>-children/<id>`. The checkout shares the parent's `target/`, `node_modules`, and any path listed in `.summonr/linked` or `.agents/linked`. The parent applies each finished child's edits on its own. Generated files (`__pycache__`, `*.pyc`, and the pytest, mypy, and ruff caches) never land.

Children started together do not see each other's edits. When two children change the same lines, that child's checkout stays on disk and the others still land. A research child stays on the parent workspace. Each child uses the parent's file, shell, and language-server tools; background commands stay with the parent. The parent runs verification after they finish. Pass `--worktree` on the session to isolate the whole run in a git worktree.

A background child (`task` with `background`) runs as a Tokio task while the parent keeps working. `Loop::spawned` holds its handle; child events arrive through `Loop::mailbox` and drain before every round, on every poll, and on the interface tick. `task` with `child` polls or kills it.

`await_children` runs before Verify: it settles finished children, waits up to the job window, and defers the turn if any still run. There is no deferral cap; the model kills the child, or a user budget ends the run. Deferred background commands are recorded on Work by job id, so the next turn waits for the same jobs before it can verify.

## Language modules

`src/language.rs` contains the four compiled-in language modules: Rust, TypeScript/JavaScript, Python, and Go. A module owns project markers, source extensions, server candidates, and LSP initialization options. `src/tools/lsp.rs` uses the requested file to select a module and keeps one server connection for each active module. The advertised tool is `lsp`.

The Rust module disables rust-analyzer check-on-save, build scripts, proc macros, and `cargo --all-targets`. The model must use `run_command` for an explicit Cargo check or test. A workspace may choose a compiled-in module, but it cannot provide an executable command. Only the user config at `~/.summonr/config.toml` may set `[lsp] command`.

## What this is not

- Not a new harness. Disk and env say summonr (`~/.summonr`, `SUMMONR_*`).
- Not crates.io. Sigil is a git dependency.
