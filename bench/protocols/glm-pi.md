# GLM summonr, pi, and OpenCode 2 protocol

`board.sh` records this file's sha256 and the bench commit in `run.txt`, and a full `glm-pi` run does not start without it, without `pairs/glm-pi.settings`, or from a bench or summonr build with uncommitted changes. Change the file only between runs.

## Question

On GLM 5.3 Flash and the 66 Terminal-Bench 4.0 tasks, how many input tokens and how much memory does summonr use against pi and OpenCode 2, and does it solve as many tasks?

## Arms

summonr, pi, and OpenCode 2, at the versions in `tools.lock` and the summonr build pinned by sha256 in `run.txt`. All run GLM 5.3 Flash through the z.ai GLM Coding Plan at effort `high`, the level every bench pair uses: summonr `--effort high`, pi `--thinking high`, OpenCode 2 the `#high` variant. None sends a temperature, so all get z.ai's default sampling.

## Tasks and tries

All 66 Terminal-Bench 4.0 tasks (`terminal-bench/terminal-bench@4.0.0`, sha256 in `dataset.lock`), three tries each, each task's own agent limit (a flat 8 h in 4.0), internet on, one trial per arm at a time (three requests in flight at most), all arms in one Harbor job. No 4.0 task was visible while summonr was developed, so all 66 are held out. Archived and not scored: the Terminal-Bench 2.1 runs `tb21-glm-1015e`, `-1015f`, `-1015g`, `-1015h`, `tb21-glm-pi-1016a` (stopped when 4.0 replaced 2.1 on the leaderboard), the stopped calibration `cal-glm-1016a`, and the Terminal-Bench 4.0 run `tb40-glm-pi-1016a` (one try each at effort `max`, stopped at 13 of 198 trials).

## Environment and measurement

As in `protocols/glm.md`: `level.py` gives every arm the same container and shell PATH, each arm writes `agent/parity.txt`, and `table.py` refuses a job whose tries of a task saw different environments. Every arm talks to z.ai through `bench/proxy.py`, labelled per trial; tokens, requests, and models come only from the proxy, with each harness's own count beside it. Memory is sampled every 0.5 s by the `rss` plugin.

## Primary claim

On the 66 tasks, against each of pi and OpenCode 2 separately, paired ratios as geometric means over tasks with 95% bootstrap intervals over tasks (`table.py RUN --subject summonr`):

- Input tokens per solved try: over tasks both arms solved, the upper bound of summonr / other is below 1.
- Mean harness RSS: the upper bound of summonr / other is below 1.
- Solve rate: the paired solve difference, summonr minus the other arm, is reported with its interval. In `tb21-glm-1015h` summonr disagreed with pi and OpenCode 2 on 16% of tasks, which gives one try on 66 tasks a 95% half-width near 0.10. Three tries narrow that only as far as a task's tries disagree with each other, so the run's own interval is the one reported.

## Validity checks

- The parity check finds no difference between the arms' environments, and every agent that ran wrote a snapshot.
- Every tool-carrying request matches a line for its arm in `pairs/glm-pi.settings`, which the first preflight at effort `high` writes from what these three arms sent; a second preflight on the run's bench commit and summonr build checks every request against it.
- Every task ran from one task checksum.

## Reruns and scoring

Reruns follow `rerun.py` as in `protocols/glm.md`: setup timeouts, rate-limit or network errors the proxy saw for that trial, and exceptions with no proxy request. Task timeouts and wrong answers stand. A solve voided by the dataset-source scan or the judge (`harbor analyze -r judge.toml`) scores 0; the report gives solve rates with and without voids.

## Report

Per arm, over the 66: solved, input, uncached input, and output tokens, time, mean and peak memory, the proxy cross-check, the settings each arm sent, and the rerun list. These are comparisons on identical containers, not leaderboard numbers.
