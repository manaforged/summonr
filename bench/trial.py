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
        "errors": sum(row["status"] >= 400 or row.get("finish_reason") == "failed" for row in calls),
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


def memory_fields(folder):
    path = folder / "memory.txt"
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


def step_usage(trajectory):
    usage = []
    for step in trajectory.get("steps", []):
        metrics = step.get("metrics")
        if step.get("source") == "agent" and metrics:
            usage.append({
                "prompt_tokens": metrics.get("prompt_tokens") or 0,
                "completion_tokens": metrics.get("completion_tokens") or 0,
                "prompt_tokens_details": {"cached_tokens": metrics.get("cached_tokens") or 0},
            })
    return usage


def standard_tokens(context, trajectory, rates):
    usage = step_usage(trajectory)
    prompt = context.get("n_input_tokens") or 0
    hit = context.get("n_cache_tokens") or 0
    output = context.get("n_output_tokens") or 0
    first = usage[0] if usage else {}
    return {
        "prompt": prompt,
        "hit": hit,
        "miss": prompt - hit,
        "session_prompt": sum(u["prompt_tokens"] for u in usage[1:]) if usage else None,
        "session_hit": sum(hit_tokens(u) for u in usage[1:]) if usage else None,
        "output": output,
        "reasoning": None,
        "cost": price(hit, prompt - hit, output, rates),
        "first_hit": hit_tokens(first) if usage else None,
        "first_prompt": first.get("prompt_tokens"),
        "max_prompt": max((u["prompt_tokens"] for u in usage), default=None),
    }


def standard_row(result, rates):
    data = json.loads(result.read_text())
    folder = result.parent
    agent = folder / "agent"
    path = agent / "trajectory.json"
    trajectory = json.loads(path.read_text()) if path.is_file() else {}
    steps = [step for step in trajectory.get("steps", []) if step.get("source") == "agent"]
    effort = (dig(data, "config", "agent", "kwargs") or {}).get("reasoning_effort")
    model = dig(data, "agent_info", "model_info", "name")
    void, warn = suspect(agent)
    verifier = folder / "verifier" / "test-stdout.txt"
    return {
        "harness": data["agent_info"]["name"],
        "version": data["agent_info"].get("version"),
        "task": data["task_name"].split("/")[-1],
        "trial": data["trial_name"],
        "checksum": data.get("task_checksum"),
        "reward": dig(data, "verifier_result", "rewards", "reward"),
        "exception": dig(data, "exception_info", "exception_type"),
        "exit": None,
        "agent_seconds": seconds(data.get("agent_execution")),
        "model_seconds": None,
        "other_seconds": None,
        **memory_fields(folder),
        "requests": len(steps),
        "errors": 0,
        "cut": 0,
        "rate_limited": 0,
        "effort": [f"{effort}/None"] if effort else [],
        "controls": [],
        "models": [model] if model else [],
        **standard_tokens(data.get("agent_result") or {}, trajectory, rates),
        "void": void,
        "warn": warn,
        "grader": verifier.is_file() and bool(GRADER.search(verifier.read_bytes())),
    }


def standard(jobs, run):
    return Path(jobs, run, "job.yaml").is_file()


def trials(jobs, run, by_label, rates):
    if standard(jobs, run):
        return [standard_row(result, rates) for result in sorted(Path(jobs, run).glob("*/result.json"))]
    return [trial_row(result, run, by_label, rates) for result in sorted(Path(jobs, run).glob("*/*/result.json"))]
