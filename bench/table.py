#!/usr/bin/env python3
import argparse
import hashlib
import re
import json
import math
import random
import statistics
from collections import defaultdict
from datetime import datetime
from pathlib import Path

RATES = {"hit": 0.006, "miss": 0.30, "output": 1.20}
RATE_CARD = "DeepSeek V4.1 Flash peak list price, USD per 1M tokens, api-docs.deepseek.com/quick_start/pricing, 2026-09-16"
HARNESSES = ["summonr", "pi", "omp", "opencode", "hermes", "goose"]
SUSPECT = re.compile(rb"terminal-bench|harbor-framework|laude-institute|tbench\.ai|marginlab\.ai|/logs/verifier|reward\.txt|/solution/|/tests/test", re.I)


def suspect(agent):
    hits = set()
    for path in agent.rglob("*"):
        if path.is_file() and path.name != "instruction.txt":
            hits.update(match.group().decode().lower() for match in SUSPECT.finditer(path.read_bytes()))
    return sorted(hits)


def seconds(span):
    if not span or not span.get("started_at") or not span.get("finished_at"):
        return None
    start = datetime.fromisoformat(span["started_at"].replace("Z", "+00:00"))
    end = datetime.fromisoformat(span["finished_at"].replace("Z", "+00:00"))
    return (end - start).total_seconds()


def wilson(passed, total, z=1.96):
    if total == 0:
        return (0.0, 0.0)
    p = passed / total
    denominator = 1 + z * z / total
    center = (p + z * z / (2 * total)) / denominator
    margin = z * math.sqrt(p * (1 - p) / total + z * z / (4 * total * total)) / denominator
    return (max(0.0, center - margin), min(1.0, center + margin))


def interval(values, seed=20260916, samples=10000):
    if not values:
        return (None, None, None)
    rng = random.Random(seed)
    medians = sorted(
        statistics.median(rng.choice(values) for _ in values) for _ in range(samples)
    )
    return (statistics.median(values), medians[int(0.025 * samples)], medians[int(0.975 * samples) - 1])


def covered(spans):
    total, end = 0.0, None
    for start, stop in sorted(spans):
        if end is None or start > end:
            total += stop - start
            end = stop
        elif stop > end:
            total += stop - end
            end = stop
    return total


def requests(paths):
    by_label = defaultdict(list)
    for path in paths:
        for line in open(path):
            row = json.loads(line)
            by_label[row["label"]].append(row)
    return by_label


def trials(jobs, run, by_label):
    rows = []
    for result in sorted(Path(jobs, run).glob("*/*/result.json")):
        data = json.loads(result.read_text())
        harness = result.parent.parent.name.split(".", 1)[0]
        label = f"{harness}.{run}.{data['trial_name']}"
        opaque = hashlib.sha256(label.encode()).hexdigest()[:16]
        calls = sorted(by_label.get(label, []) + by_label.get(opaque, []), key=lambda row: row["started"])
        usage = [row.get("usage") or {} for row in calls]
        prompt = sum(u.get("prompt_tokens", 0) for u in usage)
        hit = sum(u.get("prompt_cache_hit_tokens", 0) for u in usage)
        miss = sum(u.get("prompt_cache_miss_tokens", 0) for u in usage)
        output = sum(u.get("completion_tokens", 0) for u in usage)
        reasoning = sum((u.get("completion_tokens_details") or {}).get("reasoning_tokens", 0) for u in usage)
        agent = seconds(data.get("agent_execution"))
        model = covered([(row["started"], row["ended"]) for row in calls])
        first = calls[0] if calls else None
        exit_file = result.parent / "agent" / "exit"
        rows.append(
            {
                "harness": harness,
                "task": data["task_name"].split("/")[-1],
                "trial": data["trial_name"],
                "checksum": data.get("task_checksum"),
                "reward": ((data.get("verifier_result") or {}).get("rewards") or {}).get("reward"),
                "exception": (data.get("exception_info") or {}).get("exception_type"),
                "exit": exit_file.read_text().strip() if exit_file.exists() else None,
                "agent_seconds": agent,
                "model_seconds": model,
                "other_seconds": (agent - model) if agent is not None else None,
                "requests": len(calls),
                "errors": sum(1 for row in calls if row["status"] >= 400),
                "rate_limited": sum(1 for row in calls if row["status"] == 429),
                "prompt": prompt,
                "hit": hit,
                "miss": miss,
                "output": output,
                "reasoning": reasoning,
                "cost": (hit * RATES["hit"] + miss * RATES["miss"] + output * RATES["output"]) / 1_000_000,
                "first_hit": ((first or {}).get("usage") or {}).get("prompt_cache_hit_tokens"),
                "first_prompt": ((first or {}).get("usage") or {}).get("prompt_tokens"),
                "effort": sorted({str(row["request"].get("reasoning_effort")) for row in calls}),
                "models": sorted({str(row.get("response_model")) for row in calls}),
                "max_prompt": max((u.get("prompt_tokens", 0) for u in usage), default=0),
                "suspect": suspect(result.parent / "agent"),
            }
        )
    return rows


