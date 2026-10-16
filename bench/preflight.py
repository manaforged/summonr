import json
import sys
from pathlib import Path

from audit import drift, expectations, observed, orphans, parity, shells, unpinned
from stats import requests
from trial import trials

job, log, settings = Path(sys.argv[1]), Path(sys.argv[2]), Path(sys.argv[3])
by_label = requests([log]) if log.is_file() else {}
rows = trials(job.parent, job.name, by_label, None)
pins = dict(line.split(" ", 1) for line in (job / "run.txt").read_text().splitlines() if " " in line)
arms = len(json.loads((job / "config.json").read_text())["agents"])
finished = [json.loads(path.read_text()) for path in job.glob("*/result.json")]
broken = [f"{data['trial_name']}: {kind}" for data in finished if (kind := (data.get("exception_info") or {}).get("exception_type")) not in (None, "AgentTimeoutError")]
if not settings.with_suffix(".proxy").is_file():
    names = {data["agent_info"]["name"] for data in finished if data.get("agent_info")}
    problems = broken + ([f"{len(names)} of {arms} arms finished a trial"] if len(names) < arms else [])
    print("\n".join(problems) if problems else "preflight ok")
    sys.exit(1 if problems else 0)
problems = broken + parity(job) + orphans(job, by_label) + unpinned(rows, by_label, pins["effort"], pins["model"])
if len({row["harness"] for row in rows}) < arms:
    problems.append(f"{len({row['harness'] for row in rows})} of {arms} arms finished a trial")
seen, differ = shells(job)
for arm, text in sorted(seen.items()):
    print(f"## {arm} shell\n{text}")
problems += differ + ([f"{len(seen)} of {arms} arms reported their shell"] if len(seen) < arms else [])
for arm in sorted({row["harness"] for row in rows}):
    calls = [call for row in rows if row["harness"] == arm for call in by_label.get(row["trial"], [])]
    if not any((call.get("status") or 0) < 400 for call in calls):
        problems.append(f"{arm}: no successful request reached the proxy")
lines = ["# arm effort max_tokens temperature, as sent"] + [" ".join((arm, *sent)) for arm, seen in sorted(observed(rows, by_label).items()) for sent in sorted(seen)]
print("\n".join(lines))
if settings.is_file():
    problems += drift(rows, by_label, expectations(settings))
elif not problems:
    settings.write_text("\n".join(lines) + "\n")
    print(f"wrote {settings}")
print("\n".join(problems) if problems else "preflight ok")
sys.exit(1 if problems else 0)
