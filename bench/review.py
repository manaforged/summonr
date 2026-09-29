#!/usr/bin/env python3
import argparse
import json
from collections import defaultdict
from pathlib import Path

import deep
from stats import requests
from trial import trials

SUBJECT = "summonr"
IDLE_SECONDS = 30
SLOWER = 2.0
HEAVIER = 2.0
MEMORY_MB = 100


def paired(rows):
    tasks = defaultdict(dict)
    for row in rows:
        tasks[row["task"]][row["harness"]] = row
    return {task: arms for task, arms in tasks.items() if len(arms) > 1}


def solved(row):
    return row["reward"] == 1.0 and not row["void"]


def session_notes(jobs, run, row):
    notes = []
    for path in Path(jobs, run).glob(f"{SUBJECT}.{row['task']}.r*/*/agent/summonr/sessions/*/session.jsonl"):
        for line in path.read_text().splitlines():
            record = json.loads(line)
            millis = (record.get("usage") or {}).get("millis", 0)
            note = record.get("note") or ""
            if note == "unaccounted turn time" and millis > IDLE_SECONDS * 1000:
                notes.append(f"idle {millis // 1000} s inside summonr")
            elif record.get("kind") == "charge" and note and note != "unaccounted turn time":
                notes.append(f"stop: {note[:120]}")
    return notes


def ratio(mine, other, key):
    if not mine.get(key) or not other.get(key):
        return None
    return mine[key] / other[key]


def anomalies(jobs, run, mine, other):
    found = session_notes(jobs, run, mine)
    if mine["exit"] not in (None, "0"):
        found.append(f"exit {mine['exit']}")
    if mine["exception"]:
        found.append(f"exception {mine['exception']}")
    if mine["errors"]:
        found.append(f"{mine['errors']} provider errors ({mine['rate_limited']} rate limited)")
    slow = ratio(mine, other, "agent_seconds")
    if slow and slow > SLOWER:
        found.append(f"{slow:.1f}x pi's time ({mine['agent_seconds']:.0f} s vs {other['agent_seconds']:.0f} s)")
    heavy = ratio(mine, other, "prompt")
    if heavy and heavy > HEAVIER:
        found.append(f"{heavy:.1f}x pi's input tokens")
    if (mine.get("agent_mb") or 0) > MEMORY_MB:
        found.append(f"agent peak {mine['agent_mb']:.0f} MB")
    return found


def review(jobs, run, rows):
    tasks = paired(rows)
    ours = sum(solved(arms[SUBJECT]) for arms in tasks.values())
    theirs = sum(solved(arms["pi"]) for arms in tasks.values())
    print(f"{run}: {len(tasks)} paired, {SUBJECT} {ours}, pi {theirs}")
    for task in sorted(tasks):
        mine, other = tasks[task][SUBJECT], tasks[task]["pi"]
        verdict = {(True, False): "summonr only", (False, True): "PI ONLY", (False, False): "both failed"}.get(
            (solved(mine), solved(other)), ""
        )
        found = anomalies(jobs, run, mine, other)
        if verdict or found:
            print(f"- {task}: {verdict}{'; ' if verdict and found else ''}{'; '.join(found)}")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("run")
    parser.add_argument("--jobs", default=str(Path(__file__).parent / "jobs"))
    parser.add_argument("--proxy", nargs="+", required=True)
    parser.add_argument("--task", help="print both harnesses' round timelines and the summonr profile for one task")
    parser.add_argument("--tools", action="store_true", help="print the run's tool-failure census")
    args = parser.parse_args()
    by_label = requests(args.proxy)
    if args.task:
        deep.timeline(args.jobs, args.run, args.task, by_label)
    elif args.tools:
        deep.census(args.jobs, args.run)
    else:
        review(args.jobs, args.run, trials(args.jobs, args.run, by_label, None))


if __name__ == "__main__":
    main()
