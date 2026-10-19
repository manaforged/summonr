# harness-benchmark

Fixed-model task runs for the summonr CLI. Harbor is the runner, not this suite.

Clone, pin, run, print the table. Keys stay out of git.

## Reproduce

This runs a published plan pair the way the Terminal-Bench leaderboard runs: Harbor, the dataset version `dataset.lock` pins, the tasks' own time limits, internet on, and 3 tries per task where the leaderboard asks for 5.

1. Use a Linux host with Docker (Linux containers) and Python 3. Install the Harbor version that `tools.lock` pins: `uv tool install harbor==0.24.0`. `board.sh` refuses any other version.
2. Check out the summonr commit the published run names in `BENCHMARKS.md`, then build summonr and pin the vendor CLIs: `./tools.sh ./tools ..`. The build needs no keys. summonr builds in an Alpine container (`alpine` in `tools.lock`) as a static `x86_64-unknown-linux-musl` binary, the target `install.sh` selects by default, so it starts on task images with any glibc; the vendor CLIs install in a Debian container.
3. Sign in on the host with your own plans. For `chatgpt`: Codex, so that `~/.codex/auth.json` exists, and `summonr login openai` with the ChatGPT sign-in. For `supergrok`: Grok Build, so that `~/.grok/auth.json` exists, and `summonr login grok` with the SuperGrok sign-in. For `glm`: put `ZAI_API_KEY=...` (a GLM Coding Plan key) in `bench/glm.env`. `board.sh` starts the logging proxy from `pairs/glm.proxy`, the proxy holds the key, and the containers get a placeholder. `BENCH_LOGINS` moves the home these are read from.
4. Try two tasks first, then run the full set under a new run id:

```
BENCH_TASKS="fix-git openssl-selfsigned-cert" ./board.sh smoke-chatgpt chatgpt
./board.sh tb21-chatgpt-mine chatgpt
```

5. Judge every passing trial for reward hacking and solution lookups, then print the tables:

```
harbor analyze -r judge.toml jobs/tb21-chatgpt-mine
python3 table.py tb21-chatgpt-mine --subject summonr
```

The judge is Harbor's default evaluator, Claude Code on `claude-haiku-4-5`, so it needs an Anthropic key (`--ae ANTHROPIC_API_KEY=...`). A judge FAIL or a URL to a dataset source voids the solve. The source scan reads every file under the trial's `agent/` folder except SQLite databases, where a harness keeps its provider catalog; the same session is in its text log. `table.py` prints the score with voided solves at 0, as the leaderboard scores them, the same count with them counted, and pass@k on the tasks where no try of either harness was voided. It names any passing trial the judge has not seen.

The job pins the summonr build: `board.sh` records its sha256, and a trial whose binary differs fails at install. A rerun with the same run id resumes the job: a trial the host stopped (no `result.json`, neither a verifier result nor an error, or a `CancelledError` from stopping the job) moves to `jobs/RUN.rerun/` and runs again, and `BENCH_RERUN=1` also reruns setup timeouts and the provider failures the proxy saw. Your counts will differ from ours by provider load and sampling; the pass@k intervals in `BENCHMARKS.md` show how far.

## Tracks

| Track | Task file | k | When |
| --- | --- | --- | --- |
| Smoke | `tasks-dev.txt` (25) | 1 | wiring |
| **Tune** | `tasks-tune.txt` (10 IDs, excluded from the board) | 1 | summonr health before any board |
| **Board (terminal)** | `tasks-eval.txt` (54 IDs, frozen 2026-09-23 after dropping the 10 tuned IDs) | 5 | first table |
| Qualification (terminal) | full Terminal-Bench 2.1 (89 IDs) | 5 | after the 54-ID table |

