# Work

Summoner is a local coding harness. `summon NAME` selects a configured
endpoint; `summoner` uses your default. The connector owns HTTP and SSE
(`src/provider.rs`) and credentials (`src/provider/auth.rs`). It supports
OpenAI-compatible `chat/completions`, Anthropic Messages, and Gemini.
The native adapters live in `src/provider/native.rs`.

Reasoning text uses `reasoning_content`. Provider continuation data stays
on the tool call in the session log. Anthropic tool turns preserve their
ordered content blocks, including signed thinking and redacted thinking.
Gemini tool calls retain their thought signatures. Native continuation data
is reused only for the matching wire and model and is omitted from
Completions requests. User images are sent through all three wires.

Work is how that harness holds a coding job so the model does not
have to (`src/durable/work.rs`).

## What Work is

Work is one coding job:

- an objective
- a policy: one turn, numbered steps, or a standing goal
- a stage: Plan, Execute, or Verify
- steps, their checks, and (when verifying) a candidate capture of the
  workspace

`summon --steps` and `summon --goal` are how you start Work from the
CLI. A plain `summon` turn still opens Work if the session has none
(`src/runtime/turn.rs`).

The model never owns this. It sees one step, or one objective, and
the tools. Summoner re-reads the rest.

## What the harness holds between turns

Summoner keeps the plan between turns. Each turn is "do this item"
plus whatever check you put in the plan as ``Run `command` ``.

Summoner does not ask the model whether the job is done. The gate is
`.mage/check.toml`: each command exits 0 or the turn fails.
In goal mode a judge completion only answers continue or stop, and
only after the gates are already green (`src/goal.rs`).

Crash the process. Run the same command. Resume reads the session
log back into the conversation and `work.json` back into Work, then
aborts the turn the dead process left open (`src/durable/driver.rs`).
Resume checks the saved plan and workspace against the requested job.
If the gate list changes during a turn, Summoner selects the current checks
before verification.

## Where it lives in the tree

| Piece | File |
|---|---|
| Work, Plan, stages, `work.json` | `src/durable/work.rs` |
| Session driver: the one seam `Loop` talks to | `src/durable/driver.rs` |
| Conversation log, `session.jsonl` | `src/durable/log.rs` |
| Changed files (Git listing or directory walk, content fingerprints) | `src/durable/tree.rs` |
| Conversation that writes the log | `src/runtime/turn.rs` |
| Numbered plan | `src/steps.rs` |
| Standing goal + judge | `src/goal.rs` |
| Check commands | `src/verify.rs` |
| argv (`summon grok`, `--steps`, `--goal`) | `src/main/cli.rs` |

A session is `~/.mage/sessions/<id>/session.jsonl` (append-only
messages) plus `work.json` (the job, rewritten atomically after every
transition). The changed-file manifest reads Git without writing it. Outside
Git, it walks the directory. Both paths fingerprint file content, permissions,
and symlink targets in addition to recording size and modification time.

On disk you still see Mage names: `~/.mage/sessions/`, `.mage/jobs/`,
`MAGE_*`. Same harness.

## Worktrees

`--goal` and `--steps` call `git worktree add` (`src/worktree.rs`) so
a long job does not edit the tree you launched from. Branch
`summon/<id>`, directory `<repo>-worktrees/<id>`. On exit 0 the
harness `merge --ff-only` into the branch you started on, then
`worktree remove`. Dirty trees and non-fast-forward merges stay.
`summon worktrees prune` only drops records for directories that are
already gone. `--here` skips isolation. Interactive sessions stay in
cwd unless you pass `--worktree`.

Concurrent `implement` children use the same linked-worktree directory.
Each child starts from the parent's tracked `HEAD` with the parent's file,
shell, and language-server tools; background commands stay with the parent.
Summoner first applies every child patch in one child worktree. If the patches
conflict, the parent stays unchanged and the worktrees remain for inspection.
If they combine cleanly, Summoner applies one patch to the parent and removes
the child worktrees. The parent runs verification after reconciliation.

A background child (`task` with `background`) runs as a Tokio task while
the parent keeps working. `Loop::spawned` holds its handle; child events
arrive through `Loop::mailbox` and drain before every round, on every
poll, and on the interface tick. `task` with `child` polls or kills it.
`await_children` runs before Verify: it settles finished children, waits
up to the job window, and defers the turn if any still run. There is no
deferral cap; the model kills the child, or a user budget ends the run.
Deferred background commands are recorded on Work by job id, so the next
turn waits for the same jobs before it can verify.

## Language modules

`src/language.rs` contains the four compiled-in language modules: Rust,
TypeScript/JavaScript, Python, and Go. A module owns project markers, source
extensions, server candidates, and LSP initialization options. `src/tools/lsp.rs`
uses the requested file to select a module and keeps one server connection for
each active module.

The Rust module disables rust-analyzer check-on-save, build scripts, proc
macros, and `cargo --all-targets`. The model must use `run_command` for an
explicit Cargo check or test. A workspace may choose a compiled-in module, but
it cannot provide an executable command. Only the user config at
`~/.mage/config.toml` may set `[lsp] command`.

## What this is not

- Not a new harness. Disk and env still say Mage (`~/.mage`, `MAGE_*`).
- Not crates.io. Sigil is a git dependency.
