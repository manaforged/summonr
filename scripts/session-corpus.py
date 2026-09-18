"""Build a labelled session corpus from this machine's own sessions.

Transcripts hold whatever the user and the agent said, so the corpus is
written outside the tree and never committed.

    python3 scripts/session-corpus.py /tmp/sessions.jsonl
    summonr decide --set review --eval /tmp/sessions.jsonl

Two labels come from the recorded tool calls rather than from judgement:
`proved` is whether the session ran a command that checks its own work,
and `looped` is whether one command ran three or more times.
"""

import json
import random
import re
import subprocess
import sys

VERIFY = re.compile(
    r"cargo\s+(test|check|clippy|nextest|build)|nextest\s+run|npm\s+(test|run\s+build)"
    r"|pytest|go\s+test|make\s+(test|check)"
)
CALL = re.compile(r'`run_command`\s*\{"command":"((?:[^"\\]|\\.)*)"')
PER_BUCKET = 10
SUMMONR = "./target/debug/summonr"


def sh(*args):
    done = subprocess.run(args, capture_output=True, text=True, check=False)
    return done.stdout


def label(text):
    ran = [match.group(1) for match in CALL.finditer(text)]
    proved = any(VERIFY.search(command) for command in ran)
    repeats = {}
    for command in ran:
        repeats[command] = repeats.get(command, 0) + 1
    looped = any(count >= 3 for count in repeats.values())
    return proved, looped, len(ran)


def main():
    if len(sys.argv) < 2:
        print(__doc__)
        return 2
    out = sys.argv[1]
    listing = sh(SUMMONR, "sessions")
    ids = [line.split("\t")[0] for line in listing.splitlines() if line.strip()]
    buckets = {
        (True, True): [],
        (True, False): [],
        (False, True): [],
        (False, False): [],
    }
    for session in ids:
        text = sh(SUMMONR, "export", session)
        if not 2000 < len(text) < 400_000:
            continue
        proved, looped, ran = label(text)
        if ran == 0:
            continue
        bucket = buckets[(proved, looped)]
        if len(bucket) < PER_BUCKET:
            bucket.append(
                {
                    "name": session[:10],
                    "state": text,
                    "expect": {"proved": proved, "looped": looped},
                }
            )
        if all(len(rows) >= PER_BUCKET for rows in buckets.values()):
            break
    rows = [row for bucket in buckets.values() for row in bucket]
    random.seed(7)
    random.shuffle(rows)
    with open(out, "w", encoding="utf-8") as handle:
        for row in rows:
            handle.write(json.dumps(row) + "\n")
    print(f"{len(rows)} sessions -> {out}")
    for key in ("proved", "looped"):
        true = sum(1 for row in rows if row["expect"][key])
        print(f"  {key}: {true} true, {len(rows) - true} false")
    return 0


if __name__ == "__main__":
    sys.exit(main())