def fmt(value, digits=0):
    if value is None:
        return "–"
    return f"{value:,.{digits}f}"


def cell(values, digits=0):
    median, low, high = interval(values)
    if median is None:
        return "–"
    return f"{fmt(median, digits)} ({fmt(low, digits)}–{fmt(high, digits)})"


def summary(rows, solved_only=False):
    lines = [
        "| Harness | Solved | Input tokens / task | Uncached input / task | Output tokens / task | Cache hit | Cost / task | Time / task (s) | Requests / task |",
        "| --- | --- | --- | --- | --- | --- | --- | --- | --- |",
    ]
    for harness in HARNESSES:
        mine = [row for row in rows if row["harness"] == harness]
        if not mine:
            continue
        passed = sum(1 for row in mine if row["reward"] == 1.0)
        low, high = wilson(passed, len(mine))
        scope = [row for row in mine if row["reward"] == 1.0] if solved_only else mine
        prompt = sum(row["prompt"] for row in scope)
        hit = sum(row["hit"] for row in scope)
        lines.append(
            "| {} | {}/{} ({:.0%}, {:.0%}–{:.0%}) | {} | {} | {} | {} | {} | {} | {} |".format(
                harness,
                passed,
                len(mine),
                passed / len(mine),
                low,
                high,
                cell([row["prompt"] for row in scope]),
                cell([row["miss"] for row in scope]),
                cell([row["output"] for row in scope]),
                f"{hit / prompt:.1%}" if prompt else "–",
                cell([row["cost"] for row in scope], 4),
                cell([row["agent_seconds"] for row in scope if row["agent_seconds"] is not None], 1),
                cell([row["requests"] for row in scope]),
            )
        )
    return "\n".join(lines)


def breakdown(rows):
    lines = [
        "| Harness | Model time share | Tool and harness time share | Reasoning share of output | Cold first-request cache hit | Max prompt tokens | Errors | 429s | Effort sent | Failures | Suspect trials |",
        "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |",
    ]
    for harness in HARNESSES:
        mine = [row for row in rows if row["harness"] == harness]
        if not mine:
            continue
        agent = sum(row["agent_seconds"] or 0 for row in mine)
        model = sum(row["model_seconds"] for row in mine)
        output = sum(row["output"] for row in mine)
        reasoning = sum(row["reasoning"] for row in mine)
        firsts = [row for row in mine if row["first_prompt"]]
        cold = sum(row["first_hit"] or 0 for row in firsts) / max(1, sum(row["first_prompt"] for row in firsts))
        failures = defaultdict(int)
        for row in mine:
            if row["reward"] == 1.0:
                continue
            reason = row["exception"] or ("no requests" if row["requests"] == 0 else f"exit {row['exit']}" if row["exit"] not in (None, "0") else "wrong answer")
            failures[reason] += 1
        efforts = sorted({effort for row in mine for effort in row["effort"]})
        lines.append(
            "| {} | {} | {} | {} | {:.1%} | {} | {} | {} | {} | {} | {} |".format(
                harness,
                f"{model / agent:.0%}" if agent else "–",
                f"{(agent - model) / agent:.0%}" if agent else "–",
                f"{reasoning / output:.0%}" if output else "–",
                cold,
                fmt(max(row["max_prompt"] for row in mine)),
                sum(row["errors"] for row in mine),
                sum(row["rate_limited"] for row in mine),
                ", ".join(efforts),
                "; ".join(f"{reason} {count}" for reason, count in sorted(failures.items())) or "none",
                sum(1 for row in mine if row["suspect"]),
            )
        )
    return "\n".join(lines)


