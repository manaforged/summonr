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

RATES = {"deepseek": {"hit": 0.006, "miss": 0.30, "output": 1.20}}
RATE_CARD = {"deepseek": "DeepSeek Flash peak list price (cache hit $0.006, miss $0.30, output $1.20 per 1M tokens), api-docs.deepseek.com/quick_start/pricing, 2026-09-19; off-peak is half; list price, not plan spend"}
def order(rows, subject=None):
    names = sorted({row["harness"] for row in rows})
    if subject in names:
        names.remove(subject)
        names.insert(0, subject)
    return names
VOID = re.compile(rb"terminal-bench|harbor-framework|laude-institute|tbench\.ai|marginlab\.ai", re.I)
WARN = re.compile(rb"/logs/verifier|reward\.txt|/solution/|/tests/test", re.I)


def suspect(agent):
    void, warn = set(), set()
    for path in agent.rglob("*"):
        if path.is_file() and path.name != "instruction.txt":
            raw = path.read_bytes()
            void.update(match.group().decode().lower() for match in VOID.finditer(raw))
            warn.update(match.group().decode().lower() for match in WARN.finditer(raw))
    return sorted(void), sorted(warn)


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


def hit_tokens(usage):
    if "prompt_cache_hit_tokens" in usage:
        return usage["prompt_cache_hit_tokens"]
    return (usage.get("prompt_tokens_details") or {}).get("cached_tokens") or 0


def miss_tokens(usage):
    if "prompt_cache_miss_tokens" in usage:
        return usage["prompt_cache_miss_tokens"]
    return usage.get("prompt_tokens", 0) - hit_tokens(usage)


def requests(paths):
    by_label = defaultdict(list)
    for path in paths:
        for line in open(path):
            row = json.loads(line)
            if "label" in row:
                by_label[row["label"]].append(row)
    return by_label


def meta(jobs, run):
    kv = {}
    path = Path(jobs, run, "run.txt")
    if not path.is_file() or not path.read_text().strip():
        raise SystemExit(f"{path} missing or empty: run it with run.sh before tabulating")
    for line in path.read_text().splitlines():
        key, _, value = line.partition(" ")
        kv[key.strip()] = value.strip()
    return kv


def trials(jobs, run, by_label, rates):
    rows = []
    for result in sorted(Path(jobs, run).glob("*/*/result.json")):
        data = json.loads(result.read_text())
        harness = result.parent.parent.name.split(".", 1)[0]
        label = f"{harness}.{run}.{data['trial_name']}"
        opaque = hashlib.sha256(label.encode()).hexdigest()[:16]
        calls = sorted(by_label.get(label, []) + by_label.get(opaque, []), key=lambda row: row["started"])
        usage = [row.get("usage") or {} for row in calls]
        prompt = sum(u.get("prompt_tokens", 0) for u in usage)
        hit = sum(hit_tokens(u) for u in usage)
        miss = sum(miss_tokens(u) for u in usage)
        output = sum(u.get("completion_tokens", 0) for u in usage)
        reasoning = sum((u.get("completion_tokens_details") or {}).get("reasoning_tokens", 0) for u in usage)
        agent = seconds(data.get("agent_execution"))
        model = covered([(row["started"], row["ended"]) for row in calls])
        first = calls[0] if calls else None
        rest = [row.get("usage") or {} for row in calls[1:]]
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
                "session_prompt": sum(u.get("prompt_tokens", 0) for u in rest),
                "session_hit": sum(hit_tokens(u) for u in rest),
                "output": output,
                "reasoning": reasoning,
                "cost": (hit * rates["hit"] + miss * rates["miss"] + output * rates["output"]) / 1_000_000 if rates else None,
                "first_hit": hit_tokens((first or {}).get("usage") or {}),
                "first_prompt": ((first or {}).get("usage") or {}).get("prompt_tokens"),
                "effort": sorted({f'{row["request"].get("reasoning_effort")}/{(row["request"].get("thinking") or {}).get("type")}' for row in calls}),
                "controls": sorted({f'temp={row["request"].get("temperature")}/maxtok={row["request"].get("max_tokens")}' for row in calls}),
                "models": sorted({row["response_model"] for row in calls if row.get("response_model")}),
                "max_prompt": max((u.get("prompt_tokens", 0) for u in usage), default=0),
                "void": suspect(result.parent / "agent")[0],
                "warn": suspect(result.parent / "agent")[1],
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


