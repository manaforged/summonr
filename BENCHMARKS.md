# Benchmarks

Harness-level comparisons on Terminal-Bench 2.1. The first two put summonr against a vendor's own agent, both signed in to the vendor's plan, running its model at high effort, side by side in one job (`bench/board.sh`); the last puts it against two open-source agents on GLM 5.3 Flash. The headline plan tables score 3 trials of every task, fewer than the leaderboard's 5, so these are not leaderboard submissions. Resolution rate is the mean over the selected trials with a 95% bootstrap interval over tasks. Task is the sampling unit, trials of a task are correlated, and differences are paired and bootstrapped over tasks (10,000 resamples). Token counts are the usage each harness reported. Cost is the API list price, not plan spend. A solve whose trial reached a published Terminal-Bench solution is voided and counts as a miss. The judge pass (`harbor analyze -r judge.toml`) has not run on either job. These runs used Terminal-Bench 2.1; they were scored with `bench/table.py` at dbf2e1b2. Current protocols use Terminal-Bench 4.0.

## Selection and evidence

The plan tables below select the earliest three trials per harness and task, ordered by recorded start time, from larger jobs originally configured for five attempts. The selection is independent of reward. Those jobs contain multiple Summonr binary hashes, so they do not measure one immutable build. [Trial summaries, build identities, and reproduction instructions](bench/results/README.md) accompany the figures. The raw execution archives are not included, and the summaries cannot independently validate source lookups or the unfinished judge pass.

All five available ChatGPT trials per task give summonr 374/445 solves (84.0%) and Codex 359/445 (80.7%) after source-lookup voids. The paired difference is 0.03 (-0.01–0.08), so this larger sample does not establish a solve-rate lead. Paired summonr / Codex ratios remain 0.33 (0.28–0.38) for input per solved trial and 0.76 (0.71–0.80) for API-list-price-equivalent cost; execution time is 1.17 (1.10–1.24). Typical harness memory is 27 MB against 276 MB.

The evidence supports lower input-token use and harness overhead on these workloads, not general task-solving superiority. A typical memory figure is the median of trial mean RSS, not a hard ceiling. The three-trial ChatGPT sample includes a maximum individual Summonr agent RSS of 241 MB. Browser and full process-tree memory need their own deployment budget.

## Terminal-Bench 2.1 on a ChatGPT plan: summonr 0.1.0 against Codex 0.162.1

All 89 tasks, 3 trials each, `gpt-6.1-sol` at high effort, run `tb21-chatgpt-1013t`. Prices: $2.00 input, $0.10 cached input, $10.00 output per 1M tokens. Pass@3 counts the tasks solved in any trial, Pass^3 the tasks solved in all 3.

| Agent | Model | Resolution rate\* | Pass@3 | Pass^3 | Cost / solved |
| --- | --- | ---: | ---: | ---: | ---: |
| summonr 0.1.0 | GPT-6.1 Sol | 85.8% ± 6.5 (229/267) | 82/89 | 69/89 | $0.145 |
| Codex 0.162.1 | GPT-6.1 Sol | 79.0% ± 7.5 (211/267) | 76/89 | 63/89 | $0.206 |

Medians per trial:

| Harness | Input tokens | Uncached input | Output tokens | Cache hit | Cost | Time | First request |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Codex | 194,802 | 23,498 | 3,440 | 92.7% | $0.105 | 123 s | 14,355 |
| summonr | 58,351 | 19,703 | 3,014 | 85.9% | $0.078 | 166 s | 1,539 |

On solved trials only, median input was 58,976 tokens for summonr and 182,691 for Codex.

| Harness | Mean agent RSS | Peak agent RSS | Mean harness RSS | Peak harness RSS |
| --- | ---: | ---: | ---: | ---: |
| Codex | 273 MB | 333 MB | 276 MB | 358 MB |
| summonr | 27 MB | 27 MB | 27 MB | 30 MB |

Paired over the 89 tasks, summonr / Codex, geometric mean; below 1 means summonr used less. Solved difference is summonr minus Codex.

| Solved difference | Input per solved trial | Input | Uncached input | Output | Cost | Time | First request |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 0.07 (0.03–0.12) | 0.33 (0.28–0.39) | 0.32 (0.28–0.37) | 0.87 (0.79–0.94) | 0.87 (0.81–0.93) | 0.75 (0.70–0.81) | 1.19 (1.11–1.27) | 0.11 (0.11–0.11) |

\* 11 Codex solves are voided because its web search reached published Terminal-Bench solutions; summonr has none. OpenAI's cybersecurity filter refused 6 trials of each harness. Neither harness could start in the qemu-alpine-ssh and qemu-startup task images, and nginx-request-logging could not install its verifier's tools. All of these count as misses for both.

What the run shows:

- summonr resolved 85.8% of trials and Codex 79.0%, and the paired interval excludes zero.
- summonr sent 0.33x the input tokens per solved trial and cost 0.75x as much per task at list price. Its first request is 1,539 tokens against Codex's 14,355.
- summonr ran in 27 MB against Codex's 276 MB.
- summonr took 1.19x as long per task.

## Terminal-Bench 2.1 on a SuperGrok plan: summonr 0.1.0 against Grok Build 1.0.52

All 89 tasks, 3 trials each, `grok-4.7` at high effort, run `tb21-supergrok-1013t`. Grok Build sends its requests through its own CLI endpoint; summonr calls `api.x.ai`. Cost is not shown for this pair: grok-4.7 bills a request with 200K or more prompt tokens at twice the rate, summonr's longest prompt was 281,597 tokens, and Grok Build's per-request prompt sizes are not recorded.

