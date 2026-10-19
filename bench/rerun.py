import json
import sys
from pathlib import Path

from stats import requests

SETUP = {"AgentSetupTimeoutError"}
STOPPED = {"CancelledError"}
PROVIDER = {"ApiRateLimitError": lambda status: status == 429, "NetworkConnectionError": lambda status: status is None or status >= 500}


def eligible(job, by_label):
    found = []
    for result in sorted(Path(job).glob("*/result.json")):
        data = json.loads(result.read_text())
        kind = (data.get("exception_info") or {}).get("exception_type")
        calls = by_label.get(data["trial_name"])
        seen = kind in PROVIDER and any(PROVIDER[kind](call.get("status")) for call in calls or [])
        if kind and (kind in SETUP or seen or not calls):
            found.append((data["trial_name"], kind))
    return found


def unfinished(trial):
    result = trial / "result.json"
    text = result.read_text() if result.is_file() else ""
    data = json.loads(text) if text.strip() else {}
    kind = (data.get("exception_info") or {}).get("exception_type")
    return kind in STOPPED or (not kind and not data.get("verifier_result"))


def set_aside(job, by_label, rerun):
    job = Path(job)
    move(job, [f"dropped {path.name}" for path in sorted(job.iterdir()) if path.is_dir() and unfinished(path)])
    if rerun:
        move(job, [f"rerun {name} {kind}" for name, kind in eligible(job, by_label)])


def move(job, lines):
    aside = job.with_name(f"{job.name}.rerun")
    for line in lines:
        aside.mkdir(exist_ok=True)
        (job / line.split()[1]).rename(aside / line.split()[1])
    with open(job / "run.txt", "a") as out:
        out.writelines(line + "\n" for line in lines)

if __name__ == "__main__":
    log = Path(sys.argv[2])
    set_aside(sys.argv[1], requests([log]) if log.is_file() else {}, len(sys.argv) > 3 and sys.argv[3] == "--rerun")
