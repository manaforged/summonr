# SuperGrok plan pair protocol

`board.sh` records this file's sha256 in `run.txt`; change it only between runs.

## Arms

summonr and Grok Build, at the versions in `tools.lock` and the summonr build pinned by sha256, both on `grok-4.7` at effort `high` through the same SuperGrok plan sign-in.

## Tasks and tries

All 66 Terminal-Bench 4.0 tasks (`terminal-bench/terminal-bench@4.0.0`, sha256 in `dataset.lock`), three tries each, each task's own agent limit (a flat 8 h in 4.0), internet on, two trials per arm at a time, both arms in one Harbor job. No 4.0 task was visible while summonr was developed, so all 66 are held out. A preflight on the run's bench commit runs one try per arm, with its time limit capped by `BENCH_PREFLIGHT_TIMEOUT_MULT`, and must pass before the full run.

## Measurement

Each harness reports its own tokens; this pair has no proxy, so token figures are self-reported and the table says so. Memory is sampled every 0.5 s by the `rss` plugin. A solve voided by the dataset-source scan or the judge (`harbor analyze -r judge.toml`) scores 0, and the report gives solve rates with and without voids.

## Claim

Paired over the 66 tasks, geometric means over tasks with 95% bootstrap intervals (`table.py RUN --subject summonr`): solve difference, input tokens per solved try, and mean harness RSS, each reported whichever way it points. A solve lead is claimed only if its interval excludes 0.

## Reruns

Setup timeouts and exceptions raised before the harness ran rerun for both arms alike; task timeouts and wrong answers stand. The table lists every rerun.
