# harness-benchmark

Fixed-model comparison of product coding CLIs. Harbor is the runner, not this suite.

Clone, pin, run, print the table. Keys stay out of git.

## Tracks

Do not dump SWE-bench Verified as the board. OpenAI stopped using it as a frontier coding score. HarnessTax used 30 Lite tasks × 3; that sample cannot see a 15-point quality gap.

| Track | Task file | k | When |
| --- | --- | --- | --- |
| Smoke | `tasks-dev.txt` (25) | 1 | wiring |
| **Tune** | `tasks-tune.txt` (10 IDs, excluded from the board) | 1 | summonr/CLI health before any board |
| **Board (terminal)** | `tasks-eval.txt` (54 IDs, frozen 2026-09-23 after dropping the 10 tuned IDs) | 5 | first public table |
| Qualification (terminal) | full Terminal-Bench 2.1 (89 IDs) | 5 | after the 64-ID table |
| Coding | DeepSWE 113, own task file, later | 5 | separate table, never averaged with TB |
| Control | SWE-bench Verified 30-ID sample, later | 3 | appendix only, labeled control |

CLI only. Two model pins, never mixed: `deepseek-flash` (default) or `glm-5.3-flash` (`BENCH_VENDOR=zai`). Same list, same seed, same defaults. Desktop/IDE apps stay off the board. GLM uses `api.z.ai` through these CLIs. Cost is priced only for vendors with a rate card in `table.py` (deepseek); other vendors report tokens only. The CLI-visible provider name selects each CLI's built-in profile for that vendor; omp's wire under provider `deepseek` is gated at smoke (a completions-wire 422 was observed once outside Harbor).

## CLIs

Wired through our Harbor adapter + logging proxy (not Harbor’s built-in `-a`):

| CLI | Status |
| --- | --- |
| summonr, pi, omp, hermes | in `tools.sh` + `harness.sh` |
| opencode2 (`@opencode/cli@2.0.10` binary `opencode2`) | v2 CLI; not v1 `opencode` |
| mcode | MiniMax Code 0.4.12 |
| claude | `@anthropic-ai/claude-code@2.1.267`; Anthropic-compat URL + DeepSeek/Z.ai key only |
| codex | `@openai/codex@0.155.1`; `model_providers.bench`, no ChatGPT login |
| grok | `@xai-official/grok@1.0.34`; `~/.grok/config.toml` custom model |
| goose | LHTB only |

Keys: isolated `HOME`, `ANTHROPIC_API_KEY` / `OPENAI_API_KEY` / `XAI_API_KEY` unset. DeepSeek or Z.ai pay-as-you-go only — not Claude/Codex/Grok subscriptions.

Tune: 10×1×9 = 90 trials.

## Run

```
printf 'DEEPSEEK_API_KEY=...\n' > deepseek.env
chmod 600 deepseek.env
./tools.sh ./tools /path/to/summonr
python3 proxy.py --bind 0.0.0.0 --port 8095 --log logs/deepseek.jsonl \
  --upstream https://api.deepseek.com --anthropic https://api.deepseek.com/anthropic
BENCH_REPEATS=1 ./run.sh tune-10 tasks-tune.txt
# GLM-5.3-Flash (glm.env with ZAI_API_KEY):
# python3 proxy.py ... --upstream https://api.z.ai/api/coding/paas/v4 --anthropic https://api.z.ai/api/anthropic
# BENCH_VENDOR=zai BENCH_REPEATS=1 ./run.sh tune-10-glm tasks-tune.txt
python3 table.py tune-10 --proxy logs/deepseek.jsonl --tasks tasks-tune.txt
# after summonr is not failing the 10:
BENCH_REPEATS=5 ./run.sh board-tb21 tasks-eval.txt
python3 table.py board-tb21 --proxy logs/deepseek.jsonl --tasks tasks-eval.txt
```

`jobs/<run>/run.txt` records seed, model, k, dataset tree hash, task-list hash, harness.sh hash, summonr binary sha256 and source commit. Rerunning a run id aborts on any recorded mismatch (seed, vendor, model, effort, repeats, harness.sh, dataset, tasks). `table.py` refuses a run without `run.txt`. It prints reliability (pass@1 / pass@k / pass^k), all / solved / failed cuts, paired ratios, and effort honored per harness, measured from proxy request bodies. A dataset-identity leak hit (terminal-bench, tbench.ai, laude-institute, marginlab.ai, harbor-framework) voids the solve; a verifier-path hit (/logs/verifier, reward.txt, /solution/, /tests/test) is a warn.

## Fairness

- Task IDs committed before looking at harness scores; the 10 tune IDs are excluded from the board list.
- Every arm runs its CLI's headless full-access mode (`--yolo`, `--dangerously-skip-permissions`, `--sandbox danger-full-access`, `--always-approve`, or summonr's uncontained default). This is a controlled constant so approval prompts never measure the benchmark; it equalizes to summonr's documented default (`--no-sandbox ... (the default)` in `summon --help`), not above it.
- `dataset.lock` pins the dataset tree hash; `run.sh` aborts on mismatch.
- Sampling controls (temperature, max_tokens, effort wire values) belong to each harness and are measured from proxy request bodies, published per arm in "Sampling controls seen" and "Effort honored". `--agent-timeout-multiplier` is fixed at 1.0 unless `BENCH_TIMEOUT_MULT` says otherwise, recorded in run.txt.
- `table.py` requires `--subject`; no harness is the default anchor. Solved rates across arms with different task coverage are not comparable; the Tasks covered column and the paired table are.
- `tools.sh` aborts if any pinned CLI entrypoint is missing from the build.
- Tokens from the logging proxy, not from a CLI self-report.
- Time is Harbor `agent_execution` wall, not turn count.
- Cost / solved = all-trial spend / solved count (failures stay in the numerator).
- Winner per column. No composite rank.
