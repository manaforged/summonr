import json
import sys
from pathlib import Path

from stats import requests

SETUP = {"AgentSetupTimeoutError", "BaseInstallError", "HarnessInstallError"}
STOPPED = {"CancelledError"}
STANDS = {"AgentTimeoutError"}


def cut(kind, calls):
    last = calls[-1].get("status") if calls else 0
    return last == 429 or (last or 0) >= 500 or (kind == "NetworkConnectionError" and bool(calls) and last is None)


def eligible(job, by_label, skip=()):
    found = []
    for result in sorted(Path(job).glob("*/result.json")):
        if result.parent.name in skip:
            continue
        data = json.loads(result.read_text())
        kind = (data.get("exception_info") or {}).get("exception_type")
        calls = by_label.get(data["trial_name"]) or []
        solved = (((data.get("verifier_result") or {}).get("rewards") or {}).get("reward") or 0) >= 1
        if not solved and kind not in STANDS and (kind in SETUP or cut(kind, calls) or (kind and not calls)):
            found.append((data["trial_name"], kind or "-"))
    return found


def unfinished(trial):
    result = trial / "result.json"
    text = result.read_text() if result.is_file() else ""
    data = json.loads(text) if text.strip() else {}
    kind = (data.get("exception_info") or {}).get("exception_type")
    return kind in STOPPED or (not kind and not data.get("verifier_result"))


def aside(job, by_label, rerun):
    job = Path(job)
    dropped = [path.name for path in sorted(job.iterdir()) if path.is_dir() and unfinished(path)]
    reruns = [f"rerun {name} {kind}" for name, kind in eligible(job, by_label, dropped)] if rerun else []
    return [f"dropped {name}" for name in dropped] + reruns


def set_aside(job, by_label, rerun):
    move(Path(job), aside(job, by_label, rerun))


def move(job, lines):
    aside = job.with_name(f"{job.name}.rerun")
    for line in lines:
        aside.mkdir(exist_ok=True)
        (job / line.split()[1]).rename(aside / line.split()[1])
    with open(job / "run.txt", "a") as out:
        out.writelines(line + "\n" for line in lines)

if __name__ == "__main__":
    log = Path(sys.argv[2])
    lines = aside(sys.argv[1], requests([log]) if log.is_file() else {}, "--rerun" in sys.argv[3:])
    if "--move" in sys.argv[3:]:
        move(Path(sys.argv[1]), lines)
    else:
        print("\n".join(lines))