def summary(rows, scope="all"):
    lines = [
        "| Harness | Solved | Tasks covered | Input tokens / task | Uncached input / task | Output tokens / task | Cache hit | Session cache | Cost / task | Time / task (s) | Requests / task |",
        "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |",
    ]
    for harness in order(rows):
        mine = [row for row in rows if row["harness"] == harness]
        if not mine:
            continue
        if scope == "solved":
            mine = [row for row in mine if row["reward"] == 1.0 and not row["void"]]
        elif scope == "failed":
            mine = [row for row in mine if row["reward"] != 1.0 or row["void"]]
        if not mine:
            continue
        passed = sum(1 for row in mine if row["reward"] == 1.0 and not row["void"])
        low, high = wilson(passed, len(mine))
        prompt = sum(row["prompt"] for row in mine)
        hit = sum(row["hit"] for row in mine)
        sess_p = sum(row["session_prompt"] for row in mine)
        sess_h = sum(row["session_hit"] for row in mine)
        lines.append(
            "| {} | {}/{} ({:.0%}, {:.0%}–{:.0%}) | {} | {} | {} | {} | {} | {} | {} | {} | {} |".format(
                harness,
                passed,
                len(mine),
                passed / len(mine),
                low,
                high,
                len({row["task"] for row in mine}),
                cell([row["prompt"] for row in mine]),
                cell([row["miss"] for row in mine]),
                cell([row["output"] for row in mine]),
                f"{hit / prompt:.1%}" if prompt else "–",
                f"{sess_h / sess_p:.1%}" if sess_p else "–",
                cell([row["cost"] for row in mine if row["cost"] is not None], 4),
                cell([row["agent_seconds"] for row in mine if row["agent_seconds"] is not None], 1),
                cell([row["requests"] for row in mine]),
            )
        )
    return "\n".join(lines)


def reliability(rows):
    lines = [
        "| Harness | Tasks | Pass@1 | Pass@k | Pass^k | Cost / solved | First-request tokens |",
        "| --- | --- | --- | --- | --- | --- | --- |",
    ]
    for harness in order(rows):
        by = per_task(rows, harness)
        if not by:
            continue
        n = len(by)

        def first_ok(group):
            ordered = sorted(group, key=lambda row: row["trial"])
            return bool(ordered) and ordered[0]["reward"] == 1.0 and not ordered[0]["void"]

        p1 = sum(1 for group in by.values() if first_ok(group))
        pk = sum(1 for group in by.values() if any(row["reward"] == 1.0 and not row["void"] for row in group))
        hat = sum(1 for group in by.values() if group and all(row["reward"] == 1.0 and not row["void"] for row in group))
        spend = sum(row["cost"] or 0 for group in by.values() for row in group)
        wins = sum(1 for group in by.values() for row in group if row["reward"] == 1.0 and not row["void"])
        l1, h1 = wilson(p1, n)
        lk, hk = wilson(pk, n)
        lh, hh = wilson(hat, n)
        firsts = [row["first_prompt"] for group in by.values() for row in group if row.get("first_prompt")]
        lines.append(
            "| {} | {} | {}/{} ({:.0%}, {:.0%}–{:.0%}) | {}/{} ({:.0%}, {:.0%}–{:.0%}) | {}/{} ({:.0%}, {:.0%}–{:.0%}) | {} | {} |".format(
                harness,
                n,
                p1,
                n,
                p1 / n,
                l1,
                h1,
                pk,
                n,
                pk / n,
                lk,
                hk,
                hat,
                n,
                hat / n,
                lh,
                hh,
                f"{spend / wins:.4f}" if wins and any(row["cost"] for group in by.values() for row in group) else "–",
                cell(firsts),
            )
        )
    return "\n".join(lines)


def breakdown(rows, want_effort=None):
    lines = [
        "| Harness | Model time share | Tool and harness time share | Reasoning share of output | Cold first-request cache hit | Max prompt tokens | Errors | 429s | Effort sent | Effort honored | Sampling controls seen | Failures | Voided | Warn |",
        "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |",
    ]
    for harness in order(rows):
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
        honored = "n/a" if want_effort in ("", "default", None) else (
            "yes" if all(want_effort in {e.split("/", 1)[0] for e in row["effort"]} for row in mine)
            else "no" if not any(want_effort in {e.split("/", 1)[0] for e in row["effort"]} for row in mine)
            else "mixed")
        lines.append(
            "| {} | {} | {} | {} | {:.1%} | {} | {} | {} | {} | {} | {} | {} | {} | {} |".format(
                harness,
                f"{model / agent:.0%}" if agent else "–",
                f"{(agent - model) / agent:.0%}" if agent else "–",
                f"{reasoning / output:.0%}" if output else "–",
                cold,
                fmt(max(row["max_prompt"] for row in mine)),
                sum(row["errors"] for row in mine),
                sum(row["rate_limited"] for row in mine),
                ", ".join(efforts),
                honored,
                "; ".join(sorted({c for row in mine for c in row["controls"]})),
                "; ".join(f"{reason} {count}" for reason, count in sorted(failures.items())) or "none",
                sum(1 for row in mine if row["void"]),
                sum(1 for row in mine if row["warn"]),
            )
        )
    return "\n".join(lines)


