import json
import os
import subprocess
from collections import Counter
from pathlib import Path

from stats import hit_tokens
from trial import calls_for

TOOLS = Path(__file__).parent / "tools"
FAILURE_CAUSES = [
    ("edit", "does not appear in the file", "old text not found"),
    ("edit", "edits overlap", "edits overlap"),
    ("edit", "edit got keys", "unknown edit keys"),
    ("edit", "edit requires old+new", "unknown edit keys"),
    ("edit", "matches", "tag ambiguous"),
    ("edit", "tag", "tag stale"),
    (None, "No such file or directory", "no such path"),
    (None, "Is a directory", "path is a directory"),
    (None, "non-empty path", "empty path"),
    ("run_command", "timed out", "command timed out"),
    ("run_command", "exit ", "command failed"),
]


def trial_dir(jobs, run, harness, task):
    return next(Path(jobs, run).glob(f"{harness}.{task}.r*/*/"), None)


def rounds(jobs, run, harness, task, by_label):
    trial = trial_dir(jobs, run, harness, task)
    if trial is None:
        return []
    data = json.loads((trial / "result.json").read_text())
    return calls_for(by_label, harness, run, data)


def round_line(index, row, prompt_before):
    usage = row.get("usage") or {}
    prompt = usage.get("prompt_tokens") or 0
    request = row["request"]
    tools = ",".join(call.get("name") or "?" for call in request.get("round_calls") or []) or "-"
    result_bytes = sum(result.get("bytes", 0) for result in request.get("round_results") or [])
    hit = hit_tokens(usage) / prompt if prompt else 0
    return (
        f"{index:>4} {prompt:>8,} {prompt - prompt_before:>+8,} {hit:>5.0%} "
        f"{usage.get('completion_tokens') or 0:>7,} {row.get('reasoning_tokens') or 0:>7,} "
        f"{round(row.get('latency') or 0):>5}s {result_bytes:>8,}  {tools[:40]}"
        + (f"  !{row.get('error')[:40]}" if row.get("error") else "")
    )


def summary(harness, calls):
    usage = [row.get("usage") or {} for row in calls]
    prompts = [u.get("prompt_tokens") or 0 for u in usage]
    outputs = [u.get("completion_tokens") or 0 for u in usage]
    results = sum(
        result.get("bytes", 0) for row in calls for result in row["request"].get("round_results") or []
    )
    return (
        f"{harness}: {len(calls)} requests, input {sum(prompts):,}, "
        f"max prompt {max(prompts, default=0):,}, output {sum(outputs):,} (max round {max(outputs, default=0):,}), "
        f"tool result bytes {results:,}"
    )


def profile(jobs, run, task):
    trial = trial_dir(jobs, run, "summonr", task)
    sessions = list(trial.glob("agent/summonr/sessions/*/session.jsonl")) if trial else []
    if not sessions:
        return "no summonr session"
    home = sessions[0].parents[2]
    binary = TOOLS / "summonr" / "bin" / "summonr"
    done = subprocess.run(
        [str(binary), "profile", sessions[0].parent.name],
        capture_output=True,
        text=True,
        env={**os.environ, "SUMMONR_HOME": str(home)},
        check=False,
    )
    return (done.stdout or done.stderr).strip()


def timeline(jobs, run, task, by_label):
    header = f"{'rnd':>4} {'prompt':>8} {'grew':>8} {'cache':>5} {'out':>7} {'reason':>7} {'time':>6} {'res B':>8}  prior-round tools"
    for harness in ("summonr", "pi"):
        calls = rounds(jobs, run, harness, task, by_label)
        print(f"\n== {summary(harness, calls)}\n{header}")
        before = 0
        for index, row in enumerate(calls, 1):
            print(round_line(index, row, before))
            before = (row.get("usage") or {}).get("prompt_tokens") or before
    print(f"\n== summonr profile\n{profile(jobs, run, task)}")


def cause(tool, text):
    return next(
        (label for scope, needle, label in FAILURE_CAUSES if scope in (None, tool) and needle in text),
        "other",
    )


def failures(session):
    calls = {}
    for line in session.read_text().splitlines():
        record = json.loads(line)
        if record.get("kind") == "assistant":
            calls.update({call["id"]: call["function"]["name"] for call in record.get("tool_calls") or []})
        elif record.get("kind") == "tool":
            yield calls.get(record.get("call"), "?"), record.get("success") is not False, record.get("text") or ""


def census(jobs, run):
    total, failed, causes = Counter(), Counter(), Counter()
    for session in Path(jobs, run).glob("summonr.*/*/agent/summonr/sessions/*/session.jsonl"):
        for tool, ok, text in failures(session):
            total[tool] += 1
            if not ok:
                failed[tool] += 1
                causes[(tool, cause(tool, text))] += 1
    print(f"{run}: tool calls and failures (summonr sessions)")
    for tool in sorted(total, key=total.get, reverse=True):
        print(f"  {tool:<12} {total[tool]:>5} calls {failed[tool]:>4} failed ({failed[tool] / total[tool]:.0%})")
    print("causes")
    for (tool, label), count in causes.most_common():
        print(f"  {count:>4}  {tool:<12} {label}")
