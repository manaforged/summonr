#!/usr/bin/env python3
import argparse
from collections import Counter
from pathlib import Path

from audit import broken_note, clean, flagged, lookups, proxy_check, refusals, superseded, unjudged
from paired import control, failed, ok, order, paired, per_task, task_rate
from stats import cell, fmt, requests
from trial import standard, trials

RATES = {
    "deepseek": {"hit": 0.006, "miss": 0.30, "output": 1.20},
    "chatgpt": {"hit": 0.10, "miss": 2.00, "output": 10.00},
    "supergrok": {"hit": 0.50, "miss": 2.00, "output": 6.00, "long": {"at": 200_000, "hit": 1.00, "miss": 4.00, "output": 12.00}},
}
RATE_CARD = {
    "deepseek": "DeepSeek Flash peak list price (cache hit $0.006, miss $0.30, output $1.20 per 1M tokens), api-docs.deepseek.com/quick_start/pricing, 2026-09-19; off-peak is half; list price, not plan spend",
    "chatgpt": "gpt-6.1-sol API list price (cached input $0.10, input $2.00, output $10.00 per 1M tokens), openai.com/index/introducing-gpt-6-1-sol, 2026-10-12; cache writes priced as input; list price, not plan spend",
    "supergrok": "grok-4.7 API list price (cached input $0.50, input $2.00, output $6.00 per 1M tokens; a request of 200K or more prompt tokens bills $1.00, $4.00 and $12.00 for all its tokens), docs.x.ai/docs/models, 2026-10-16; requests whose prompt sizes are not recorded are not priced; list price, not plan spend",
}
UNSET = ("", "default", None)


def meta(jobs, run):
    kv = {}
    path = Path(jobs, run, "run.txt")
    if not path.is_file() or not path.read_text().strip():
        raise SystemExit(f"{path} missing or empty: run it with run.sh before tabulating")
    for line in path.read_text().splitlines():
        key, _, value = line.partition(" ")
        kv[key.strip()] = value.strip()
    return kv


def rate(rows, solved=lambda tries: sum(tries) / len(tries)):
    groups = {}
    for row in rows:
        groups.setdefault(row["task"], []).append(row)
    return f"{sum(map(ok, rows))}/{len(rows)}, {task_rate(groups, solved)}"


def share(part, whole, spec):
    return format(part / whole, spec) if whole and part is not None else "–"


def column(rows, key, digits=0):
    return cell([row[key] for row in rows if row[key] is not None], digits)


def total(rows, key):
    values = [row[key] for row in rows if row[key] is not None]
    return sum(values) if values else None


def table_row(values):
    return "| " + " | ".join(str(value) for value in values) + " |"


SCOPES = {"all": lambda row: True, "solved": ok, "failed": failed}


