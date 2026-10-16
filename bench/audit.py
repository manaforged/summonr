import json
from collections import Counter, defaultdict
from pathlib import Path

from stats import requests
from trial import effort_of


def clean(rows):
    dirty = {row["task"] for row in rows if row["void"]}
    return [row for row in rows if row["task"] not in dirty]


def lookups(rows, harnesses):
    lines = [
        "| Harness | Solved, lookups scored 0 | Solved, lookups counted | Voided |",
        "| --- | --- | --- | --- |",
    ]
    for harness in harnesses:
        mine = [row for row in rows if row["harness"] == harness]
        solved = [row for row in mine if row["reward"] == 1.0]
        voided = sum(1 for row in solved if row["void"])
        lines.append(f"| {harness} | {len(solved) - voided} | {len(solved)} | {voided} |")
    return "\n".join(lines)


def unjudged(rows):
    return dict(Counter(row["harness"] for row in rows if row["reward"] == 1.0 and not row["judged"]))


def broken_note(rows):
    broken = sorted({row["task"] for row in rows if row["grader"]})
    if not broken:
        return ""
    return f"Counted as misses for every harness, verifier could not install its tools: {', '.join(broken)}\n"


def flagged(rows):
    marked = [row for row in rows if row["void"] or row["warn"]]
    if not marked:
        return
    print("\nFlagged trials (void = solve removed; warn = path or source-name mention only)\n")
    for row in marked:
        kind = "void" if row["void"] else "warn"
        print(f"- {kind} {row['harness']} {row['task']} reward={row['reward']}: {', '.join(row['void'] + row['warn'])}")


def parity(job):
    seen, problems = defaultdict(lambda: defaultdict(set)), []
    for result in sorted(Path(job).glob("*/result.json")):
        data = json.loads(result.read_text())
        if not data.get("agent_execution"):
            continue
        agent = json.loads((result.parent / "config.json").read_text())["agent"]
        snapshot = result.parent / "agent" / "parity.txt"
        if not snapshot.is_file():
            problems.append(f"{result.parent.name}: the agent ran with no environment snapshot")
            continue
        seen[data["task_name"].split("/")[-1]][snapshot.read_text()].add(agent.get("name") or agent.get("import_path"))
    return problems + [f"{task}: {len(envs)} distinct environments across {sorted(set().union(*envs.values()))}" for task, envs in sorted(seen.items()) if len(envs) > 1]


def superseded(job):
    path = Path(job) / "run.txt"
    lines = path.read_text().splitlines() if path.is_file() else []
    return [line.split() for line in lines if line.startswith(("dropped ", "rerun "))]


def orphans(job, by_label):
    trials = {path.name for path in Path(job).iterdir() if path.is_dir()} | {fields[1] for fields in superseded(job)}
    return [f"proxy label {label} matches no trial in {Path(job).name} ({len(calls)} requests)" for label, calls in sorted(by_label.items()) if label not in trials]


def retasked(rows):
    sums = defaultdict(set)
    for row in rows:
        sums[row["task"]].add(row["checksum"])
    return [f"{task}: {len(found)} task checksums in one run" for task, found in sorted(sums.items()) if len(found) > 1]


def summed(items, key):
    return sum(item.get(key) or 0 for item in items)


def settings(requests, read):
    return ",".join(sorted({str(read(request)) for request in requests})) or "–"


def proxy_row(harness, calls, mine):
    usage = [call.get("usage") or {} for call in calls]
    requests = [call.get("request") or {} for call in calls]
    sent_in, told_in = summed(usage, "prompt_tokens"), summed(mine, "reported_prompt")
    ratio = f"{told_in / sent_in:.3f}" if sent_in else "–"
    limited = sum(1 for call in calls if call.get("status") == 429)
    capped = sum(1 for call in calls if call.get("finish_reason") == "length")
    return (
        f"| {harness} | {sent_in:,} | {told_in:,} | {ratio} | {summed(usage, 'completion_tokens'):,} | {summed(mine, 'reported_output'):,} | {limited} | {capped} "
        f"| {settings(requests, effort_of)} | {settings(requests, lambda r: r.get('max_tokens'))} | {settings(requests, lambda r: r.get('temperature'))} |"
    )


def proxy_check(rows, by_label):
    lines = [
        "| Harness | Proxy input | Reported input | Ratio | Proxy output | Reported output | 429s | Stopped at length | Effort sent | max_tokens sent | Temperature sent |",
        "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |",
    ]
    for harness in sorted({row["harness"] for row in rows}):
        mine = [row for row in rows if row["harness"] == harness]
        calls = [call for row in mine for call in by_label.get(row["trial"], [])]
        lines.append(proxy_row(harness, calls, mine))
    return "\n".join(lines)


def observed(rows, by_label):
    seen = {}
    for row in rows:
        for call in by_label.get(row["trial"], []):
            request = call.get("request") or {}
            if request.get("tools"):
                seen.setdefault(row["harness"], set()).add((effort_of(request), str(request.get("max_tokens")), str(request.get("temperature"))))
    return seen


def drift(rows, by_label, expected):
    found = []
    for harness, sent in sorted(observed(rows, by_label).items()):
        want = expected.get(harness, set())
        if not sent <= want:
            found.append(f"{harness}: settings file names {sorted(want)}, requests sent {sorted(sent - want)}")
    return found


def expectations(path):
    found = defaultdict(set)
    for line in Path(path).read_text().splitlines():
        if line.strip() and not line.startswith("#"):
            arm, *sent = line.split()
            found[arm].add(tuple(sent))
    return found


def refusals(jobs, runs, rows, vendor, pinned):
    problems = retasked(rows)
    settings = Path(__file__).parent / "pairs" / f"{vendor}.settings"
    for run in runs:
        log = Path(jobs).parent / "logs" / f"{run}.jsonl"
        problems += parity(Path(jobs) / run) if pinned else []
        problems += orphans(Path(jobs) / run, requests([log])) if log.is_file() else []
    if pinned and not settings.is_file():
        return problems + [f"{settings.name} is missing: run the preflight and commit the settings it writes"]
    logs = [Path(jobs).parent / "logs" / f"{run}.jsonl" for run in runs]
    by_label = requests([log for log in logs if log.is_file()])
    return problems + (drift(rows, by_label, expectations(settings)) if settings.is_file() else [])


def shells(job):
    seen = {}
    for env in Path(job).glob("*/verifier/env.txt"):
        data = json.loads((env.parent.parent / "result.json").read_text())
        seen[data["agent_info"]["name"]] = env.read_text()
    runtimes = {arm: {line for line in text.splitlines() if line.startswith("run ")} for arm, text in seen.items()}
    common = set.intersection(*runtimes.values()) if runtimes else set()
    return seen, [f"{arm} shell reaches {sorted(found - common)}" for arm, found in sorted(runtimes.items()) if found - common]


def unpinned(rows, by_label, effort, model):
    found = defaultdict(Counter)
    for row in rows:
        for call in by_label.get(row["trial"], []):
            request, served = call.get("request") or {}, call.get("response_model")
            if request.get("tools") and request.get("reasoning_effort") != effort:
                found[row["harness"]][f"tool requests sent effort {request.get('reasoning_effort')}, not {effort}"] += 1
            if served and served != model and not served.startswith(f"{model}-"):
                found[row["harness"]][f"responses came from {served}"] += 1
    return [f"{arm}: {count} {what}" for arm, kinds in sorted(found.items()) for what, count in sorted(kinds.items())]
