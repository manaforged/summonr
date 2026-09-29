# harness-benchmark

Fixed-model task runs for the summonr CLI. Harbor is the runner, not this suite.

Clone, pin, run, print the table. Keys stay out of git.

## Tracks

| Track | Task file | k | When |
| --- | --- | --- | --- |
| Smoke | `tasks-dev.txt` (25) | 1 | wiring |
| **Tune** | `tasks-tune.txt` (10 IDs, excluded from the board) | 1 | summonr health before any board |
| **Board (terminal)** | `tasks-eval.txt` (54 IDs, frozen 2026-09-23 after dropping the 10 tuned IDs) | 5 | first table |
| Qualification (terminal) | full Terminal-Bench 2.1 (89 IDs) | 5 | after the 54-ID table |

Every arm runs at effort `high` (`BENCH_EFFORT`, default `high`): summonr gets `--effort high`, pi gets `--thinking high`, and the proxy log shows what each arm sent. Two model pins, never mixed: `deepseek-flash` (default) or `glm-5.3-flash` (`BENCH_VENDOR=zai`). Same list, same seed, same defaults. GLM uses `api.z.ai`. Cost is priced only for vendors with a rate card in `table.py` (deepseek); other vendors report tokens only.

## Run

```
printf 'DEEPSEEK_API_KEY=...\n' > deepseek.env
chmod 600 deepseek.env
./tools.sh ./tools /path/to/summonr
python3 proxy.py --bind 0.0.0.0 --port 8094 --log logs/deepseek.jsonl \
  --upstream https://api.deepseek.com --anthropic https://api.deepseek.com/anthropic
BENCH_REPEATS=1 ./run.sh tune-10 tasks-tune.txt
# GLM-5.3-Flash (glm.env with ZAI_API_KEY):
# python3 proxy.py --bind 0.0.0.0 --port 8095 --log logs/glm.jsonl --upstream https://api.z.ai/api/coding/paas/v4 --anthropic https://api.z.ai/api/anthropic
# BENCH_VENDOR=zai BENCH_REPEATS=1 ./run.sh tune-10-glm tasks-tune.txt
python3 table.py tune-10 --subject summonr --proxy logs/deepseek.jsonl --tasks tasks-tune.txt
# after summonr is not failing the 10:
BENCH_REPEATS=5 ./run.sh board-tb21 tasks-eval.txt
python3 table.py board-tb21 --subject summonr --proxy logs/deepseek.jsonl --tasks tasks-eval.txt
```

`jobs/<run>/run.txt` records seed, model, k, dataset tree hash, task-list hash, harness.sh hash, summonr binary sha256 and source commit. Rerunning a run id aborts on any recorded mismatch (seed, vendor, provider, model, effort, repeats, timeout multiplier, harness.sh, dataset, tasks, summonr binary, summonr commit, tools manifest). A summonr built from a tree with uncommitted changes records its commit as `<sha>-dirty`. `table.py` refuses a run without `run.txt`, a run in which any arm's response model is not the pinned model, a run in which an arm's requests succeeded but the proxy log holds no token usage for it, and a run in which a trial solved its task with no request in the proxy log. It prints reliability (pass@1 / pass@k / pass^k), all / solved / failed cuts, paired ratios, and effort honored per harness, measured from proxy request bodies. A dataset-identity leak hit (terminal-bench, tbench.ai, laude-institute, marginlab.ai, harbor-framework) voids the solve; a verifier-path hit (/logs/verifier, reward.txt, /solution/, /tests/test) is a warn.

`./restart-bench.sh RUN_SUFFIX` stops any running board, removes Docker networks that stopped trials left behind (they exhaust Docker's 172.x pool, and the firewall only lets 172.16.0.0/12 reach the proxy), rebuilds summonr from the bench checkout, and starts GLM and DeepSeek boards as `tb21-zai-RUN_SUFFIX` and `tb21-deepseek-RUN_SUFFIX`. The proxy logs `nudged` on the request that follows a reasoning nudge.

## Fairness

- Task IDs committed before looking at harness scores; the 10 tune IDs are excluded from the board list.
- `dataset.lock` pins the dataset tree hash; `run.sh` aborts on a mismatch and on a dataset with no entry.
- The proxy port follows the vendor: 8094 for `deepseek`, 8095 for `zai`. Set `BENCH_PROXY` to override.
- Sampling controls (temperature, max_tokens, effort wire values) are measured from proxy request bodies and printed in "Sampling controls seen" and "Effort honored". `--agent-timeout-multiplier` is fixed at 1.0 unless `BENCH_TIMEOUT_MULT` says otherwise, recorded in run.txt.
- `table.py` requires `--subject`; no harness is the default anchor.
- Tokens come from the logging proxy, not from a CLI self-report. The vendor key is inside each task container, so a solver that called the vendor directly would bypass the proxy.
- Trials run in lockstep, in task-list order: every harness starts the same task at the same time, and the next task starts only when all of them finish. Every partial table is a 1:1 comparison on the same tasks, and both arms share the same host load, proxy, and vendor rate limit.
- The qualification track (all 89 tasks) includes the 10 tune tasks and the 25 dev tasks, which were visible while summonr was adjusted. Only the 54-task board is held out.
- Time is Harbor `agent_execution` wall, not turn count.
- Cost / solved = all-trial spend / solved count (failures stay in the numerator).