| Agent | Model | Resolution rate\* | Pass@3 | Pass^3 |
| --- | --- | ---: | ---: | ---: |
| summonr 0.1.0 | Grok 4.7 | 74.2% ± 8.0 (198/267) | 76/89 | 55/89 |
| Grok Build 1.0.52 | Grok 4.7 | 70.4% ± 8.5 (188/267) | 70/89 | 53/89 |

Medians per trial:

| Harness | Input tokens | Uncached input | Output tokens | Cache hit | Time |
| --- | ---: | ---: | ---: | ---: | ---: |
| Grok Build | 820,158 | 89,722 | 35,894 | 93.0% | 714 s |
| summonr | 579,764 | 80,611 | 28,634 | 92.4% | 504 s |

On solved trials only, median input was 373,831 tokens for summonr and 803,466 for Grok Build. On missed trials it was 1,295,559 for summonr in 31 requests and 891,660 for Grok Build in 19.

| Harness | Mean agent RSS | Peak agent RSS | Mean harness RSS | Peak harness RSS |
| --- | ---: | ---: | ---: | ---: |
| Grok Build | 146 MB | 157 MB | 146 MB | 157 MB |
| summonr | 27 MB | 28 MB | 27 MB | 29 MB |

Paired over the 89 tasks, summonr / Grok Build, geometric mean; below 1 means summonr used less. Solved difference is summonr minus Grok Build.

| Solved difference | Input per solved trial | Input | Uncached input | Output | Time |
| --- | --- | --- | --- | --- | --- |
| 0.04 (-0.02–0.10) | 0.56 (0.48–0.64) | 0.63 (0.55–0.72) | 0.76 (0.67–0.86) | 0.82 (0.76–0.88) | 0.74 (0.65–0.88) |

\* 10 summonr solves and 5 Grok Build solves are voided because the model looked up published Terminal-Bench solutions on the web. The task time limit ended 36 summonr trials and 44 Grok Build trials. git-multibranch and pytorch-model-cli could not install their verifier's tools. All of these count as misses.

What the run shows:

- The two resolution rates are the same within the interval.
- summonr sent 0.56x the input tokens per solved trial and took 0.74x the time.
- On the trials it missed, summonr used more input than Grok Build.
- summonr ran in 27 MB against Grok Build's 146 MB.

## Terminal-Bench 2.1 on GLM 5.3 Flash: summonr 0.1.0 against pi 1.0.0 and OpenCode 2.0.22

86 tasks, one try each, `glm-5.3-flash` at high effort through z.ai, every harness on its defaults, run by `bench/run.sh`. summonr and pi ran in one job; OpenCode ran in a later job on the same tasks, model, effort, and dataset. Three tasks are dropped for every harness because their verifier could not install its tools: path-tracing-reverse, qemu-alpine-ssh, qemu-startup. Tokens only; this vendor is not priced. Intervals are 95% Wilson for solved and bootstrap medians elsewhere. The solved cut counts only the tasks that harness solved; the all cut counts all 86.

| Agent | Resolution rate | Input tokens, solved | Input tokens, all | Output tokens, solved | Output tokens, all | Time, solved | Time, all |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| summonr 0.1.0 | 73.3% (63/86, 63%–81%) | 95,271 | 194,193 | 8,231 | 12,920 | 219 s | 470 s |
| pi 1.0.0 | 72.1% (62/86, 62%–80%) | 240,762 | 212,337 | 15,226 | 15,310 | 368 s | 410 s |
| OpenCode 2.0.22 | 67.4% (58/86, 57%–76%) | 228,578 | 253,726 | 12,364 | 17,756 | 341 s | 567 s |

| Agent | Mean agent RSS | Mean harness RSS |
| --- | ---: | ---: |
| summonr | 24 MB | – |
| pi | 134 MB | – |
| OpenCode | 86 MB | 394 MB |

Harness RSS adds every helper the harness started without a shell; OpenCode's own server brings it to 394 MB. Paired over the 86 tasks, summonr / other, geometric mean:

| Other | Solved difference | Input tokens | Output tokens | Time |
| --- | --- | --- | --- | --- |
| pi | 0.01 (-0.10–0.12) | 0.94 (0.76–1.17) | 0.86 (0.72–0.99) | 0.99 (0.89–1.11) |
| OpenCode | 0.06 (-0.02–0.14) | 0.74 (0.57–1.00) | 0.86 (0.74–1.06) | 0.77 (0.68–0.87) |

What the run shows:

- The three resolution rates are the same within the interval.
- On the tasks each agent solved, summonr used the fewest input tokens, 95,271 against 228,578 and 240,762.
- Across all 86 tasks the input gap narrows to 0.94 against pi and 0.74 against OpenCode, because summonr's failed tasks ran long.
- summonr ran in 24 MB against pi's 134 MB and OpenCode's 394 MB.

## Suite layout

`bench/tasks/<name>/`: `task.txt` (prompt), `fixture/` (the workspace, committed as the baseline of a fresh repo), `verify.sh` (exit 0 = solved). 16 tasks: bugfix, feature, and refactor fixtures plus repo-navigation tasks. `bench/tasks-hard/` holds 20 harder tasks. `bench/README.md` documents the runner and the Terminal-Bench board.
