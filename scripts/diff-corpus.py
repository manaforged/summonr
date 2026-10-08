"""Build a labelled diff corpus from a git repository's own history.

The corpus contains verbatim source from that repository, so it is written
outside the tree and never committed. Point it at a repo, then score a
question set against the result:

    python3 scripts/diff-corpus.py ~/src/myrepo /tmp/diff.jsonl
    summonr infer --set diff --eval /tmp/diff.jsonl

Labels come from the patch text, not from judgement, so a question that
disagrees with a label is a real disagreement worth reading.
"""

import json
import random
import re
import subprocess
import sys

PANIC = re.compile(r"\.unwrap\(\)|\.expect\(|panic!|unreachable!")
PUB = re.compile(
    r"^\s*pub(\(crate\)|\(super\))?\s+(async\s+|unsafe\s+|const\s+)*"
    r"(fn|struct|enum|trait|type|mod|static)\b"
    r"|^\s*pub(\(crate\)|\(super\))?\s+\w+\s*:"
)
TEST = re.compile(r"_tests\.rs$|/tests/|test_support\.rs$|fixtures?\.rs$|/testing/")
GUARD = re.compile(
    r"^\s*(if|\}\s*else\s+if|match|while)\b"
    r"|^\s*(assert|debug_assert|ensure)!"
    r"|return\s+Err\(|\.ok_or|\.ok_or_else|\.checked_|\bis_empty\(\)|\bcontains\("
)
PER_BUCKET = 5


def git(root, *args):
    done = subprocess.run(
        ["git", *args], capture_output=True, text=True, cwd=root, check=False
    )
    return done.stdout


def changed(text, sign, header):
    return [
        line[1:]
        for line in text.splitlines()
        if line.startswith(sign) and not line.startswith(header)
    ]


def label(path, text):
    added = changed(text, "+", "+++")
    if any("#[cfg(test)]" in line for line in added):
        return None
    removed = changed(text, "-", "---")
    is_test = bool(TEST.search(path))
    panics = (not is_test) and any(PANIC.search(line) for line in added)
    widens = any(PUB.search(line) for line in added)
    gone = sum(1 for line in removed if GUARD.search(line))
    kept = sum(1 for line in added if GUARD.search(line))
    guards = gone > kept
    return panics, widens, guards


def rust_diffs(patch):
    for body in re.split(r"(?m)^diff --git ", patch)[1:]:
        path = body.splitlines()[0].split()[-1]
        path = path[2:] if path.startswith("b/") else path
        if not path.endswith(".rs"):
            continue
        text = "diff --git " + body
        if not 300 < len(text) < 50_000:
            continue
        yield path, text


def example(sha, path, text, marked):
    return {
        "name": f"{sha[:7]}:{path.split('/')[-1]}"[:26],
        "state": text,
        "expect": {
            "adds_panic": marked[0],
            "widens_surface": marked[1],
            "removes_guard": marked[2],
        },
    }


def build(root, commits):
    keys = [(a, b, c) for a in (True, False) for b in (True, False) for c in (True, False)]
    buckets = {key: [] for key in keys}
    for sha in git(root, "log", "--format=%H", f"-{commits}").split():
        patch = git(root, "show", "--no-color", "--no-renames", sha)
        for path, text in rust_diffs(patch):
            marked = label(path, text)
            if marked is None:
                continue
            bucket = buckets[marked]
            if len(bucket) < PER_BUCKET:
                bucket.append(example(sha, path, text, marked))
        if all(len(rows) >= PER_BUCKET for rows in buckets.values()):
            break
    rows = [row for bucket in buckets.values() for row in bucket]
    random.seed(7)
    random.shuffle(rows)
    return rows


def whole(root, commits):
    rows = []
    for sha in git(root, "log", "--format=%H", f"-{commits}").split():
        patch = git(root, "show", "--no-color", "--no-renames", sha)
        paths = [
            line.split()[-1][2:]
            for line in patch.splitlines()
            if line.startswith("diff --git ")
        ]
        if not paths or not 300 < len(patch) < 200_000:
            continue
        rows.append(
            {
                "name": sha[:7],
                "state": patch,
                "expect": {
                    "untested": not any(TEST.search(path) for path in paths),
                    "spans_crates": len({path.split("/")[0:2][-1] for path in paths}) > 1,
                },
            }
        )
    tested = [row for row in rows if not row["expect"]["untested"]][:20]
    untested = [row for row in rows if row["expect"]["untested"]][:20]
    both = tested + untested
    random.seed(7)
    random.shuffle(both)
    return both


def main():
    if len(sys.argv) < 3:
        print(__doc__)
        return 2
    root, out = sys.argv[1], sys.argv[2]
    commits = int(sys.argv[3]) if len(sys.argv) > 3 else 400
    if out.endswith("commits.jsonl"):
        rows = whole(root, commits)
        with open(out, "w", encoding="utf-8") as handle:
            for row in rows:
                handle.write(json.dumps(row) + "\n")
        true = sum(1 for row in rows if row["expect"]["untested"])
        print(f"{len(rows)} commits -> {out}")
        print(f"  untested: {true} true, {len(rows) - true} false")
        return 0
    rows = build(root, commits)
    with open(out, "w", encoding="utf-8") as handle:
        for row in rows:
            handle.write(json.dumps(row) + "\n")
    counts = {
        key: sum(1 for row in rows if row["expect"][key])
        for key in ("adds_panic", "widens_surface", "removes_guard")
    }
    print(f"{len(rows)} examples -> {out}")
    for key, true in counts.items():
        print(f"  {key}: {true} true, {len(rows) - true} false")
    if len(rows) < 8 * PER_BUCKET:
        print("warning: fewer examples than asked for; widen the commit range")
    return 0


if __name__ == "__main__":
    sys.exit(main())