def summary_line(harness, mine):
    return table_row([
        harness,
        rate(mine),
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
        "| Harness | Mean agent RSS (MB) | Peak agent RSS (MB) | Max agent RSS (MB) | Mean harness RSS (MB) | Peak harness RSS (MB) | Max harness RSS (MB) | Mean tree RSS (MB) | Peak tree RSS (MB) | Max tree RSS (MB) | Trials sampled |",
        "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |",
    ]
    for harness in order(rows):
        mine = [row for row in rows if row["harness"] == harness]
        cols = []
        for key in ("agent", "harness", "tree"):
            peaks = [row[f"{key}_mb"] for row in mine if row.get(f"{key}_mb")]
            means = [row[f"{key}_avg_mb"] for row in mine if row.get(f"{key}_avg_mb")]
            cols += [cell(means), cell(peaks), fmt(max(peaks, default=None))]
        lines.append(table_row([harness, *cols, len([row for row in mine if row.get("agent_mb")])]))
    return "\n".join(lines)


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
        rate(runs),
        rate(runs, any),
        rate(runs, all),
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
    hits = [want in {effort.split("/", 1)[0] for effort in row["effort"]} for row in mine if row["effort"]]
    if all(hits):
        return "yes"
    return "mixed" if any(hits) else "no"


def cold_hit(mine):
    firsts = [row for row in mine if row["first_prompt"]]
    if not firsts:
        return "–"
    return f"{sum(row['first_hit'] or 0 for row in firsts) / sum(row['first_prompt'] for row in firsts):.1%}"


def failures_text(mine):
    failures = Counter(failure(row) for row in mine if row["reward"] != 1.0)
    return "; ".join(f"{reason} {count}" for reason, count in sorted(failures.items())) or "none"


def breakdown_line(harness, mine, want):
    agent = sum(row["agent_seconds"] or 0 for row in mine)
    model = total(mine, "model_seconds")
    return table_row([
        harness,
        share(model, agent, ".0%"),
        share(None if model is None else agent - model, agent, ".0%"),
        share(total(mine, "reasoning"), total(mine, "output"), ".0%"),
        cold_hit(mine),
        fmt(max((row["max_prompt"] for row in mine if row["max_prompt"] is not None), default=None)),
        fmt(total(mine, "errors")),
        fmt(total(mine, "cut")),
        fmt(total(mine, "rate_limited")),
        ", ".join(sorted({effort for row in mine for effort in row["effort"]})),
        honored(mine, want),
        "; ".join(sorted({control for row in mine for control in row["controls"]})),
        failures_text(mine),
        sum(1 for row in mine if row["void"]),
        sum(1 for row in mine if row["warn"]),
    ])


def breakdown(rows, want_effort=None):
    lines = [
        "| Harness | Model time share | Tool and harness time share | Reasoning share of output | Cold first-request cache hit | Max prompt tokens | Errors | Cut streams | 429s | Effort sent | Effort honored | Sampling controls seen | Failures | Voided | Warn |",
        "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |",
    ]
    for harness in order(rows):
        mine = [row for row in rows if row["harness"] == harness]
        if mine:
            lines.append(breakdown_line(harness, mine, want_effort))
    return "\n".join(lines)


def arguments():
    parser = argparse.ArgumentParser()
    parser.add_argument("runs", nargs="+")
    parser.add_argument("--jobs", default=str(Path(__file__).parent / "jobs"))
    parser.add_argument("--proxy", nargs="+")
    parser.add_argument("--csv")
    parser.add_argument("--subject", required=True)
    parser.add_argument("--tasks")
    parser.add_argument("--control")
    return parser.parse_args()


def gather(args, rates):
    rows, shared = [], {}
    for run in args.runs:
        logs = [Path(log) for log in args.proxy or [Path(args.jobs).parent / "logs" / f"{run}.jsonl"] if not standard(args.jobs, run) or Path(log).is_file()]
        found = trials(args.jobs, run, requests(logs) if logs else None, rates)
        rows += [{**row, "run": run} for row in found]
    for row in rows:
        if shared.setdefault(row["harness"], row["run"]) != row["run"]:
            raise SystemExit(f"harness {row['harness']} appears in {shared[row['harness']]} and {row['run']}")
    return rows


def load(args, rates):
    if args.proxy and len(args.runs) > 1:
        raise SystemExit("--proxy names one log; give one run with it")
    rows = gather(args, rates)
    if args.tasks:
        keep = {line.strip() for line in open(args.tasks) if line.strip()}
        rows = [row for row in rows if row["task"] in keep]
    print(broken_note(rows), end="")
    return rows


def refuse(rows, pin, side=()):
    checks = [
        (f"response model is not the pinned {pin}", {f'{row["harness"]}: {model}' for row in rows for model in row["models"] if model != pin and not model.startswith(f"{pin}-") and model not in side}),
        ("solved with no request in the proxy log (proxy bypassed or wrong --proxy file)", {row["harness"] for row in rows if row["requests"] == 0 and row["reward"] == 1.0}),
        ("requests succeeded but no token usage was recorded for", {row["harness"] for row in rows if row["requests"] > (row["errors"] or 0) + (row["cut"] or 0) and row["prompt"] == 0}),
    ]
    for message, found in checks:
        if found:
            raise SystemExit(f"{message}: {', '.join(sorted(found))}")


def windows(rows):
    spans = {}
    for row in rows:
        start, end = row.get("window") or (None, None)
        if start and end:
            low, high = spans.get(row["harness"], (start, end))
            spans[row["harness"]] = (min(low, start), max(high, end))
    return "Active window per arm (UTC): " + "; ".join(f"{arm} {start[:16]} to {end[:16]}" for arm, (start, end) in sorted(spans.items())) + "."


def cost_note(vendor, rates):
    if not rates:
        return "Cost: suppressed for this run; tokens only."
    return RATE_CARD.get(vendor, f"Cost: no rate card for vendor {vendor}; tokens only.")


VERSION_KEYS = {"summonr": ("summonr_version", "summonr_commit"), "pi": ("pi",), "opencode2": ("opencode2",), "hermes": ("hermes",), "omp": ("omp",)}


def version_of(info, harness):
    keys = VERSION_KEYS.get(harness, ("summonr_commit",) if harness.startswith("summonr-") else ())
    found = [info[key][:12] if key.endswith("commit") else info[key] for key in keys if info.get(key)]
    return " ".join(found[:1] + [f"({value})" for value in found[1:]]) or "unrecorded"


def write_csv(path, rows):
    keys = [key for key in rows[0] if key not in ("effort", "models", "void", "warn", "controls", "window")] if rows else []
    with open(path, "w") as out:
        out.write(",".join(keys) + "\n")
        for row in rows:
            out.write(",".join(str(row[key]) for key in keys) + "\n")


def proxy_section(args, rows):
    logs = [log for run in args.runs if standard(args.jobs, run) for log in [Path(args.jobs).parent / "logs" / f"{run}.jsonl"] if log.is_file()]
    if not logs:
        return []
    return [("\nProxy cross-check (tokens the provider counted against tokens each harness reported; settings each harness sent)\n", lambda: proxy_check(rows, requests(logs)))]


def report(args, info, infos, rows, vendor, rates):
    models = sorted({model for row in rows for model in row["models"]})
    checksums = len({row["checksum"] for row in rows})
    runs = ", ".join(f"`{run}`" for run in args.runs)
    print(f"Run {runs}: vendor {vendor}, effort {info.get('effort', 'default')}, {len(rows)} trials, response models {models}, {checksums} task checksums.")
    if len(args.runs) > 1:
        print("Combined runs: arms from different runs did not run side by side; tasks, model, effort, and dataset match.")
    run_of = {row["harness"]: row["run"] for row in rows}
    seen = {row["harness"]: row.get("version") for row in rows if row.get("version")}
    print("Versions: " + ", ".join(f"{harness} {seen.get(harness) or version_of(infos[run_of[harness]], harness)}" for harness in order(rows)) + ".")
    print(cost_note(vendor, rates) + " Intervals: solve rates are a 95% bootstrap over tasks; token, time, and memory medians are a bootstrap over tries (10,000 resamples, seed 20260916). A URL to a dataset source or a judge FAIL voids the solve; a source name or verifier path is a warn.")
    for run in args.runs:
        gone = superseded(Path(args.jobs) / run)
        print(f"Reruns in {run}: " + (", ".join(" ".join(fields[1:]) for fields in gone if fields[0] == "rerun") or "none") + f"; {sum(fields[0] == 'dropped' for fields in gone)} unfinished trials dropped on resume.")
    print(windows(rows))
    if unjudged(rows):
        print(f"Not judged: passing trials without analysis.json per harness {unjudged(rows)}; run harbor analyze -r judge.toml before publishing.")
    sections = [
        ("Reliability (pass@1 = mean over every try; pass@k = task solved in any of k, pass^k = in all k)\n", lambda: reliability(rows)),
        ("\nAll trials\n", lambda: summary(rows)),
        ("\nSolved trials only\n", lambda: summary(rows, scope="solved")),
        ("\nFailed trials only\n", lambda: summary(rows, scope="failed")),
        ("\nMemory (mean and peak RSS per trial, sampled every 0.5 s; agent = the harness process, harness = it and every helper it started without a shell (language servers, MCP servers, its own server), tree = it and every process it ran)\n", lambda: memory(rows)),
        ("\nWhere the time and tokens go\n", lambda: breakdown(rows, info.get("effort"))),
        (f"\nPaired against {args.subject}\n", lambda: paired(rows, args.subject)),
        ("\nLookups (solved trials, with voided solves scored 0 and counted)\n", lambda: lookups(rows, order(rows, args.subject))),
        ("\nClean tasks (no voided trial in any arm)\n", lambda: reliability(clean(rows))),
    ]
    sections += proxy_section(args, rows)
    if args.control:
        sections.append((f"\nA/A control ({args.control})\n", lambda: control(rows, args.subject, args.control)))
    for title, body in sections:
        print(title)
        print(body())
    flagged(rows)


def main():
    args = arguments()
    infos = {run: meta(args.jobs, run) for run in args.runs}
    for key in ("vendor", "provider", "model", "effort", "dataset", "tasks", "timeout_mult"):
        values = {run: found.get(key) for run, found in infos.items()}
        if len(set(values.values())) > 1:
            raise SystemExit(f"runs differ on {key}: {values}")
    info = infos[args.runs[0]]
    vendor = info.get("vendor", "deepseek")
    rates = RATES.get(vendor)
    pin = info.get("model")
    if not pin:
        raise SystemExit("run.txt records no pinned model")
    rows = load(args, rates)
    problems = refusals(args.jobs, [run for run in args.runs if standard(args.jobs, run)], rows, vendor, any(found.get("protocol") for found in infos.values()) and (Path(__file__).parent / "pairs" / f"{vendor}.proxy").is_file())
    if problems:
        raise SystemExit("the run does not meet its protocol:\n" + "\n".join(problems))
    refuse(rows, pin, [model for model in info.get("side_models", "-").split() if model != "-"])
    report(args, info, infos, rows, vendor, rates)
    if args.csv:
        write_csv(args.csv, rows)


if __name__ == "__main__":
    main()
