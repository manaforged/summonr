"""Count where sessions stopped before the work was done.

Reads the session logs under $SUMMONR_HOME/sessions (default ~/.summonr) and
prints one JSON object:

    python3 scripts/session-stops.py --since 2026-10-15 --skip /cand/

nudges       user messages that only push the agent on ("go", "keep going")
ends         turns that closed normally
open_ends    of those, turns whose last reply names work still open
skill_stops  turns that ended right after a slash-command message
steers       messages delivered while a turn was running
"""

import argparse
import datetime
import json
import os
import re
import sys

NUDGE = re.compile(
    r"^\s*(go|yes|yep|yeah|ok|continue|keep (going|pushing|fixing|coding|working)"
    r"|fix (it|all|this)|land (it|all|this)|do it)\b"
    r"|stop(ping)? (every|after)|keep ?stopping|why .*stop|not (\w+ )?done|don'?t act",
    re.I,
)
OPEN = re.compile(
    r"\b(next|remaining|not (yet )?(landed|done|started|run)|still (ahead|open|blocked)"
    r"|waits? on you|your (go|call|word)|say (the word|push|go)|parked"
    r"|separate (migration|unit)|after (that|those)|queued|stays stopped|is blocked)\b",
    re.I,
)
HARNESS = ("Harness", "[summon", "Repair the failed", "The completion check failed",
           "Work toward", "<")
STEER = "the user sent this while you were working"


def records(path):
    with open(path, encoding="utf-8") as lines:
        for line in lines:
            try:
                yield json.loads(line)
            except json.JSONDecodeError:
                continue


def scan(path, totals):
    last = ""
    previous = ""
    for record in records(path):
        kind = record.get("kind")
        if kind == "assistant":
            last = record.get("text") or ""
        elif kind == "close" and record.get("code") is None:
            totals["ends"] += 1
            totals["open_ends"] += bool(OPEN.search(last[-1500:]))
            totals["skill_stops"] += previous.startswith("/")
        elif kind == "user":
            stored = record.get("text") or ""
            text = record.get("source") or stored
            totals["steers"] += STEER in stored
            if text.startswith(HARNESS):
                continue
            totals["messages"] += 1
            totals["nudges"] += bool(len(text) < 220 and NUDGE.search(text[:200]))
            previous = text


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--since", required=True, help="YYYY-MM-DD")
    parser.add_argument("--skip", action="append", default=[],
                        help="skip sessions whose workspace contains this text")
    args = parser.parse_args()
    home = os.environ.get("SUMMONR_HOME") or os.path.expanduser("~/.summonr")
    root = os.path.join(home, "sessions")
    since = datetime.datetime.fromisoformat(args.since).timestamp()
    totals = dict.fromkeys(
        ("sessions", "messages", "nudges", "ends", "open_ends", "skill_stops", "steers"), 0)
    for name in sorted(os.listdir(root)):
        path = os.path.join(root, name, "session.jsonl")
        if not os.path.exists(path) or os.path.getmtime(path) < since:
            continue
        head = next(records(path), {})
        workspace = str(head.get("workspace") or "")
        if any(skip in workspace for skip in args.skip):
            continue
        totals["sessions"] += 1
        scan(path, totals)
    json.dump(totals, sys.stdout)
    print()


if __name__ == "__main__":
    main()
