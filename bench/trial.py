import hashlib
import json
import re
from pathlib import Path

from stats import covered, hit_tokens, miss_tokens, seconds

SOURCES = rb"\b(?:terminal-bench|tbench|marginlab|laude-institute|harbor-framework)"
VENDOR_FILES = {"cache", "plugins", "__pycache__"}
VOID = re.compile(rb"https?:(?:\\?/){2}[^\s\"'<>]*(?:" + SOURCES + rb")|github\.com[:/](?:laude-institute|harbor-framework)", re.I)
WARN = re.compile(rb"/logs/verifier|reward\.txt|/solution/|/tests/test|terminal-bench(?!-canary)|tbench\.ai|marginlab\.ai|laude-institute|harbor-framework", re.I)
GRADER = re.compile(rb"uvx: command not found|/root/\.local/bin/env: No such file")
MEMORY = ("agent", "tree", "harness", "agent_avg", "tree_avg", "harness_avg")


def suspect(agent):
    void, warn = set(), set()
    for path in agent.rglob("*"):
        if path.is_file() and path.name != "instruction.txt" and not VENDOR_FILES & set(path.relative_to(agent).parts):
            try:
                raw = path.read_bytes()
            except OSError:
                continue
            void.update(match.group().decode().lower() for match in VOID.finditer(raw))
            warn.update(match.group().decode().lower() for match in WARN.finditer(raw))
    return sorted(void), sorted(warn)


def dig(value, *keys):
    for key in keys:
        value = (value or {}).get(key)
    return value


def reasoning_tokens(usage):
    return (usage.get("completion_tokens_details") or {}).get("reasoning_tokens", 0)


def price(hit, miss, output, rates):
    if not rates:
        return None
    return (hit * rates["hit"] + miss * rates["miss"] + output * rates["output"]) / 1_000_000


def token_fields(calls, rates):
    usage = [row.get("usage") or {} for row in calls]
    first = usage[0] if usage else {}
    hit = sum(hit_tokens(u) for u in usage)
    miss = sum(miss_tokens(u) for u in usage)
    output = sum(u.get("completion_tokens", 0) for u in usage)
    return {
        "prompt": sum(u.get("prompt_tokens", 0) for u in usage),
        "hit": hit,
        "miss": miss,
        "session_prompt": sum(u.get("prompt_tokens", 0) for u in usage[1:]),
        "session_hit": sum(hit_tokens(u) for u in usage[1:]),
        "output": output,
        "reasoning": sum(reasoning_tokens(u) for u in usage),
        "cost": price(hit, miss, output, rates),
        "first_hit": hit_tokens(first),
        "first_prompt": first.get("prompt_tokens"),
        "max_prompt": max((u.get("prompt_tokens", 0) for u in usage), default=0),
    }


def effort_of(request):
    return f'{request.get("reasoning_effort")}/{(request.get("thinking") or {}).get("type")}'


def controls_of(request):
    return f'temp={request.get("temperature")}/maxtok={request.get("max_tokens")}'


def request_fields(calls):
    return {
        "requests": len(calls),
        "errors": sum(row["status"] >= 400 for row in calls),
        "cut": sum(1 for row in calls if row.get("cut_off") and not row.get("usage")),
        "rate_limited": sum(row["status"] == 429 for row in calls),
        "effort": sorted({effort_of(row["request"]) for row in calls}),
        "controls": sorted({controls_of(row["request"]) for row in calls}),
        "models": sorted({row["response_model"] for row in calls if row.get("response_model")}),
    }


def time_fields(data, calls):
    agent = seconds(data.get("agent_execution"))
    model = covered([(row["started"], row["ended"]) for row in calls])
    return {
        "agent_seconds": agent,
        "model_seconds": model,
        "other_seconds": None if agent is None else agent - model,
    }


def memory_fields(agent):
    path = agent / "memory.txt"
    values = {}
    if path.exists():
        values = dict(line.split() for line in path.read_text().splitlines() if line.strip())
    return {f"{name}_mb": int(values[f"{name}_kb"]) / 1024 if f"{name}_kb" in values else None for name in MEMORY}


def calls_for(by_label, harness, run, data):
    label = f"{harness}.{run}.{data['trial_name']}"
    opaque = hashlib.sha256(label.encode()).hexdigest()[:16]
    return sorted(by_label.get(label, []) + by_label.get(opaque, []), key=lambda row: row["started"])


def trial_row(result, run, by_label, rates):
    data = json.loads(result.read_text())
    harness = result.parent.parent.name.split(".", 1)[0]
    calls = calls_for(by_label, harness, run, data)
    agent = result.parent / "agent"
    exit_file = agent / "exit"
    void, warn = suspect(agent)
    verifier = result.parent / "verifier" / "test-stdout.txt"
    return {
        "harness": harness,
        "task": data["task_name"].split("/")[-1],
        "trial": data["trial_name"],
        "checksum": data.get("task_checksum"),
        "reward": dig(data, "verifier_result", "rewards", "reward"),
        "exception": dig(data, "exception_info", "exception_type"),
        "exit": exit_file.read_text().strip() if exit_file.exists() else None,
        **time_fields(data, calls),
        **memory_fields(agent),
        **request_fields(calls),
        **token_fields(calls, rates),
        "void": void,
        "warn": warn,
        "grader": verifier.is_file() and bool(GRADER.search(verifier.read_bytes())),
    }


def trials(jobs, run, by_label, rates):
    return [trial_row(result, run, by_label, rates) for result in sorted(Path(jobs, run).glob("*/*/result.json"))]
