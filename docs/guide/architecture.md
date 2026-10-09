# Architecture

summonr is a terminal coding agent. `summonr NAME` selects a configured endpoint; `summonr` uses your default. The connector owns HTTP and SSE (`src/provider.rs`) and credentials (`src/credentials.rs`, `src/endpoint/auth.rs`, and `crates/auth`). Completions seats (including `zen`, `go`, and `opencode`) speak OpenAI `chat/completions`. Anthropic Messages and Gemini use their own wires in `src/provider/wire/anthropic.rs` and `src/provider/wire/gemini.rs`.

Reasoning text uses `reasoning_content`. Provider continuation data stays on the tool call in the session log. Anthropic tool turns preserve their ordered content blocks, including signed thinking and redacted thinking. Gemini tool calls retain their thought signatures. Native continuation data is reused only for the matching wire and model and is omitted from Completions requests. User images are sent through all three wires.

Work is how summonr holds a coding job so the model does not have to (`src/durable/work.rs`).

## What Work is

Work is one coding job summonr holds so the model does not have to (`src/durable/work.rs`):

- an objective
- a policy: one turn, numbered steps, or a standing goal
- a stage: Plan, Execute, Verify, or Review
- steps, their checks, and (when verifying) a candidate capture of the workspace

Plan and Work are one pipeline. Plan writes `.summonr/plan.md`. `/plan accept` selects that file, sets `RunMode::Work`, and starts Execute in the same turn (`src/tui/worker/command.rs`). A work request with no selected plan stays in Plan (`Loop::set_mode`). When Work is terminal, the interface returns to Auto. Goal (`/goal`, `--goal`) is the loop without a written plan. `shift+tab` switches Auto and Plan only (`RunMode::next`).

`summonr --steps` and `summonr --goal` start Work from the CLI. A plain `summonr` turn still opens an internal Work record if the session has none (`src/runtime/turn.rs`). That is Auto, not the Work UI mode.

The model never owns this. It sees one step, or one objective, and the tools. summonr re-reads the rest.

## Tools

The advertised schema is `src/tools/catalog.rs`. Dispatch still accepts the old names.

| Tool | Job |
| --- | --- |
| `read` | File as tagged lines, directory, image, or search (`pattern` / `glob`) |
| `edit` | Hash-anchored replace, insert, delete, or a batch |
| `write` | Create or overwrite |
| `run_command` | Sandboxed shell |
| `todo` | The itemized step list summonr tracks, in steps and goal runs |

On when the module is on: `skill`, `lsp`, `web` (the `search` module, off by default), `task` (interactive sessions only), and the mail tools `agents`, `send`, `inbox`, and `board`. `task` is on by default and `mail` is off by default; `/settings` switches either. Plan mode advertises `read` and `run_command` (plus `skill`, and `web` when `search` is on).

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
| Plan file `.summonr/plan.md` | `src/plan.rs` |
| RunMode Plan → Work | `src/runtime.rs` `next`, `Loop::set_mode` |
| Standing goal + judge | `src/goal.rs` |
| Check commands | `src/verify.rs` |
| Advertised tools | `src/tools/catalog.rs` |
| argv (`summonr grok`, `--steps`, `--goal`) | `src/main/cli.rs` |

A session is `~/.summonr/sessions/<id>/session.jsonl` (append-only messages) plus `work.json` (the job, rewritten atomically after every transition). The changed-file manifest reads Git without writing it. Outside Git, it walks the directory. Both paths fingerprint file content, permissions, and symlink targets in addition to recording size and modification time.

On disk the names are summonr: `~/.summonr/sessions/`, `.summonr/jobs/`, `SUMMONR_*`.

## Worktrees

`--goal` and `--steps` edit the directory you launched from. Pass `--worktree` to call `git worktree add` (`src/worktree.rs`). Branch `summonr/<id>`, directory `<repo>-worktrees/<id>`. On exit 0 the harness `merge --ff-only` into the branch you started on, then `worktree remove`. Dirty trees and non-fast-forward merges stay. `summonr worktrees prune` only drops records for directories that are already gone. `--here` is the default. Interactive sessions stay in cwd unless you pass `--worktree`.

Each `implement` child gets a delta checkout in `<repo>-deltas/<id>`. The checkout shares the parent's `target/`, `node_modules`, and any path listed in `.summonr/linked` or `.agents/linked`. The parent applies each finished child's edits on its own. Generated files (`__pycache__`, `*.pyc`, and the pytest, mypy, and ruff caches) never land. Children started together do not see each other's edits. When two children change the same lines, that child's checkout stays on disk and the others still land. A research child stays on the parent workspace. Each child uses the parent's file, shell, and language-server tools; background commands stay with the parent. The parent runs verification after they finish. Pass `--worktree` on the session to isolate the whole run in a git worktree.

A background child (`task` with `background`) runs as a Tokio task while the parent keeps working. `Loop::spawned` holds its handle; child events arrive through `Loop::mailbox` and drain before every round, on every poll, and on the interface tick. `task` with `child` polls or kills it. `await_children` runs before Verify: it settles finished children, waits up to the job window, and defers the turn if any still run. There is no deferral cap; the model kills the child, or a user budget ends the run. Deferred background commands are recorded on Work by job id, so the next turn waits for the same jobs before it can verify.

## Language modules

`src/language.rs` contains the four compiled-in language modules: Rust, TypeScript/JavaScript, Python, and Go. A module owns project markers, source extensions, server candidates, and LSP initialization options. `src/tools/lsp.rs` uses the requested file to select a module and keeps one server connection for each active module. The advertised tool is `lsp`.

The Rust module disables rust-analyzer check-on-save, build scripts, proc macros, and `cargo --all-targets`. The model must use `run_command` for an explicit Cargo check or test. A workspace may choose a compiled-in module, but it cannot provide an executable command. Only the user config at `~/.summonr/config.toml` may set `[lsp] command`.

## What this is not

- Not a new harness. Disk and env say summonr (`~/.summonr`, `SUMMONR_*`).
- Not crates.io. Sigil is a git dependency.
