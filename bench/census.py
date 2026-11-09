import json
import sys
from collections import defaultdict


def sessions(path):
    found = defaultdict(list)
    with open(path) as ledger:
        for line in ledger:
            line = line.strip()
            if line:
                entry = json.loads(line)
                found[entry.get("session") or "-"].append(entry)
    return found


def census(entries):
    best = {}
    raised = compared = failed = 0
    for entry in sorted(entries, key=lambda entry: entry.get("time", "")):
        scores = entry.get("scores") or {}
        if entry.get("error") or not scores:
            failed += 1
            continue
        if best:
            compared += 1
            raised += any(value > best.get(name, float("-inf")) for name, value in scores.items())
        for name, value in scores.items():
            best[name] = max(value, best.get(name, float("-inf")))
    return len(entries), failed, raised, compared


def main():
    print("session\treadings\tfailed\traised_best\tcompared\tshare")
    for session, entries in sorted(sessions(sys.argv[1]).items()):
        readings, failed, raised, compared = census(entries)
        share = f"{raised / compared:.2f}" if compared else "-"
        print(f"{session}\t{readings}\t{failed}\t{raised}\t{compared}\t{share}")


if __name__ == "__main__":
    main()
