import json
import sys
from pathlib import Path

from stats import requests

SETUP = {"AgentSetupTimeoutError"}
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


if __name__ == "__main__":
    log = Path(sys.argv[2])
    for name, kind in eligible(sys.argv[1], requests([log]) if log.is_file() else {}):
        print(name, kind)
