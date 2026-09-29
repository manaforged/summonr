#!/usr/bin/env python3
import argparse
import math
import random
import statistics
from collections import Counter, defaultdict
from pathlib import Path

from stats import cell, fmt, requests, wilson
from trial import trials

RATES = {"deepseek": {"hit": 0.006, "miss": 0.30, "output": 1.20}}
RATE_CARD = {"deepseek": "DeepSeek Flash peak list price (cache hit $0.006, miss $0.30, output $1.20 per 1M tokens), api-docs.deepseek.com/quick_start/pricing, 2026-09-19; off-peak is half; list price, not plan spend"}
PAIRED = [
    ("input tokens", "prompt"),
    ("uncached input", "miss"),
    ("output tokens", "output"),
    ("cost", "cost"),
    ("time", "agent_seconds"),
    ("first-request tokens", "first_prompt"),
]
UNSET = ("", "default", None)


def order(rows, subject=None):
    names = sorted({row["harness"] for row in rows})
    if subject in names:
        names.remove(subject)
        names.insert(0, subject)
    return names


def meta(jobs, run):
    kv = {}
    path = Path(jobs, run, "run.txt")
    if not path.is_file() or not path.read_text().strip():
        raise SystemExit(f"{path} missing or empty: run it with run.sh before tabulating")
    for line in path.read_text().splitlines():
        key, _, value = line.partition(" ")
        kv[key.strip()] = value.strip()
    return kv


def ok(row):
    return row["reward"] == 1.0 and not row["void"]


def failed(row):
    return not ok(row)


def rate(passed, total):
    low, high = wilson(passed, total)
    return f"{passed}/{total} ({passed / total:.0%}, {low:.0%}–{high:.0%})"


def share(part, whole, spec):
    return format(part / whole, spec) if whole else "–"


def column(rows, key, digits=0):
    return cell([row[key] for row in rows if row[key] is not None], digits)


def total(rows, key):
    return sum(row[key] for row in rows)


def table_row(values):
    return "| " + " | ".join(str(value) for value in values) + " |"


def per_task(rows, harness):
    tasks = defaultdict(list)
    for row in rows:
        if row["harness"] == harness:
            tasks[row["task"]].append(row)
    return tasks


SCOPES = {"all": lambda row: True, "solved": ok, "failed": failed}


def summary_line(harness, mine):
    return table_row([
        harness,
        rate(sum(map(ok, mine)), len(mine)),
        len({row["task"] for row in mine}),
        column(mine, "prompt"),
        column(mine, "miss"),
        column(mine, "output"),
        share(total(mine, "hit"), total(mine, "prompt"), ".1%"),
        share(total(mine, "session_hit"), total(mine, "session_prompt"), ".1%"),
        column(mine, "cost", 4),
        column(mine, "agent_seconds", 1),
        column(mine, "requests"),
    ])


def summary(rows, scope="all"):
    lines = [
        "| Harness | Solved | Tasks covered | Input tokens / task | Uncached input / task | Output tokens / task | Cache hit | Session cache | Cost / task | Time / task (s) | Requests / task |",
        "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |",
    ]
    keep = SCOPES[scope]
    for harness in order(rows):
        mine = [row for row in rows if row["harness"] == harness and keep(row)]
        if mine:
            lines.append(summary_line(harness, mine))
    return "\n".join(lines)


def memory(rows):
    lines = [
        "| Harness | Peak agent RSS (MB) | Max agent RSS (MB) | Peak tree RSS (MB) | Max tree RSS (MB) | Trials sampled |",
        "| --- | --- | --- | --- | --- | --- |",
    ]
    for harness in order(rows):
        agent = [row["agent_mb"] for row in rows if row["harness"] == harness and row["agent_mb"]]
        tree = [row["tree_mb"] for row in rows if row["harness"] == harness and row["tree_mb"]]
        lines.append(table_row([harness, cell(agent), fmt(max(agent, default=None)), cell(tree), fmt(max(tree, default=None)), len(agent)]))
    return "\n".join(lines)


def first_ok(group):
    return ok(min(group, key=lambda row: row["trial"]))


def cost_per_solved(runs):
    wins = sum(map(ok, runs))
    if not wins or not any(row["cost"] for row in runs):
        return "–"
    return f"{sum(row['cost'] or 0 for row in runs) / wins:.4f}"