A harness named `summonr-NAME` runs the binary at `tools/summonr-NAME/bin/summonr`, so two summonr builds can run as paired arms. Every arm runs at effort `high` (`BENCH_EFFORT`, default `high`): summonr gets `--effort high`, pi gets `--thinking high`, opencode2 gets the `#high` model variant, hermes gets `agent.reasoning_effort: high` in its config, omp gets `--thinking high`, codex gets `model_reasoning_effort=high`, and the proxy log shows what each arm sent. opencode2 and hermes run their built-in z.ai providers (`zai-coding-plan`, `zai`) pointed at the proxy, and omp reads a `models.yml` that points its provider at the proxy; hermes sends no `max_tokens`, so the provider default applies, and the table shows it under sampling controls. Memory is sampled every 0.5 s three ways: agent (the harness process), harness (it and every helper it started without a shell, such as a language server, an MCP server, or opencode's own server), and tree (every process under it). Model pins, never mixed: `deepseek-flash` (default), `glm-5.3-flash` (`BENCH_VENDOR=zai`), or `gpt-6.1-sol` (`BENCH_VENDOR=openai`, `openai.env` with `OPENAI_API_KEY`). Three plan vendors run summonr against each vendor's own CLI on the same subscription: `claude` (`claude-opus-5-5`, summonr's `claude` endpoint against Claude Code), `chatgpt` (`gpt-6.1-sol`, summonr's `openai` endpoint against Codex), and `supergrok` (`grok-4.7`, summonr's `grok` endpoint against Grok Build). On `openai`, codex runs its native binary with a `bench` model provider on the Responses wire, with WebSockets off so every request is an HTTP POST the proxy can count, and with hosted web search on, as in a stock install; summonr runs its `openai` endpoint on the same wire. Codex keeps its rollout under `/logs/agent/codex/sessions/`, where each response's usage is a cross-check on the proxy count. Same list, same seed, same defaults. GLM uses `api.z.ai`. Cost is priced only for vendors with a rate card in `table.py` (deepseek); other vendors report tokens only.

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


`tools.sh` installs the Pi, OpenCode (`@opencode/cli`), Hermes, OMP (`@oh-my-pi/pi-coding-agent`, run on Bun), Codex (`@openai/codex`, its linux-x64 native binary and bundled `rg`), Claude Code (`@anthropic-ai/claude-code`), Grok Build (`@xai-official/grok`), Bun, and uv versions that `tools.lock` pins and records them, the summonr version, and Node in `versions.txt`; Hermes installs inside the tools container so its paths match the `/opt/tools` mount, and the container hands `tools/` back to the invoking user. `run.txt` records every arm's version, a resume with any other version aborts, and `table.py` prints each arm's version under the run line; `run.txt` records the Pi version, and a resume with another aborts. A trial whose `result.json` has no verifier result (the host stopped it) moves aside and runs again on resume. The bench runs in the product checkout; `jobs`, `logs`, `datasets`, and `tools` may be links to a data disk. The firewall lets only 172.16.0.0/12 reach the proxy, so the host's Docker `default-address-pools` must stay inside that range. The proxy logs `nudged` on the request that follows a reasoning nudge.

## Plan pairs

A plan pair runs the way the Terminal-Bench leaderboard runs, `harbor run -d terminal-bench/terminal-bench@4.0.0` with the pair's `n_attempts` (3) in place of the leaderboard's `-k 5`, the task instructions unchanged, internet on, `timeout_multiplier` 1.0, and no resource or timeout overrides ([Terminal-Bench 4.0](https://www.tbench.ai/news/terminal-bench-4-0), [leaderboard](https://tbench.ai/leaderboard)). `board.sh RUN PAIR` renders `pairs/PAIR.yaml` into one Harbor job that holds both arms, so their trials interleave under the same provider load, one trial at a time per arm, and checks the hub dataset against `dataset.lock` first. Rerunning a run id resumes the Harbor job.

| Pair | Vendor arm | summonr arm |
| --- | --- | --- |
| `chatgpt` | Harbor's `codex` agent, ChatGPT sign-in from `~/.codex/auth.json` | `summonr_agent:Summonr`, `openai` endpoint |
| `supergrok` | `grok_plan:GrokPlan`: Harbor's `grok-build` agent signed in with SuperGrok | `summonr_agent:Summonr`, `grok` endpoint |
| `glm` | Harbor's `pi` agent, `hermes_agent:HermesCli`, `opencode2_agent:OpenCode2` (OpenCode 2, `@opencode/cli`), and `omp_agent:Omp` (OMP on Bun), all on z.ai's GLM Coding Plan | `summonr_agent:Summonr`, `zai` endpoint |

Vendor arms keep Harbor's defaults; Grok Build's server-side web search stays off, as in Harbor. Harbor's OpenCode agent installs OpenCode 1.x (`opencode-ai`) and Harbor has no OMP agent, so `opencode2_agent` and `omp_agent` reuse Harbor's OpenCode and pi adapters and change only the install and the command line. Harbor's Hermes agent has no effort option, so `hermes_agent` adds `agent.reasoning_effort` to the config Harbor writes and keeps the rest of it. Since 0.21, Hermes has no `hermes version` command, which Harbor's Hermes agent runs after install, and Hermes 0.22's installer leaves its dependency environment unrecorded in a container ("no dependency environment is committed", [hermes-agent#138990](https://github.com/NousResearch/hermes-agent/issues/138990)). `hermes_agent` reuses Harbor's agent, fetches the installer from the pinned tag, and ends the install with `hermes pm repair && hermes --version`. Every arm of a `glm` job starts from the same container: before its harness installs, `level.py` records the task image's PATH and installs one base package set (`ca-certificates`, `curl`, `git`, `ripgrep`, `xz`, `unzip`, `bash`, `coreutils`, `python3`, `nodejs`, `npm`), and after the install it restores the shell startup files the installers edited and wraps the harness's launcher so the harness runs its own runtime by absolute path while every shell it opens sees only the image PATH. No arm gets a language runtime the others lack from its installer; Hermes still runs its terminal inside its own Python venv. Scores are compared across arms on identical containers and are not leaderboard numbers. Every `glm` arm, plus a second summonr arm as an A/A control, talks to z.ai through the logging proxy, so `table.py` checks each arm's reported tokens against the proxy's count and lists the effort, `max_tokens`, and temperature each arm sent. `BENCH_PREFLIGHT=1 ./board.sh RUN glm` runs one try per arm plus `probe/env-probe`, which asks each harness's own shell which runtimes it reaches, and writes `pairs/glm.settings` from what each arm sent. `BENCH_RERUN=1 ./board.sh RUN glm` resumes a run after moving out the trials `rerun.py` lists. A full `glm` run follows `protocols/glm.md`, whose sha256 `board.sh` records in `run.txt`; `table.py` refuses a job in which arms saw different environments. Every `glm` arm reads `ZAI_API_KEY` (OpenCode 2 gets it as `ZHIPU_API_KEY`; summonr sends a provider key from the environment only to that provider's own URL, so its arms get the proxy placeholder as `api_key` through `BENCH_KEY`), and Hermes gets `GLM_BASE_URL` set to the Coding Plan endpoint, which the other arms use by default. `tools.lock` pins each CLI version. `GrokPlan` changes only sign-in: Harbor's `grok-build` takes an xAI API key. The summonr arm uploads the build at `tools/summonr/bin/summonr` (`BENCH_SUMMONR`) and writes an ATIF `agent/trajectory.json` like the built-in agents. Like Harbor's Codex, Pi, OpenCode, and Grok Build adapters, it copies a task's `skills_dir` into its own skills folder (`$SUMMONR_HOME/skills`) and writes the task's MCP servers into its config as `[mcp.NAME]`, so tasks that ship skills or MCP servers start for every arm. summonr and Grok Build access tokens expire within hours, so trials mount `~/.summonr` and `~/.grok` read-write at `/opt/auth` and link the credential files, and a refresh in one trial reaches the others. Some task images run as a non-root user and name no agent user; `image_user:Docker`, the environment every pair uses, then runs every arm as the image's user so Harbor's own handoffs (Codex `auth.json`) reach that user, and `image_user.lend` gives summonr and Grok Build a private copy of `/opt/auth`. The copy goes back to the mount when the trial ends, unless another trial changed the file first; a refresh in such a trial does not reach the others until then. Codex tokens last ten days, so Harbor's copy of `auth.json` per trial is safe; sign in again before a run if the login is older than a day.

Token counts come from each agent's `result.json` (`n_input_tokens`, `n_cache_tokens`, `n_output_tokens`), as on the leaderboard, except in a run with a proxy log, such as `glm`, where they come from the proxy and the agent's own count is printed beside them. The `rss:Rss` job plugin samples each agent's process tree from the host every 0.5 s between agent start and end and writes `memory.txt` into the trial folder. [Reproduce](#reproduce) covers the judge pass; a flagged trial scores 0, as on the leaderboard ([integrity update](https://www.tbench.ai/news/leaderboard-integrity-update)).

```
./board.sh tb21-chatgpt-1014a chatgpt
python3 table.py tb21-chatgpt-1014a --subject summonr
```

The Claude pair still runs through `run.sh` (`BENCH_VENDOR=claude`).

## Fairness

- Task IDs committed before looking at harness scores; the 10 tune IDs are excluded from the board list.
- `dataset.lock` pins the dataset tree hash; `run.sh` aborts on a mismatch and on a dataset with no entry.
- The proxy port follows the vendor: 8094 for `deepseek`, 8095 for `zai`, 8096 for `openai`, 8091 for `claude`, 8092 for `chatgpt`, 8093 for `supergrok`. Set `BENCH_PROXY` to override.
- Sampling controls (temperature, max_tokens, effort wire values) are measured from proxy request bodies and printed in "Sampling controls seen" and "Effort honored". `--agent-timeout-multiplier` is fixed at 1.0 unless `BENCH_TIMEOUT_MULT` says otherwise, recorded in run.txt.
- `table.py` requires `--subject`; no harness is the default anchor.
- Tokens come from the logging proxy, not from a CLI self-report. On key vendors, task containers hold only the placeholder key, so a solver cannot call the vendor around the proxy. On plan vendors, both arms of a pair use the same plan tier, and each container holds its arm's plan login.
- `run.sh` trials run in lockstep, in task-list order: every harness starts the same task at the same time. `BENCH_PARALLEL` (default 1, recorded in run.txt) sets how many tasks run at once; a slot takes the next task only when every harness has finished its current one. Every partial table is a 1:1 comparison on the same tasks, and both arms share the same host load, proxy, and vendor rate limit. A `board.sh` job gives each arm its own slot, so arms work on different tasks at any moment; the table prints each arm's 429s and cut streams.
- The qualification track (all 89 tasks) includes the 10 tune tasks and the 25 dev tasks, which were visible while summonr was adjusted. Only the 54-task board is held out.
- Time is Harbor `agent_execution` wall, not turn count.
- Cost / solved = all-trial spend / solved count (failures stay in the numerator).
