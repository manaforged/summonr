# GLM pair protocol

`board.sh` records this file's sha256 and the bench commit in `run.txt`, and a full `glm` run does not start without it, without `pairs/glm.settings`, or from a bench or summonr build with uncommitted changes. Change the file only between runs.

## Question

On the same model and the same tasks, how many input tokens and how much memory does summonr use against pi, Hermes, OpenCode 2, and OMP, and does it solve as many tasks?

## Arms

summonr, a second summonr arm as a control, pi, Hermes, OpenCode 2, and OMP, at the versions in `tools.lock` and the summonr build pinned by sha256 in `run.txt` and copied into the job directory. Every arm runs GLM 5.3 Flash through the z.ai GLM Coding Plan at effort `high`, the level every bench pair uses: summonr `--effort high`, pi and OMP `--thinking high`, OpenCode 2 the `#high` variant, and Hermes `agent.reasoning_effort: high` in the config Harbor writes for it. That config also sets `terminal.timeout: 180` (Hermes's own default) and turns memory and checkpoints off; `hermes_agent` drops Harbor's `delegation.max_iterations: 50` so Hermes's own default of 250 applies, and Hermes has no turn cap.

## Tasks and tries

All 89 Terminal-Bench 2.1 tasks, one try each, a 6 h agent time limit per trial (`override_timeout_sec: 21600`, the limit z.ai's model card used for its Terminal-Bench 2.1 score), internet on, one trial per arm at a time, all arms in one Harbor job. Four earlier starts are archived and not scored: one (168 trials) ran at effort `high` under the tasks' own 15 to 60 minute limits and was stopped because timeouts were most of every arm's failures, with single replies of 10 minutes or more in every arm; a five-try run (14 trials) stopped to move to one try, and runs at three and two trials per arm stopped within ten minutes when z.ai answered with 429s and trials failed with `ApiRateLimitError`. One trial per arm drew no 429 in the first 80 minutes of the five-try run. The 10 tasks in `tasks-tune.txt` and the 25 in `tasks-dev.txt` were visible while summonr was developed; the 54 in `tasks-eval.txt` were not. The primary analysis uses the 54; the 89 are reported beside them.

## Calibration

Before the full run, `pairs/glm-cal.yaml` runs the model card's own setup beside summonr and pi on 12 tasks drawn with `random.Random(20261016).sample` from the sorted 89: compile-compcert, custom-memory-heap-crash, extract-elf, install-windows-3.11, multi-source-data-merger, password-recovery, pytorch-model-cli, raman-fitting, regex-chess, sam-cell-seg, tune-mjcf, winning-avg-corewars. The reference arm is Claude Code 2.1.207 (`claude_agent`) on z.ai's Anthropic endpoint through the same proxy, with `CLAUDE_CODE_MAX_OUTPUT_TOKENS=65536`, Claude Code's own sampling and effort, and the 6 h limit. z.ai reports 84.3 for that setup. The full run starts only if the reference solves at least 8 of the 12: at a true rate of 0.843, 7 or fewer happens with probability 0.029, so 7 or fewer means this bench does not reproduce the setup the score came from. The calibration's solves, time to solve, and reply lengths are reported with the full run.

## Environment

Every arm starts from the same container: `bench/level.py` records the image PATH, installs one base package set (from the image's own pinned `snapshot.debian.org` sources when its mirror no longer serves the versions its index lists, as on the two Debian 11 task images), and wraps each harness so the shells its model opens see only the image PATH. Each arm writes `agent/parity.txt` after install, and `table.py` refuses a job in which any two tries of a task saw different environments or an agent ran without a snapshot. Before a harness installs, `level.py` saves the home's shell startup files, and after the install it restores them and deletes any an installer created, so a harness that builds its shell from a login-shell snapshot (OMP, Hermes) inherits no installer's PATH edits; on preflight 1015c OMP's shell otherwise reached the `bun` its installer put in `~/.bashrc`. Harnesses still put their own directories on the model's PATH (pi `~/.pi/agent/bin`, Hermes `~/.local/bin`); the preflight's shell report lists what each one reaches. The preflight also runs `probe/env-probe`, a task that has each harness's own shell report every runtime it reaches, with versions, from a command that writes outside the working directory; the preflight fails when any arm's shell reaches a runtime another arm's does not, or when an arm cannot run it. Arms do not run in lockstep: each has its own slot and works on its own task at any moment, so the table prints each arm's active window and its 429s.

## Measurement

Every arm talks to the provider through `bench/proxy.py`, labelled per trial, which counts tokens and records the effort, `max_tokens`, and temperature each request sends. Token, request, and model figures come only from the proxy; a proxy label that matches no trial refuses the run. Each harness's own count is printed beside the proxy's as a cross-check, with each arm's replies that stopped at a length limit. pi, OMP, Hermes, and OpenCode 2 choose their z.ai request shape from the provider id, not the base URL, so routing through the proxy leaves their requests as they would be against z.ai; the preflight's settings lines show what each one sends. The proxy asks for usage on streams that did not ask for it, so those arms receive one extra usage chunk. summonr and its control share one key and one system-prompt prefix, so each can warm the provider cache for the other: input-token counts do not depend on the cache, while uncached input and cost do. Memory is sampled every 0.5 s by the `rss` plugin; the harness scope is the harness process and every helper it started without a shell.

## Primary claim

One joint claim, made only if it holds against every rival arm on the 54 held-out tasks, with paired ratios as geometric means over tasks and 95% bootstrap intervals over tasks (`table.py RUN --subject summonr --tasks tasks-eval.txt`):

- Input tokens per solved try: over tasks both arms solved at least once, the upper bound of the summonr / other ratio is below 1.
- Mean harness RSS: the upper bound of the summonr / other ratio is below 1.
- Solve rate: the lower bound of summonr minus the other arm's paired solve difference is above -0.10.

Each rival is a separate test at the 95% level, and the claim needs all of them, so no further correction applies. Every other figure in the table is descriptive and is reported whichever way it points. The -0.10 margin is fixed here before any one-try result. Drawing one try per task from the ChatGPT pair (`tb21-chatgpt-1013t`, summonr against Codex), the 95% paired solve-difference interval was 0.124 wide over 89 tasks and 0.151 over the 53 held-out tasks both arms ran, so with one try the margin sits about one and a half half-widths below no difference. One try per task cannot show a smaller solve gap.

## Validity checks

The run counts only when all of these hold:

- The parity check finds no difference between tries of a task, and every agent that ran wrote a snapshot.
- Every tool-carrying request an arm sent matches a line listed for it in `pairs/glm.settings`; requests without tools (a harness's own title or summary calls) are listed with their settings in the proxy table and are not pinned. The preflight (`BENCH_PREFLIGHT=1 ./board.sh RUN glm`) writes the file from the tool requests each arm sent, and it is committed before the full run.
- A preflight passed on the bench commit and summonr build the full run uses, with `pairs/glm.settings` already committed: every arm finished a trial, reached the proxy with a successful request, wrote an identical snapshot, and reported the same shell runtimes. The first preflight writes the settings file; a second one on the commit that holds it checks every request against it.
- Every task ran from one task checksum.
- The summonr control against summonr holds over all 89 tasks (`table.py RUN --subject summonr --control summonr-control`): the 99% intervals for the paired solve difference, input tokens per solved try, and mean harness RSS each contain no difference. Three checks at 99% keep the chance that noise alone fails a clean run near 3%; the table prints each interval's width. No claim smaller than the control's interval is made.

## Reruns

A trial reruns, for every arm alike, only when it raised `AgentSetupTimeoutError`; raised `ApiRateLimitError` and the proxy log shows a 429 for that trial; raised `NetworkConnectionError` and the proxy log shows a failed upstream request for that trial; or ended with an exception and no request in the proxy log. Harbor names these errors by matching text anywhere in the agent's output, including tool output, so a 429 from a task's own download does not count. `rerun.py` applies that rule, and `BENCH_RERUN=1 ./board.sh RUN glm` moves exactly those trials out of the job, records each in `run.txt`, and resumes. A trial left unfinished by an interrupted run is recorded as dropped and run again. Harbor's in-run retries stay off. Task timeouts and wrong answers stand. The table lists every rerun trial with its exception.

## Scoring

A solve voided by the dataset-source scan or by the judge (`harbor analyze -r judge.toml`: Claude Code on `claude-haiku-4-5`, criteria in `judge.toml`, the same for every arm) scores 0, and every task counts. The report lists every void with the trial, the matched text or the judge criterion it failed. The source scan skips SQLite databases, where OpenCode 2 keeps a provider catalog whose text would otherwise flag every one of its trials. The judge runs on every passing trial of every arm; the table names any it has not seen. The report gives solve rates with and without voids.

## Sensitivity

Fixed now and reported whichever way they point: the primary figures on all 89 tasks beside the 54; solve rates with judge and source voids counted as solves beside them scored 0; and uncached input tokens beside raw input tokens.

## During the run

Monitoring shows only trials finished, exceptions, 429s, and cut streams per arm. Nobody reads solve rates, tokens, or memory per arm until the run ends and every validity check has passed, and no run stops or restarts for a reason that concerns one arm.

## Report

Per arm, on the 54 and on the 89: solved (voids at 0 and counted), pass@1, pass@k, and pass^k with task-bootstrap intervals; input, uncached input, and output tokens; cost at list price; time; mean and peak memory in every scope; the proxy cross-check; the settings each arm sent; the A/A result; and the rerun list. These scores compare arms on identical containers; they are not leaderboard numbers.
