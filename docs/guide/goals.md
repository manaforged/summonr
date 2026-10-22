# Goals and checks

This page shows you how to make summonr work until your check command passes, run a numbered plan, and read what a run changed.

## Set a check

A check is the command that says the work is done. Put it in `.summonr/check.toml` at the workspace root:

```sh
mkdir -p .summonr
echo 'check = "cargo test"' > .summonr/check.toml
```

The check gates `--goal` and `--steps` runs, `/goal` and `/steps` in the interface, and the check at exit. A plain prompt in auto mode does not run it, and its receipt says `no check`.

| Key | Value |
| --- | --- |
| `check` | One command: `check = "CMD"` |
| `checks` | Several commands, all of which must pass: `checks = ["CMD", ...]` |

With no check file, `/goal` in the interface detects a check from the workspace root and prints it, with how to set another. The first match in this order wins:

| Marker file | Detected check |
| --- | --- |
| `Cargo.toml` | `cargo test` |
| `package.json` with a non-empty `test` script | `npm test` |
| `go.mod` | `go test ./...` |

A Python project gets no detected check, and the judge decides.

Use one key, not both. The `edit` and `write` tools refuse `.summonr/check.toml` and `.summonr/policy.toml` in every mode. If the check file changes during a run anyway, summonr restores it, a `note` event says so, and the run keeps the check it loaded at start. `--no-verify` skips the check gate for one run.

A check must leave the workspace as it found it. A check that writes files there, such as tests that save a data file, fails with `workspace changed after the check ran`; point that output at a temporary directory.

## Run a goal

```sh
summonr --goal "make cargo test pass"
```

`--goal` keeps one objective until `.summonr/check.toml` is green and a judge says stop. Every goal turn restates the objective to the model, so a compacted context cannot lose it.

| Judge verdict | Effect |
| --- | --- |
| `done` | The goal ends. |
| `continue` | The next turn starts. |
| `stalled` | The goal ends with the reason. |
| `impossible` | The goal ends with the reason. |

In the interface, `/goal TEXT` starts a goal, `/goal` alone shows it, and `/goal clear` ends it. A session reopened with an unfinished goal says so, and `/goal` with the same text continues it.

A goal has no turn cap and no failed-check attempt limit. It runs until the judge stops it, a budget ends it, you cancel it, or a harness error occurs. A precise goal ("make `cargo test` pass") ends sooner than an open one.

A turn that ends while a background command it started is still running is deferred, with no verdict and no repair strike, until the job exits or the model kills it. A later turn cannot complete while that job runs.

## Run a plan

`--steps PLAN.md` hands the model one numbered item at a time:

```sh
summonr --steps PLAN.md
```

```markdown
## Steps

1. Add the `--limit` flag. Run `cargo test cli`.
2. Document the flag in README.md.
```

A step that contains ``Run `CMD` `` gets `CMD` as its own check, in addition to `.summonr/check.toml`. A step with no `Run` phrase is gated on `.summonr/check.toml` alone. The last step runs every check. Each step gets `step_attempts` tries (default 3).

A `### Heading` under `## Steps` groups the items that follow it into a phase. Every turn carries the ledger to the model and to the transcript, with phases as Roman numerals:

| Marker | Step state |
| --- | --- |
| `[x]` | Done |
| `[/]` | This step |
| `[ ]` | Later |
| `[!]` | Blocked |

## Budgets

| Flag | Limit |
| --- | --- |
| `--max-tokens N` | Tokens for the run |
| `--max-secs N` | Seconds the run spends working on turns |

Both flags bound every mode. Time spent waiting for your next prompt does not count against `--max-secs`, and summonr checks both limits before each model round. A budget counts from the run that loads the session, so a resumed or forked session starts with its full budget.

## Exit codes

| Code | Meaning |
| --- | --- |
| 0 | Done. In goal and step modes, the work completed. |
| 1 | Harness error: bad arguments, config error, or internal failure. The message goes to stderr as `summonr: <error>`. |
| 2 | A check failed, including a goal or step run that stopped short and then failed the check at exit. |
| 3 | Stopped short (budget, stall, or aborted turn) with no check configured, or the check passed but the goal was not met. |
| 130 | Interrupted by `SIGINT`, or Ctrl+C on Windows. |
| 143 | Terminated by `SIGTERM` (Unix). |
| 129 | Hung up by `SIGHUP` (Unix). |

When a `--goal` or `--steps` run stops short, summonr runs the configured check once at exit, and its result decides the exit code. A failed check exits 2 even when the budget ran out. `-p` runs no check at exit.

## Isolate a run

`--worktree` runs the session in a git worktree at `<repo>-worktrees/<id>`, on the branch `summonr/<id>`. `--here` is the default. The worktree starts from your last commit, so commit `.summonr/check.toml` first; an untracked check file stays behind and the run has no check.

| Outcome | What happens |
| --- | --- |
| Exit 0 | summonr commits the files the run changed as `summonr: <objective>`, fast-forwards your branch, and removes the tree. Files over 5 MB and anything the run did not touch are left in place and named. |
| The tree cannot fast-forward or still holds uncommitted files | The tree is kept and named, and the exit code is 1. |
| A signal exit | The finish is skipped: the worktree and its branch stay in place, with the run's files uncommitted. |

## Review what a run changed

```sh
summonr diff                    # the patch against the base
summonr diff --stat
summonr diff --turn last --json
```

`summonr diff [PATH]` shows changes against the base: the worktree base with `--worktree`, `HEAD` otherwise. `--turn` takes `N`, `last`, or `session`. In the interface, the same data is the diff pane; see [Using the terminal](terminal.md).

## Script it

`-p` runs one prompt and exits. `--json` prints one JSON event per line with `-p`, `--goal`, and `--steps`:

```sh
summonr anthropic -p --json "why does cargo test fail?"
summonr anthropic --goal "make cargo test pass" --max-secs 1800 --json
echo $?
```

JSON turn receipts keep model text separate from the changed-path list and every configured completion check. `verified` on a receipt is true only when at least one check ran and every check passed; a turn with no configured check completes with `verified: false`. The [API map](../api.md) lists the event shapes.

## Workflows

A workflow runs each role on its own model. The roles are `scout`, `implement`, `review`, `judge`, and `summary`. A role the workflow does not name runs on the endpoint and model you started with.

```toml
# ~/.summonr/config.toml
workflow = "cheap-scout"      # optional default

[workflows.cheap-scout]
scout = "grok"
review = "zai"
judge = "grok"
```

`summonr -w cheap-scout`, `/workflow cheap-scout` in the interface, or the `workflow` key picks one. `summonr workflows` lists them. In the interface every card action has a verb:

| Command | Action |
| --- | --- |
| `/workflow` | Open the card |
| `/workflow NAME` | Apply a saved workflow |
| `/workflow off` | Apply none |
| `/workflow list` | List saved workflows |
| `/workflow show` | Show the live seats |
| `/workflow set ROLE ENDPOINT [MODEL]` | Put a role on an endpoint |
| `/workflow clear ROLE` | Clear a role |
| `/workflow save NAME` | Write the live seats as `[workflows.NAME]` |
| `/workflow default NAME` | Set the workflow new sessions start with |
