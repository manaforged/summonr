# Benchmarks

Harness-level comparison. Model, tasks, and endpoint match; the runs were not simultaneous. Task is the sampling unit, repeats are correlated trials, and differences are paired and bootstrapped.

## Terminal-Bench 2.1 on plan sign-in: summonr against Codex, 2026-10-13

87 tasks, one pass each, `gpt-6.1-sol` at high effort, both harnesses signed in with a ChatGPT plan, run side by side by `bench/run.sh` as `tb21-chatgpt-1013h` (summonr 0.1.0, Codex 0.162.1). Every request went through the logging proxy, which counts tokens and time. Two tasks are dropped for both harnesses because their verifier could not install its tools: qemu-alpine-ssh, qemu-startup. Cost is priced at the API list price ($2.00 input, $0.10 cached input, $10.00 output per 1M tokens), not plan spend. Intervals are 95% Wilson for solved and bootstrap medians (10,000 resamples) elsewhere.

| Harness | Solved\* | Cost / solved | Input tokens / task | Uncached input / task | Output tokens / task | Cache hit | Cost / task | Time / task | First request |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Codex | 68/87 (78%, 68%–86%) | $0.210 | 202,276 | 26,922 | 3,825 | 91.9% | $0.109 | 160 s | 14,368 |
| summonr | 67/87 (77%, 67%–85%) | $0.144 | 51,373 | 18,047 | 2,973 | 85.9% | $0.071 | 161 s | 1,534 |

| Harness | Mean agent RSS | Peak agent RSS | Mean harness RSS | Peak harness RSS |
| --- | ---: | ---: | ---: | ---: |
| Codex | 180 MB | 239 MB | 204 MB | 282 MB |
| summonr | 25 MB | 26 MB | 25 MB | 26 MB |

Paired over the 87 tasks, summonr / Codex, geometric mean; below 1 means summonr used less.

| Solved difference | Input tokens | Uncached input | Output tokens | Cost | Time |
| --- | --- | --- | --- | --- | --- |
| -0.01 (-0.09–0.08) | 0.28 (0.22–0.35) | 0.72 (0.62–0.84) | 0.78 (0.63–0.94) | 0.65 (0.54–0.77) | 0.91 (0.72–1.13) |

\* Provider errors ended 9 tasks before the harness could finish them. OpenAI's cybersecurity filter refused break-filter-js-from-html and vulnerable-secret for both harnesses. Capacity errors ended 3 Codex tasks ("Selected model is at capacity") and 4 summonr tasks ("servers are currently overloaded"). The table counts all 9 as failures. Four Codex solves are voided because its web search reached published Terminal-Bench solutions; the scanner removes any solve whose trial reached a dataset source.

What the run shows:

- The two harnesses solved the same number of tasks within the interval.
- summonr sent 0.28x the input tokens and cost 0.65x as much per task at list price. Its first request is 1,534 tokens against Codex's 14,368.
- summonr ran in 25 MB against Codex's 180 MB.
- Time per task is even.

## Terminal-Bench 2.1, 2026-10-02 to 2026-10-03

86 tasks, one pass each, GLM 5.3 Flash (`glm-5.3-flash`) at high effort through z.ai, every harness on its defaults, run by `bench/run.sh` with the method in [`bench/README.md`](bench/README.md). Three runs: `tb21-zai-1002d` (summonr 0.1.0-rc.1 at 74e08fbc, pi 1.0.0), `tb21-zai-1002f` (OpenCode 2.0.22, Hermes v2026.9.24), and `tb21-zai-1003a` (OMP 18.4.12). Arms from different runs did not run side by side; tasks, model, effort, and dataset match. Three tasks are dropped for every harness because their verifier could not install its tools: path-tracing-reverse, qemu-alpine-ssh, qemu-startup. Cost is not priced for this vendor; tokens only. Intervals are 95% Wilson for solved and bootstrap medians (10,000 resamples) elsewhere.

Solved shows the task count with its interval. Token and time figures are medians per task, in two cuts. The solved cut counts only the tasks that harness solved, so each harness has its own task set. The all cut counts all 86 tasks, solved and failed. Uncached input is the input the provider did not serve from its prompt cache. Cache hit is the share of input tokens served from the provider's prompt cache, taken from the usage each response reported.

