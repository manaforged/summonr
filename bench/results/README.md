# Terminal-Bench 2.1 trial summaries

These CSV files contain numerical trial summaries, not provider requests, credentials, or conversation transcripts. They are historical measurements, not measurements of the current source revision.

| File | Source job | Selection | Rows |
| --- | --- | --- | ---: |
| `chatgpt-first3.csv` | `tb21-chatgpt-1013t` | Earliest three trials per harness and task | 534 |
| `chatgpt-all5.csv` | `tb21-chatgpt-1013t` | All five trials per harness and task | 890 |
| `supergrok-first3.csv` | `tb21-supergrok-1013t` | Earliest three trials per harness and task | 534 |

Each pair covers 89 tasks. Selection orders each harness/task group by its recorded `started_at`, then takes the first three. It does not select by reward. The original job manifests specify five attempts; the Grok job did not finish all five attempts on every task.

## Interpretation

`reward` is the verifier's raw result. A scored solve requires `reward == 1` and `voided == False`. `judged == False` means the separate judge pass has not run; the source-lookup scan alone determines the recorded voids. These files must not be described as fully judged results.

`prompt`, `hit`, `miss`, and `output` are the harness-reported token counters. They are not an independent proxy measurement. Blank values mean unavailable, not zero. Cost is an API-list-price estimate, not a subscription charge. Grok costs are omitted when per-request prompt sizes needed for tiered billing are unavailable.

Memory fields are in MB as reported by the scorer (KiB divided by 1024). `agent_mb` is a trial's highest sampled agent RSS; `harness_avg_mb` is its mean harness RSS, including helpers; `harness_mb` is its peak harness RSS. The published typical memory figure is the median of trial means. It is neither a maximum nor an allocation limit. Sampling does not cover every trial.

The task checksum identifies the task inputs. The `version` field is the harness's reported package version, not an immutable source identity. These jobs used multiple Summonr binaries. The recorded binary SHA-256 values are:

```text
4c1eb43f254fb29a836cb5ab2b5d220bdfca497418d539a36a68bbe3b1643e62
dc4bdd5e8e2325141bf00941e597e5609592157203ba599d52535e33de96333e
23c626a16729bb438ed7c5fed61aa90957148820c01525b953ae32d653cbf7e6
8772e3c7d81c43fb345ecb4bfd4415c6259235f47a08488ef73384115c487f27
```

The archives do not establish one immutable Summonr build for the entire comparison. Do not use these results as qualification of a different build.

## Reproduction

The published three-trial tables were reproduced with `bench/table.py` at `dbf2e1b2`. Its SHA-256 is `c8b8705430ce524e7a6277c3c4d2403dd52a08a38e8b459b0e29c5e379bab01e`. The current `bench/table.py --csv` exporter adds the `voided` flag and uses CSV quoting while retaining the numerical fields.

With the original archives and earliest-three subsets present:

```sh
python3 bench/table.py judge3-tb21-chatgpt-1013t --subject summonr --csv chatgpt-first3.csv
python3 bench/table.py tb21-chatgpt-1013t --subject summonr --csv chatgpt-all5.csv
python3 bench/table.py judge3-tb21-supergrok-1013t --subject summonr --csv supergrok-first3.csv
```

The original archives are not included. The CSV files allow recalculation of solve counts, descriptive statistics, and paired task-bootstrap comparisons; they do not allow independent reinspection of source lookups, judge decisions, or task execution. Recalculate the full-five comparison from this checkout with the existing statistics owner:

```sh
python3 - <<'PY'
import csv
import sys
sys.path.insert(0, 'bench')
from paired import paired
with open('bench/results/chatgpt-all5.csv', newline='') as source:
    rows = list(csv.DictReader(source))
for row in rows:
    row['void'] = row['voided'] == 'True'
    for key in ('reward', 'prompt', 'miss', 'output', 'cost', 'agent_seconds', 'first_prompt', 'harness_avg_mb', 'harness_mb'):
        row[key] = float(row[key]) if row[key] else None
print(paired(rows, 'summonr'))
PY
```

The full-five ChatGPT comparison retains the input-token advantage but not a statistically established solve lead: the paired solve difference is 0.03 (-0.01 to 0.08). See [the benchmark report](../../BENCHMARKS.md) for all published qualifications.
