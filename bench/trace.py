#!/usr/bin/env python3
import argparse
import hashlib
import json
from pathlib import Path


def label(harness, run, trial):
    return hashlib.sha256(f"{harness}.{run}.{trial}".encode()).hexdigest()[:16]


def trials(jobs, run, task):
    for result in sorted(Path(jobs, run).glob(f"*.{task}.r*/*/result.json")):
        data = json.loads(result.read_text())
        harness = result.parent.parent.name.split(".", 1)[0]
        reward = ((data.get("verifier_result") or {}).get("rewards") or {}).get("reward")
        yield harness, result.parent.parent.name, data["trial_name"], reward


def rounds(rows):
    for index, row in enumerate(sorted(rows, key=lambda row: row["started"]), 1):
        usage = row.get("usage") or {}
        request = row["request"]
        yield index, row, usage, request


def show(rows, name, reward):
    prompt = sum((row.get("usage") or {}).get("prompt_tokens", 0) for row in rows)
    output = sum((row.get("usage") or {}).get("completion_tokens", 0) for row in rows)
    seconds = sum(row["latency"] for row in rows)
    print(f"\n== {name} reward={reward} requests={len(rows)} input={prompt:,} output={output:,} model_s={seconds:.0f}")
    for index, row, usage, request in rounds(rows):
        results = sum(item["bytes"] for item in request.get("round_results") or [])
        calls = "; ".join(f"{c['name']} {c['args'][:90]}".replace("\n", " ") for c in request.get("round_calls") or [])
        print(
            f"{index:>3} in {usage.get('prompt_tokens', 0):>7,} out {usage.get('completion_tokens', 0):>6,}"
            f" {row['latency']:>5.0f}s | prev results {results:>6,}b | prev calls: {calls[:160]}"
        )


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("run")
    parser.add_argument("task")
    parser.add_argument("--jobs", default=str(Path(__file__).parent / "jobs"))
    parser.add_argument("--proxy", default=str(Path(__file__).parent / "logs" / "glm.jsonl"))
    parser.add_argument("--harness", nargs="*")
    args = parser.parse_args()
    by_label = {}
    for line in open(args.proxy):
        row = json.loads(line)
        by_label.setdefault(row["label"], []).append(row)
    for harness, name, trial, reward in trials(args.jobs, args.run, args.task):
        if args.harness and harness not in args.harness:
            continue
        show(by_label.get(label(harness, args.run, trial), []), name, reward)


if __name__ == "__main__":
    main()
