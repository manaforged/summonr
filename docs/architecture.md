# Work

This is the Mage harness. Summoner is the product name. `summon` is
the command — `summon MODEL` starts it with a specific model, and a
bare `summoner` uses your default. The connector owns HTTP and SSE (`src/provider.rs`) and
credentials (`src/provider/auth.rs`: API key or OAuth token). CoT is
one field (`reasoning_content`): open-weight `<think>` tags hoist
into it; frontier hosts that never send it omit it on the next POST.
The provider is any HTTP `chat/completions` host.

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

## Why that helps an agent run

A model given a twelve-step plan in one prompt stops looking at step
7. Summoner keeps the plan. Each turn is "do this item" plus whatever
check you put in the plan as ``Run `command` ``.

A model asked "are we done?" will say yes. Summoner does not ask it
that for the gate. `.mage/check.toml` is a process. Exit 0 or not.
In goal mode a judge completion only answers continue or stop, and
only after the gates are already green (`src/goal.rs`).

Crash the process. Run the same command. Resume reads the session
log back into the conversation and `work.json` back into Work, then
aborts the turn the dead process left open (`src/durable/driver.rs`).
If the plan text, workspace, or gate list changed, Summoner refuses
to guess.

That is the agentic loop this repo is built around: you name the job,
the harness keeps it, the model edits, commands prove it, two files
are the resume program.

## Where it lives in the tree

| Piece | File |
|---|---|
| Work, Plan, stages, `work.json` | `src/durable/work.rs` |
| Session driver: the one seam `Loop` talks to | `src/durable/driver.rs` |
| Conversation log, `session.jsonl` | `src/durable/log.rs` |
| Changed files (git read-only, `(size, mtime)` fallback) | `src/durable/tree.rs` |
| Conversation that writes the log | `src/runtime/turn.rs` |
| Numbered plan | `src/steps.rs` |
| Standing goal + judge | `src/goal.rs` |
| Check commands | `src/verify.rs` |
| argv (`summon grok`, `--steps`, `--goal`) | `src/main/cli.rs` |

A session is `~/.mage/sessions/<id>/session.jsonl` (append-only
messages) plus `work.json` (the job, rewritten atomically after every
transition). Git is read for the changed-file manifest and never
written; a non-git workspace gets a `(size, mtime)` walk instead.

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
Each child starts from the parent's tracked `HEAD` and has file tools only.
Summoner first applies every child patch in one child worktree. If the patches
conflict, the parent stays unchanged and the worktrees remain for inspection.
If they combine cleanly, Summoner applies one patch to the parent and removes
the child worktrees. The parent runs verification after reconciliation.

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
- Not crates.io. Sigil is a git dependency. Version is `0.1.0` alpha.
