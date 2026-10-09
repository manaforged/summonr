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

A harness named `summonr-NAME` runs the binary at `tools/summonr-NAME/bin/summonr`, so two summonr builds can run as paired arms. Every arm runs at effort `high` (`BENCH_EFFORT`, default `high`): summonr gets `--effort high`, pi gets `--thinking high`, opencode2 gets the `#high` model variant, hermes gets `--reasoning high`, omp gets `--thinking high`, codex gets `model_reasoning_effort=high`, and the proxy log shows what each arm sent. opencode2 and hermes run their built-in z.ai providers (`zai-coding-plan`, `zai`) pointed at the proxy, and omp reads a `models.yml` that points its provider at the proxy; hermes sends no `max_tokens`, so the provider default applies, and the table shows it under sampling controls. Memory is sampled every 0.5 s three ways: agent (the harness process), harness (it and every helper it started without a shell, such as a language server, an MCP server, or opencode's own server), and tree (every process under it). Three model pins, never mixed: `deepseek-flash` (default), `glm-5.3-flash` (`BENCH_VENDOR=zai`), or `gpt-6.1-sol` (`BENCH_VENDOR=openai`, `openai.env` with `OPENAI_API_KEY`). On `openai`, codex runs its native binary with a `bench` model provider on the Responses wire, with WebSockets off so every request is an HTTP POST the proxy can count, and with hosted web search on, as in a stock install; summonr runs its `openai` endpoint on the same wire. Codex keeps its rollout under `/logs/agent/codex/sessions/`, where each response's usage is a cross-check on the proxy count. Same list, same seed, same defaults. GLM uses `api.z.ai`. Cost is priced only for vendors with a rate card in `table.py` (deepseek); other vendors report tokens only.

## Run

```
printf 'DEEPSEEK_API_KEY=...\n' > deepseek.env
chmod 600 deepseek.env
./tools.sh ./tools ..
BENCH_REPEATS=1 ./run.sh tune-10 tasks-tune.txt
# GLM-5.3-Flash (glm.env with ZAI_API_KEY):
# BENCH_VENDOR=zai BENCH_REPEATS=1 ./run.sh tune-10-glm tasks-tune.txt
# GPT-6.1 Sol, summonr against codex (openai.env with OPENAI_API_KEY):
# BENCH_VENDOR=openai BENCH_REPEATS=1 ./run.sh tune-10-sol tasks-tune.txt summonr codex
python3 table.py tune-10 --subject summonr --tasks tasks-tune.txt
# after summonr is not failing the 10:
BENCH_REPEATS=5 ./run.sh board-tb21 tasks-eval.txt
python3 table.py board-tb21 --subject summonr --tasks tasks-eval.txt
```

`run.sh` starts the vendor's logging proxy on its port and logs to `logs/RUN_ID.jsonl`; `table.py` reads that file unless `--proxy` names another. Only the proxy process reads the env file and holds the vendor key. Every `KEY`, `KEY_2`, `KEY_3`… of the vendor's key variable in the env file is a key: the proxy gives each new task to the key with the fewest active tasks, both harnesses of a task use the same key, and each proxy record names its key index. `BENCH_DUMP=1` also saves every request body under `logs/RUN_ID.bodies/LABEL/`. Harbor and the task containers get the placeholder `bench-proxy`. A vendor row can add keys for another upstream (`routes`, `NAME=URL` on `--key-env`) and drop request fields that upstream rejects (`drops`, `--drop HOST=field,field`); each proxy record names its upstream host and the fields it dropped.

`jobs/<run>/run.txt` records seed, model, k, dataset tree hash, task-list hash, harness.sh hash, summonr binary sha256 and source commit. Rerunning a run id aborts on any recorded mismatch (seed, vendor, provider, model, effort, repeats, timeout multiplier, harness.sh, dataset, tasks, summonr binary, summonr commit, tools manifest). A summonr built from a tree with uncommitted changes records its commit as `<sha>-dirty`. `table.py` refuses a run without `run.txt`, a run in which any arm's response model is not the pinned model, a run in which an arm's requests succeeded but the proxy log holds no token usage for it, and a run in which a trial solved its task with no request in the proxy log. `table.py RUN [RUN...]` combines runs when vendor, provider, model, effort, dataset, task list, and timeout multiplier match and no harness appears twice, and says the arms did not run side by side. It prints each arm's version, then reliability (pass@1 / pass@k / pass^k), all / solved / failed cuts, paired ratios, and effort honored per harness, measured from proxy request bodies. A URL or GitHub path to a dataset source (terminal-bench, tbench, marginlab, laude-institute, harbor-framework) voids the solve; a bare source name or a verifier path (/logs/verifier, reward.txt, /solution/, /tests/test) is a warn. A stream the agent closed before its usage frame counts under "Cut streams"; its tokens are not in the totals.

`tools.sh` installs the Pi, OpenCode (`@opencode/cli`), Hermes, OMP (`@oh-my-pi/pi-coding-agent`, run on Bun), Codex (`@openai/codex`, its linux-x64 native binary and bundled `rg`), Bun, and uv versions that `tools.lock` pins and records them, the summonr version, and Node in `versions.txt`; Hermes installs inside the tools container so its paths match the `/opt/bench` mount, and the container hands `tools/` back to the invoking user. `run.txt` records every arm's version, a resume with any other version aborts, and `table.py` prints each arm's version under the run line; `run.txt` records the Pi version, and a resume with another aborts. A trial whose `result.json` has no verifier result (the host stopped it) moves aside and runs again on resume. `./restart-bench.sh RUN_SUFFIX [VENDOR...]` stops any running board, removes Harbor containers and the Docker networks that stopped trials left behind, moves its own checkout to `origin/main`, rebuilds summonr from it, and starts the named boards (default `zai deepseek`) as `tb21-VENDOR-RUN_SUFFIX`. The bench runs in the product checkout; `jobs`, `logs`, `datasets`, and `tools` may be links to a data disk. The firewall lets only 172.16.0.0/12 reach the proxy, so the host's Docker `default-address-pools` must stay inside that range. The proxy logs `nudged` on the request that follows a reasoning nudge.

## Fairness

- Task IDs committed before looking at harness scores; the 10 tune IDs are excluded from the board list.
- `dataset.lock` pins the dataset tree hash; `run.sh` aborts on a mismatch and on a dataset with no entry.
- The proxy port follows the vendor: 8094 for `deepseek`, 8095 for `zai`, 8096 for `openai`. Set `BENCH_PROXY` to override.
- Sampling controls (temperature, max_tokens, effort wire values) are measured from proxy request bodies and printed in "Sampling controls seen" and "Effort honored". `--agent-timeout-multiplier` is fixed at 1.0 unless `BENCH_TIMEOUT_MULT` says otherwise, recorded in run.txt.
- `table.py` requires `--subject`; no harness is the default anchor.
- Tokens come from the logging proxy, not from a CLI self-report. Task containers hold only the placeholder key, so a solver cannot call the vendor around the proxy.
- Trials run in lockstep, in task-list order: every harness starts the same task at the same time. `BENCH_PARALLEL` (default 1, recorded in run.txt) sets how many tasks run at once; a slot takes the next task only when every harness has finished its current one. Every partial table is a 1:1 comparison on the same tasks, and both arms share the same host load, proxy, and vendor rate limit.
- The qualification track (all 89 tasks) includes the 10 tune tasks and the 25 dev tasks, which were visible while summonr was adjusted. Only the 54-task board is held out.
- Time is Harbor `agent_execution` wall, not turn count.
- Cost / solved = all-trial spend / solved count (failures stay in the numerator).
