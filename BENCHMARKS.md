# Benchmarks

Harness-level comparison: same model, same tasks, same endpoint. The
harness is the only variable. Method follows the Scaffold Effect study:
task is the sampling unit, repeats are correlated trials, differences are
paired and bootstrapped.

## Known confounds in these runs

Both tables below are superseded pending a re-run with the current
`bench/grade.sh`. Do not quote their rows as results. These confounds
apply to the runs behind them:

- Unequal reasoning settings. In the 20 × 3 run, pi sent
  `reasoning_effort: high`, `thinking: enabled`, and `max_tokens: 384000`
  on 456 of 456 requests. Summonr sent none of the three on 419 of 419.
  The output-token and latency differences are not attributable to the
  harness alone.
- Operator rules loaded by summonr only. `grade.sh` did not isolate
  `HOME`, so summonr loaded the operator's global instruction files
  (about 4.2 KB of terse-output rules). Pi loaded neither. This lowers summonr's output tokens and raises its
  prefix size.
- Readable graders. Each run was a worktree of the summonr repo, so
  `bench/tasks*/*/verify.sh` was inside the workspace. Summonr read
  verifiers in 4 calls and pi in 3. One pi run searched the temp
  directory for other runs' verifiers and the proxy log.
- The resolution difference is one pi timeout. 60/60 against 59/60 is
  `pi-refactor-inject-1`, the run that searched for graders. It ran about
  1,150 s against a 600 s limit, because `rsswatch.sh` sent TERM with no
  KILL. Summonr also got a graceful `--max-secs` stop that pi did not
  get.
- Short sessions and parent-PID RSS only. The median session is about
  10 s and about 7 requests. RSS is the agent PID alone, without child
  processes, in `-p` print mode. The figures are a baseline footprint.
  They are not evidence for long sessions, compaction, LSP, MCP, or the
  terminal UI.

The current `grade.sh` pins effort and `max_tokens` on both arms,
isolates `HOME`, runs each task in a fresh git repo outside the checkout,
interleaves the arms, sends KILL 10 s after TERM, and samples both the
agent PID and its process tree.

## Graded suite, 16 tasks × 1 (superseded)

`bench/grade.sh`, deepseek-v4-flash through `bench/proxy.py`, detached
worktrees at HEAD, deterministic per-task verifiers, release build.

| row | pi | summonr |
| --- | ---: | ---: |
| resolution | 16/16 (100%) | 16/16 (100%) |
| requests | 84 | **83** |
| output tokens | 16,425 | **10,117** |
| uncached input | 58,202 | 60,625 |
| total input | 340,442 | 389,201 |
| cache hit rate | 82.9% | 84.4% |
| agent latency | 130s | **111s** |
| cost (deepseek list) | $0.039 | **$0.032** |
| peak RSS | 136 MB med · 139 MB max | **20 MB med · 21 MB max** |
| tool failures | unmeasured | 0/89 |

At identical resolution: fewer model calls, −38% output, −15% latency,
−18% cost, ~6.6× less memory. The remaining +4% uncached-input premium is
priced capability: user global rules files (pi loads none), schemas for
three extra tools, larger read volume per task.

## Agentic suite: 20 tasks × 3 (superseded)

`bench/tasks-hard/`: misleading-test diagnosis, multi-file bugs,
cross-module refactors, system tasks (migration, config plumbing, feature
removal), constrained fixes (no test edits, ≤6-line diff, no new imports,
API-stable). 20 tasks × 3 repeats, release build, `--max-secs` bound.

| row | pi | summonr |
| --- | ---: | ---: |
| resolution | 59/60 | **60/60** |
| requests | 456 | **419** |
| uncached input | 401,562 | **271,980** |
| total input | 3,398,298 | **2,281,964** |
| cache hit rate | 88.2% | 88.1% |
| output tokens | 120,575 | **88,932** |
| latency | 873s | **753s** |
| cost (deepseek list) | $0.283 | **$0.200** |
| mean RSS (sampled) | 127 MB | **19 MB** |
| peak RSS (sampled) | 137 MB med · 155 MB max | **20 MB med · 36 MB max** |

On tasks that need real search and multi-file work summonr now wins every
row including resolution: −32% uncached input, −33% total input, −26%
output, −14% latency, −29% cost. Memory is the moat: **6.7× less average
resident memory** (19 MB vs 127 MB, time-averaged over each session's
lifetime) and 4.3× less at the worst observed peak (36 vs 155 MB).

RSS is sampled at 4 Hz on the agent PID for the whole run
(`bench/rsswatch.sh`); "mean" is the arithmetic mean of per-session
sampled RSS, "peak" is the per-session sample maximum. Earlier `time -l`
figures (51–56 MB summonr, up to 403 MB pi) measured the launcher process
group and transient page-in, not live-session load.

Earlier runs of this suite exposed and fixed real defects:
`diag-unicode`'s ASCII expectation was contradictory (both harnesses
looped 30+ minutes on an unsolvable task; summonr's answer proved the
impossibility), `con-preserve-api` shadowed the stdlib `tokenize` module,
`sys-cleanup` had a malformed grep. Those runs motivated the guard's
same-file re-read and no-progress detection plus `TASK_TIMEOUT` bounds.

## Method used in these runs

- Tokens and latency come from the logging proxy both arms share, not
  either CLI's self-report.
- Pass/fail comes from `verify.sh` running against the worktree, not the
  agent's claims.
- RSS comes from `bench/rsswatch.sh` polling the agent PID at 4 Hz for the
  full run. Mean RSS is the per-session sample mean, peak RSS the sample
  maximum. Both arms are sampled identically. All numbers here are the
  release build. Debug binaries inflate startup RSS ~1.5× and are never
  benchmarked. RSS is the main process only: language servers, MCP
  servers, and subagents are separate processes and are not counted, and
  ~10 s median sessions mean these are largely startup footprints.
- Tasks were fixture-audited before use: every verifier fails on the
  untouched fixture and passes on a hand-solved copy. The audit caught an
  unsolvable task (contradictory tests), a task targeting untracked files
  worktrees don't contain, and an ambiguous prompt. All were fixed before the run.
- Single-token aggregates are reported alongside repeats; the observed
  run-to-run band on uncached input is roughly ±15%.

## Suite layout

`bench/tasks/<name>/`: `task.txt` (prompt), `fixture/` (the workspace,
committed as the baseline of a fresh repo), `verify.sh` (exit 0 = solved). 16 tasks: bugfix, feature,
refactor fixtures plus repo-navigation tasks. `bench/README.md` documents
the runner and the larger Harbor board.

## Known limits

- Resolution is saturated: both arms pass everything, so the discriminating
  rows are efficiency and memory. Harder tasks (multi-file bugs,
  failing-test diagnosis, longer refactors) are needed before pass-rate
  claims.
- 16 tasks, one model, one endpoint. Internal tuning evidence, not an
  external-validity claim. The Harbor board (`bench/README.md`) is the
  public-facing track.
- Cost uses deepseek list prices; uncached-input premium translates
  directly to price but cached tokens bill at ~1/50 rate.