PAIRED = [("input tokens", "prompt"), ("uncached input", "miss"), ("output tokens", "output"), ("cost", "cost"), ("time", "agent_seconds")]


def per_task(rows, harness):
    tasks = defaultdict(list)
    for row in rows:
        if row["harness"] == harness:
            tasks[row["task"]].append(row)
    return tasks


def paired(rows, subject="summonr", seed=20260916, samples=10000):
    mine = per_task(rows, subject)
    lines = [
        f"Ratios are {subject} / other, geometric mean over tasks both ran, 95% bootstrap over tasks. Below 1 means {subject} used less.",
        "",
        "| Other | Tasks | Solved difference | " + " | ".join(name for name, _ in PAIRED) + " |",
        "| --- | --- | --- | " + " | ".join("---" for _ in PAIRED) + " |",
    ]
    rng = random.Random(seed)
    for other in HARNESSES:
        if other == subject:
            continue
        theirs = per_task(rows, other)
        tasks = sorted(set(mine) & set(theirs))
        if not tasks:
            continue
        solved = {task: statistics.mean(r["reward"] == 1.0 for r in mine[task]) - statistics.mean(r["reward"] == 1.0 for r in theirs[task]) for task in tasks}
        logs = {}
        for name, key in PAIRED:
            logs[name] = {}
            for task in tasks:
                a = [r[key] for r in mine[task] if r[key]]
                b = [r[key] for r in theirs[task] if r[key]]
                if a and b:
                    logs[name][task] = math.log(statistics.mean(a) / statistics.mean(b))
        draws = [[rng.choice(tasks) for _ in tasks] for _ in range(samples)]
        def ci(values, transform):
            keys = list(values)
            if not keys:
                return "–"
            point = transform(statistics.mean(values.values()))
            means = sorted(transform(statistics.mean(values[t] for t in draw if t in values)) for draw in draws if any(t in values for t in draw))
            return f"{point:.2f} ({means[int(0.025 * len(means))]:.2f}–{means[int(0.975 * len(means)) - 1]:.2f})"
        cells = [ci(logs[name], math.exp) for name, _ in PAIRED]
        lines.append(f"| {other} | {len(tasks)} | {ci(solved, lambda v: v)} | " + " | ".join(cells) + " |")
    return "\n".join(lines)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("run")
    parser.add_argument("--jobs", default=str(Path(__file__).parent / "jobs"))
    parser.add_argument("--proxy", nargs="+", default=[str(Path(__file__).parent / "logs" / "smoke.jsonl")])
    parser.add_argument("--csv")
    parser.add_argument("--tasks")
    args = parser.parse_args()
    rows = trials(args.jobs, args.run, requests(args.proxy))
    if args.tasks:
        keep = {line.strip() for line in open(args.tasks) if line.strip()}
        rows = [row for row in rows if row["task"] in keep]
    models = sorted({model for row in rows for model in row["models"]})
    checksums = len({row["checksum"] for row in rows})
    print(f"Run `{args.run}`: {len(rows)} trials, response models {models}, {checksums} task checksums.")
    print(f"Cost: {RATE_CARD}. Intervals: 95% Wilson for solved, bootstrap median (10,000 resamples, seed 20260916) elsewhere.\n")
    print("All tasks\n")
    print(summary(rows))
    print("\nSolved tasks only\n")
    print(summary(rows, solved_only=True))
    print("\nWhere the time and tokens go\n")
    print(breakdown(rows))
    print("\nPaired against summonr\n")
    print(paired(rows))
    flagged = [row for row in rows if row["suspect"]]
    if flagged:
        print("\nSuspect trials (judge before scoring)\n")
        for row in flagged:
            print(f"- {row['harness']} {row['task']} reward={row['reward']}: {', '.join(row['suspect'])}")
    if args.csv:
        keys = [key for key in rows[0] if key not in ("effort", "models")] if rows else []
        with open(args.csv, "w") as out:
            out.write(",".join(keys) + "\n")
            for row in rows:
                out.write(",".join(str(row[key]) for key in keys) + "\n")


if __name__ == "__main__":
    main()