PAIRED = [
    ("input tokens", "prompt"),
    ("uncached input", "miss"),
    ("output tokens", "output"),
    ("cost", "cost"),
    ("time", "agent_seconds"),
    ("first-request tokens", "first_prompt"),
]


def per_task(rows, harness):
    tasks = defaultdict(list)
    for row in rows:
        if row["harness"] == harness:
            tasks[row["task"]].append(row)
    return tasks


def paired(rows, subject, seed=20260916, samples=10000):
    mine = per_task(rows, subject)
    lines = [
        f"Ratios are {subject} / other, geometric mean over tasks both ran, 95% bootstrap over tasks. Below 1 means {subject} used less.",
        "",
        "| Other | Tasks | Solved difference | " + " | ".join(name for name, _ in PAIRED) + " |",
        "| --- | --- | --- | " + " | ".join("---" for _ in PAIRED) + " |",
    ]
    rng = random.Random(seed)
    for other in order(rows, subject):
        if other == subject:
            continue
        theirs = per_task(rows, other)
        tasks = sorted(set(mine) & set(theirs))
        if not tasks:
            continue
        solved = {task: statistics.mean(r["reward"] == 1.0 and not r["void"] for r in mine[task]) - statistics.mean(r["reward"] == 1.0 and not r["void"] for r in theirs[task]) for task in tasks}
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
    parser.add_argument("--proxy", nargs="+", default=[str(Path(__file__).parent / "logs" / "deepseek.jsonl")])
    parser.add_argument("--csv")
    parser.add_argument("--subject", required=True)
    parser.add_argument("--tasks")
    args = parser.parse_args()
    info = meta(args.jobs, args.run)
    vendor = info.get("vendor", "deepseek")
    rates = RATES.get(vendor)
    by_label = requests(args.proxy)
    pin = info.get("model")
    if not pin:
        raise SystemExit("run.txt records no pinned model")
    rows = trials(args.jobs, args.run, by_label, rates)
    if args.tasks:
        keep = {line.strip() for line in open(args.tasks) if line.strip()}
        rows = [row for row in rows if row["task"] in keep]
    wrong = sorted({f'{row["harness"]}: {model}' for row in rows for model in row["models"] if model != pin})
    if wrong:
        raise SystemExit(f"response model is not the pinned {pin}: {', '.join(wrong)}")
    unseen = sorted({row["harness"] for row in rows if row["requests"] == 0 and row["reward"] == 1.0})
    if unseen:
        raise SystemExit(f"solved with no request in the proxy log (proxy bypassed or wrong --proxy file): {', '.join(unseen)}")
    blind = sorted({row["harness"] for row in rows if row["requests"] > row["errors"] and row["prompt"] == 0})
    if blind:
        raise SystemExit(f"requests succeeded but the proxy log holds no token usage for: {', '.join(blind)}")
    models = sorted({model for row in rows for model in row["models"]})
    checksums = len({row["checksum"] for row in rows})
    print(f"Run `{args.run}`: vendor {vendor}, effort {info.get('effort', 'default')}, {len(rows)} trials, response models {models}, {checksums} task checksums.")
    print((RATE_CARD.get(vendor, f"Cost: no rate card for vendor {vendor}; tokens only.") if rates else "Cost: suppressed for this run; tokens only.") + " Intervals: 95% Wilson for solved, bootstrap median (10,000 resamples, seed 20260916) elsewhere. A dataset-identity leak hit voids the solve; a verifier-path hit is a warn.")
    print("Reliability (task-level; pass@k = any of k, pass^k = all k)\n")
    print(reliability(rows))
    print("\nAll trials\n")
    print(summary(rows))
    print("\nSolved trials only\n")
    print(summary(rows, scope="solved"))
    print("\nFailed trials only\n")
    print(summary(rows, scope="failed"))
    print("\nWhere the time and tokens go\n")
    print(breakdown(rows, info.get("effort")))
    print(f"\nPaired against {args.subject}\n")
    print(paired(rows, args.subject))
    flagged = [row for row in rows if row["void"] or row["warn"]]
    if flagged:
        print("\nFlagged trials (void = solve removed; warn = path mention only)\n")
        for row in flagged:
            kind = "void" if row["void"] else "warn"
            print(f"- {kind} {row['harness']} {row['task']} reward={row['reward']}: {', '.join(row['void'] + row['warn'])}")
    if args.csv:
        keys = [key for key in rows[0] if key not in ("effort", "models", "void", "warn", "controls")] if rows else []
        with open(args.csv, "w") as out:
            out.write(",".join(keys) + "\n")
            for row in rows:
                out.write(",".join(str(row[key]) for key in keys) + "\n")


if __name__ == "__main__":
    main()