| Harness | Solved | Input tokens, solved | Input tokens, all | Uncached input, solved | Uncached input, all | Cache hit, solved | Cache hit, all | Output tokens, solved | Output tokens, all | Time, solved | Time, all |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| summonr | 63/86 (73%, 63%–81%) | 95,271 | 194,193 | 14,151 | 23,436 | 95.3% | 95.0% | 8,231 | 12,920 | 219 s | 470 s |
| pi | 62/86 (72%, 62%–80%) | 240,762 | 212,337 | 22,464 | 22,758 | 95.3% | 95.2% | 15,226 | 15,310 | 368 s | 410 s |
| OMP | 59/86 (69%, 58%–77%) | 218,939 | 294,356 | 17,617 | 20,030 | 95.3% | 95.0% | 10,642 | 12,531 | 294 s | 327 s |
| OpenCode | 58/86 (67%, 57%–76%) | 228,578 | 253,726 | 29,448 | 40,433 | 92.8% | 93.2% | 12,364 | 17,756 | 341 s | 567 s |
| Hermes | 49/86 (57%, 46%–67%) | 225,215 | 272,086 | 19,053 | 20,568 | 96.9% | 96.8% | 12,047 | 16,118 | 335 s | 558 s |

RSS is sampled every 0.5 s per trial. Agent RSS is the harness process alone. Harness RSS is the harness process and every helper it started without a shell (language servers, MCP servers, its own server); a dash means the harness started no such helper. The README's "Mean memory" column shows harness RSS where it is measured and agent RSS otherwise.

| Harness | Mean agent RSS | Peak agent RSS | Mean harness RSS | Peak harness RSS |
| --- | ---: | ---: | ---: | ---: |
| summonr | 24 MB | 24 MB | – | – |
| pi | 134 MB | 146 MB | – | – |
| OMP | 368 MB | 413 MB | 387 MB | 505 MB |
| OpenCode | 86 MB | 95 MB | 394 MB | 480 MB |
| Hermes | 200 MB | 204 MB | 202 MB | 244 MB |

Mean and peak are medians over trials of each trial's mean and highest sample.

Paired against summonr over the 86 tasks, solved and failed. Solved difference is summonr minus other. The other columns are summonr / other, geometric mean; below 1 means summonr used less.

| Other | Solved difference | Input tokens | Output tokens | Time |
| --- | --- | --- | --- | --- |
| pi | 0.01 (-0.10–0.12) | 0.94 (0.76–1.17) | 0.86 (0.72–0.99) | 0.99 (0.89–1.11) |
| OMP | 0.05 (-0.05–0.15) | 0.58 (0.47–0.71) | 0.91 (0.80–1.02) | 1.03 (0.88–1.23) |
| OpenCode | 0.06 (-0.02–0.14) | 0.74 (0.57–1.00) | 0.86 (0.74–1.06) | 0.77 (0.68–0.87) |
| Hermes | 0.16 (0.06–0.27) | 0.87 (0.58–1.31) | 0.92 (0.67–1.35) | 0.71 (0.62–0.80) |

What the run shows:

- summonr solved the most tasks, but its lead over pi, OMP, and OpenCode is inside the interval. Only Hermes is separated.
- On the tasks each harness solved, summonr used the fewest tokens and the least time: 95,271 input tokens against 218,939 to 240,762 for the others.
- Across all 86 tasks the gap is smaller. summonr's median input is still the lowest, but its median output is above OMP's, and its median time is above OMP's and pi's. Paired task by task, summonr used fewer input tokens than OMP; against pi, OpenCode, and Hermes the input-token interval reaches 1.
- Failed tasks cause the difference. summonr's failed tasks ran a median of 551,156 input tokens and 29 requests, against 199,458 and 14 for pi.
- summonr ran in 24 MB. The next smallest agent process was OpenCode at 86 MB, and its own server brings OpenCode to 394 MB.

## Suite layout

`bench/tasks/<name>/`: `task.txt` (prompt), `fixture/` (the workspace, committed as the baseline of a fresh repo), `verify.sh` (exit 0 = solved). 16 tasks: bugfix, feature, and refactor fixtures plus repo-navigation tasks. `bench/tasks-hard/` holds 20 harder tasks. `bench/README.md` documents the runner and the Terminal-Bench board.