def reliability_line(harness, groups):
    runs = [row for group in groups for row in group]
    n = len(groups)
    return table_row([
        harness,
        n,
        rate(sum(map(first_ok, groups)), n),
        rate(sum(any(map(ok, group)) for group in groups), n),
        rate(sum(all(map(ok, group)) for group in groups), n),
        cost_per_solved(runs),
        cell([row["first_prompt"] for row in runs if row.get("first_prompt")]),
    ])


def reliability(rows):
    lines = [
        "| Harness | Tasks | Pass@1 | Pass@k | Pass^k | Cost / solved | First-request tokens |",
        "| --- | --- | --- | --- | --- | --- | --- |",
    ]
    for harness in order(rows):
        groups = list(per_task(rows, harness).values())
        if groups:
            lines.append(reliability_line(harness, groups))
    return "\n".join(lines)


def failure(row):
    if row["exception"]:
        return row["exception"]
    if row["requests"] == 0:
        return "no requests"
    if row["exit"] not in (None, "0"):
        return f"exit {row['exit']}"
    return "wrong answer"


def honored(mine, want):
    if want in UNSET:
        return "n/a"
    hits = [want in {effort.split("/", 1)[0] for effort in row["effort"]} for row in mine]
    if all(hits):
        return "yes"
    return "mixed" if any(hits) else "no"


def cold_hit(mine):
    firsts = [row for row in mine if row["first_prompt"]]
    return sum(row["first_hit"] or 0 for row in firsts) / max(1, sum(row["first_prompt"] for row in firsts))


def failures_text(mine):
    failures = Counter(failure(row) for row in mine if row["reward"] != 1.0)
    return "; ".join(f"{reason} {count}" for reason, count in sorted(failures.items())) or "none"


def breakdown_line(harness, mine, want):
    agent = sum(row["agent_seconds"] or 0 for row in mine)
    model = total(mine, "model_seconds")
    return table_row([
        harness,
        share(model, agent, ".0%"),
        share(agent - model, agent, ".0%"),
        share(total(mine, "reasoning"), total(mine, "output"), ".0%"),
        f"{cold_hit(mine):.1%}",
        fmt(max(row["max_prompt"] for row in mine)),
        total(mine, "errors"),
        total(mine, "rate_limited"),
        ", ".join(sorted({effort for row in mine for effort in row["effort"]})),
        honored(mine, want),
        "; ".join(sorted({control for row in mine for control in row["controls"]})),
        failures_text(mine),
        sum(1 for row in mine if row["void"]),
        sum(1 for row in mine if row["warn"]),
    ])


def breakdown(rows, want_effort=None):
    lines = [
        "| Harness | Model time share | Tool and harness time share | Reasoning share of output | Cold first-request cache hit | Max prompt tokens | Errors | 429s | Effort sent | Effort honored | Sampling controls seen | Failures | Voided | Warn |",
        "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |",
    ]
    for harness in order(rows):
        mine = [row for row in rows if row["harness"] == harness]
        if mine:
            lines.append(breakdown_line(harness, mine, want_effort))
    return "\n".join(lines)


def solved_rate(group):
    return statistics.mean(map(ok, group))


def log_ratios(mine, theirs, tasks, key):
    ratios = {}
    for task in tasks:
        a = [row[key] for row in mine[task] if row[key]]
        b = [row[key] for row in theirs[task] if row[key]]
        if a and b:
            ratios[task] = math.log(statistics.mean(a) / statistics.mean(b))
    return ratios


def ci(values, transform, draws):
    if not values:
        return "–"
    point = transform(statistics.mean(values.values()))
    means = sorted(
        transform(statistics.mean(values[t] for t in draw if t in values))
        for draw in draws
        if any(t in values for t in draw)
    )
    return f"{point:.2f} ({means[int(0.025 * len(means))]:.2f}–{means[int(0.975 * len(means)) - 1]:.2f})"


def paired_line(other, mine, theirs, rng, samples):
    tasks = sorted(set(mine) & set(theirs))
    if not tasks:
        return None
    draws = [[rng.choice(tasks) for _ in tasks] for _ in range(samples)]
    solved = {task: solved_rate(mine[task]) - solved_rate(theirs[task]) for task in tasks}
    cells = [ci(log_ratios(mine, theirs, tasks, key), math.exp, draws) for _, key in PAIRED]
    return f"| {other} | {len(tasks)} | {ci(solved, lambda v: v, draws)} | " + " | ".join(cells) + " |"


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
        line = None if other == subject else paired_line(other, mine, per_task(rows, other), rng, samples)
        if line:
            lines.append(line)
    return "\n".join(lines)


