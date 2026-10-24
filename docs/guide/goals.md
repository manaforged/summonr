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
| `score` | One command that prints scores; see Score a goal |
| `targets` | A table of score names and the values they must reach |
| `spend`, `budget`, `halt` | A dollar budget for the goal; see Budget a goal |
| `protect` | Paths that stay read-only while a goal runs, such as `protect = ["tests"]`; see Run a goal |

With no check file, `/goal` in the interface detects a check from the workspace root and prints it, with how to set another. The first match in this order wins:

| Marker file | Detected check |
| --- | --- |
| `Cargo.toml` | `cargo test` |
| `package.json` with a non-empty `test` script | `npm test` |
| `go.mod` | `go test ./...` |

A Python project gets no detected check, and the judge decides.

Use one key, not both. The `edit` and `write` tools refuse `.summonr/check.toml` and `.summonr/policy.toml` in every mode. If the check file changes during a run anyway, summonr restores it, a `note` event says so, and the run keeps the check it loaded at start. `--no-verify` skips the check gate for one run.

A check must leave the workspace as it found it. A check that writes files there, such as tests that save a data file, fails with `workspace changed after the check ran`; point that output at a temporary directory.

## Score a goal

A check says pass or fail. A score says how far along you are. Add a `score` command and the values it must reach to the same file:

```toml
score = "python3 tools/board.py"

[targets]
moe_w4a16 = 2.17
attention_decode = 2.22
```

The score command prints either one JSON object of numbers, `{"moe_w4a16": 1.91}`, or one `name value` line per score. Higher is better. A target whose name the command does not print counts as unmet. `targets` without `score` is a config error; `score` without `targets` tracks the best values, and scores never block the goal.

After the check at the end of each goal turn, summonr runs the score command and appends one line to the ledger: the time in UTC, the session, the `HEAD` commit, the changed paths, and every value. Every session and agent in the checkout shares one ledger, `<git dir>/summonr/score.jsonl`. Outside a git repository it lives under `$SUMMONR_HOME/score/`.

The judge sees a scoreboard with the current value, best, target, gap, and met for each name, and the last 8 ledger entries. The next goal turn starts with the same board. A goal cannot end done while a target is unmet, the same way it cannot end done on a red check.

If the score command exits non-zero or prints no scores, the ledger line has no values and an `error`, and the board shows `score failed: <reason>` and how many measurements in a row have failed. After 3 failed measurements in a row, the goal ends with `score unavailable for 3 turns: <reason>`, even when the check is red. Fix the score command before then.

When no unmet target has improved for `score_stall_turns` scored turns (3 by default), the board says so in one line and names the entries that did not move it, so you change the mechanism rather than the parameters. That line never stops the goal.

Only a measured change after a code change counts as progress. Tests, commits, and checks the model runs itself do not, and neither does running the score again with nothing changed. Every goal turn hashes the work tree and compares the score with the last reading. At the same tree, the difference between two readings becomes that score's noise. After a change, a score has to move by more than its noise to count. A worse reading after a change is still new. The board shows how many goal turns have passed without a new reading. After 3 goal turns without one, the board tells the model to drop its current approach; the context is kept, so work that takes several turns before its first measurement is not lost. After 6, the goal ends with `no new measurement in 6 goal turns`. A score command that prints how many measurements it has seen (for example `checks 37`) makes every real measurement count, even one that sets no new best.

## Budget a goal

A goal that spends money, such as a training run or a paid benchmark, can carry a dollar budget. Put the three keys above `[targets]`, because TOML reads every key after a table header as part of that table:

```toml
score = "python3 tools/board.py"
spend = "python3 tools/spend.py"
budget = 50
halt = "sh tools/halt.sh"

[targets]
moe_w4a16 = 2.17
```

| Key | Value |
| --- | --- |
| `spend` | A command that prints the dollars spent so far, as a bare number or a `spend 12.5` line. Any baseline works. |
| `budget` | The dollars this goal may add to the `spend` reading. |
| `halt` | A command that stops every paid resource. |

`budget` needs `spend`, and `halt` needs `budget`; anything else is a config error that names the file. `spend` works without `score`, so a paid goal with only a `check` can carry a budget. `spend` without `budget` is recorded and shown, never enforced.

The budget counts from the goal's first `spend` reading. A new goal in the same session starts a new baseline and can halt again. Each goal turn runs `spend` after the score, records the reading in the ledger entry (`spend`, and `spend_start`), and the board shows `spend: $X of $B budget since the goal started`. While a turn waits on a background job or child, summonr also reads `spend` at most every `score_watch_secs` seconds (300 by default), so a long job does not hide a breach.

When the spend added since the start reaches the budget, summonr runs `halt` once per goal and records `halted` in the ledger, adds a note with the spend, the budget, and how `halt` exited, kills the session's background jobs, and ends the goal with `budget reached: $X of $B`. The exit code is 2 when the check is red or a background job it killed counts as failed, and 3 otherwise. `halt` can run again after a crash between the halt and the ledger write, so it must be safe to run twice. A `spend` command that fails never halts and never blocks the goal; the board shows `spend unavailable: <reason>`. summonr snapshots `.summonr/check.toml` when the goal starts, even when the file has no `check`, and scores against that snapshot. A run cannot raise its own budget, edit its `halt` command, or change its score command or targets.

## Run a goal

```sh
summonr --goal "make cargo test pass"
```

`--goal` keeps one objective until `.summonr/check.toml` is green and a judge says stop. Every goal turn restates the objective to the model, so a compacted context cannot lose it.

The model breaks the goal into a todo list and works on one task per turn. When a turn finishes a task, the next turn starts from a fresh context that carries over the todo list, the objective, the board, and a note for each finished task. summonr writes the note itself: the files that changed since the last finished task and whether the check passed. A finished task has to change what the check prints. If the check fails with the same output as the turn before, summonr reopens the task once, and the context stays as it was. Timings in the output do not count as a change.

A goal with a check and no score counts progress the same way: the check's output has to change after a code change. After 6 goal turns without that, the goal ends with `no new measurement in 6 goal turns`. Give the goal a `score` when its progress shows in a number rather than in the check output.

When a task cannot be done in the workspace, the model marks it `blocked` and says why. A blocked task is not a failure. When every unfinished task is blocked, the goal ends with `blocked:` and the tasks it names.

When the goal starts, summonr snapshots `.summonr/check.toml`, every workspace file the `score`, `spend`, and `halt` commands name, a workspace script a check runs directly (`./check.sh`), and every file under the `protect` paths. A check such as `sh check.sh` or `pytest tests` needs `protect = ["check.sh"]` or `protect = ["tests"]`, because a check can also name the files the task changes. Before each check, it puts back any of those files that changed or went missing and notes which ones. A run cannot pass by editing the check, the score, or the tests they run. A file added under a `protect` path is not removed.

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