def arguments():
    parser = argparse.ArgumentParser()
    parser.add_argument("run")
    parser.add_argument("--jobs", default=str(Path(__file__).parent / "jobs"))
    parser.add_argument("--proxy", nargs="+", default=[str(Path(__file__).parent / "logs" / "deepseek.jsonl")])
    parser.add_argument("--csv")
    parser.add_argument("--subject", required=True)
    parser.add_argument("--tasks")
    return parser.parse_args()


def load(args, rates):
    rows = trials(args.jobs, args.run, requests(args.proxy), rates)
    if not args.tasks:
        return rows
    keep = {line.strip() for line in open(args.tasks) if line.strip()}
    return [row for row in rows if row["task"] in keep]


def refuse(rows, pin):
    checks = [
        (f"response model is not the pinned {pin}", {f'{row["harness"]}: {model}' for row in rows for model in row["models"] if model != pin}),
        ("solved with no request in the proxy log (proxy bypassed or wrong --proxy file)", {row["harness"] for row in rows if row["requests"] == 0 and row["reward"] == 1.0}),
        ("requests succeeded but the proxy log holds no token usage for", {row["harness"] for row in rows if row["requests"] > row["errors"] and row["prompt"] == 0}),
    ]
    for message, found in checks:
        if found:
            raise SystemExit(f"{message}: {', '.join(sorted(found))}")


def cost_note(vendor, rates):
    if not rates:
        return "Cost: suppressed for this run; tokens only."
    return RATE_CARD.get(vendor, f"Cost: no rate card for vendor {vendor}; tokens only.")


def flagged(rows):
    marked = [row for row in rows if row["void"] or row["warn"]]
    if not marked:
        return
    print("\nFlagged trials (void = solve removed; warn = path mention only)\n")
    for row in marked:
        kind = "void" if row["void"] else "warn"
        print(f"- {kind} {row['harness']} {row['task']} reward={row['reward']}: {', '.join(row['void'] + row['warn'])}")


def write_csv(path, rows):
    keys = [key for key in rows[0] if key not in ("effort", "models", "void", "warn", "controls")] if rows else []
    with open(path, "w") as out:
        out.write(",".join(keys) + "\n")
        for row in rows:
            out.write(",".join(str(row[key]) for key in keys) + "\n")


def report(args, info, rows, vendor, rates):
    models = sorted({model for row in rows for model in row["models"]})
    checksums = len({row["checksum"] for row in rows})
    print(f"Run `{args.run}`: vendor {vendor}, effort {info.get('effort', 'default')}, {len(rows)} trials, response models {models}, {checksums} task checksums.")
    print(cost_note(vendor, rates) + " Intervals: 95% Wilson for solved, bootstrap median (10,000 resamples, seed 20260916) elsewhere. A dataset-identity leak hit voids the solve; a verifier-path hit is a warn.")
    sections = [
        ("Reliability (task-level; pass@k = any of k, pass^k = all k)\n", lambda: reliability(rows)),
        ("\nAll trials\n", lambda: summary(rows)),
        ("\nSolved trials only\n", lambda: summary(rows, scope="solved")),
        ("\nFailed trials only\n", lambda: summary(rows, scope="failed")),
        ("\nMemory (peak RSS per trial, sampled every 0.5 s; agent = the harness process, tree = it and every tool it ran)\n", lambda: memory(rows)),
        ("\nWhere the time and tokens go\n", lambda: breakdown(rows, info.get("effort"))),
        (f"\nPaired against {args.subject}\n", lambda: paired(rows, args.subject)),
    ]
    for title, body in sections:
        print(title)
        print(body())
    flagged(rows)


def main():
    args = arguments()
    info = meta(args.jobs, args.run)
    vendor = info.get("vendor", "deepseek")
    rates = RATES.get(vendor)
    pin = info.get("model")
    if not pin:
        raise SystemExit("run.txt records no pinned model")
    rows = load(args, rates)
    refuse(rows, pin)
    report(args, info, rows, vendor, rates)
    if args.csv:
        write_csv(args.csv, rows)


if __name__ == "__main__":
    main()
