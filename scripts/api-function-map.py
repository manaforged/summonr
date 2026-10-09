#!/usr/bin/env python3
"""File every function under a job. What cannot be filed is drift.

Reads `off/functions.tsv` and `off/lines.tsv`, which `scripts/api-census.py`
writes, and assigns each function one bucket:

- a job from the menu in `docs/guide/internals/module-api.md`
- named plumbing (tui, transport, runtime, tools-core, process)
- `REVIEW` — nothing claimed it

Writes `docs/guide/internals/function-map.md`. `REVIEW` is a list, not an answer: a row in
it is either a job that needs naming or code that needs to go.

    python3 scripts/api-census.py && python3 scripts/api-function-map.py
"""

from __future__ import annotations

import fnmatch
from collections import Counter, defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TSV = ROOT / "off" / "functions.tsv"
LINES = ROOT / "off" / "lines.tsv"
OUT = ROOT / "docs" / "guide" / "internals" / "function-map.md"

RULES: list[tuple[str, list[str]]] = [
    ("read / edit / write", ["src/hashline.rs", "src/diff.rs", "src/tools/workspace.rs", "src/tools/search.rs"]),
    ("run_command", ["src/tools/command.rs", "src/shell.rs"]),
    ("lsp", ["src/tools/lsp.rs", "src/language.rs"]),
    ("web", ["src/web.rs", "src/web/*"]),
    ("skill", ["src/skills.rs"]),
    ("task", ["src/child.rs", "src/runtime/child.rs"]),
    ("verify", ["src/verify.rs"]),
    ("steps", ["src/steps.rs"]),
    ("goal", ["src/goal.rs"]),
    ("session", ["src/durable.rs", "src/durable/*", "src/session.rs", "src/session/*"]),
    ("worktree", ["src/worktree.rs"]),
    ("workflow", ["src/workflow.rs", "src/review.rs"]),
    ("policy", ["src/policy.rs"]),
    ("endpoint / config", ["src/endpoint.rs", "src/config.rs", "src/secret.rs", "src/modules.rs", "src/prefs.rs", "src/provider/auth.rs"]),
    ("room", ["src/room.rs", "src/room/*", "src/bus.rs", "src/rpc.rs", "src/runtime/bus.rs"]),
    ("plumbing: tui", ["src/tui.rs", "src/tui/*"]),
    ("plumbing: transport", ["src/provider.rs", "src/provider/*"]),
    ("plumbing: runtime", ["src/runtime.rs", "src/runtime/*"]),
    ("plumbing: tools-core", ["src/tools.rs", "src/tools/catalog.rs", "src/tools/sandbox.rs", "src/tools/sandbox/*"]),
    ("plumbing: process", [
        "src/lib.rs", "src/main.rs", "src/main/*", "src/error.rs", "src/command.rs",
        "src/jobs.rs", "src/jobs/*", "src/herdr.rs", "src/recent.rs", "src/splash.rs",
        "src/prompt.rs", "src/scout.rs",
    ]),
]

MENU_JOBS = {name for name, _ in RULES if not name.startswith("plumbing")}


def bucket_of(path: str) -> str:
    for name, patterns in RULES:
        for pattern in patterns:
            if fnmatch.fnmatch(path, pattern):
                return name
    return "REVIEW"


def read_tsv() -> list[tuple[str, int, str, int, int]]:
    rows: list[tuple[str, int, str, int, int]] = []
    for line in TSV.read_text().splitlines()[1:]:
        path, line_no, name, loc, mentions = line.split("\t")
        rows.append((path, int(line_no), name, int(loc), int(mentions)))
    return rows


def read_lines() -> list[tuple[str, str, int]]:
    rows: list[tuple[str, str, int]] = []
    for line in LINES.read_text().splitlines()[1:]:
        path, kind, count = line.split("\t")
        rows.append((path, kind, int(count)))
    return rows


def bucket_table(buckets, source, rows) -> list[str]:
    area_files: Counter[str] = Counter()
    area_lines: Counter[str] = Counter()
    for path, _, count in source:
        area_files[bucket_of(path)] += 1
        area_lines[bucket_of(path)] += count
    out = [
        "| bucket | files | lines | functions | fn lines | share |",
        "| --- | ---: | ---: | ---: | ---: | ---: |",
    ]
    for name in sorted(set(buckets) | set(area_lines), key=lambda n: -area_lines[n]):
        group = buckets.get(name, [])
        share = 100 * len(group) / len(rows)
        out.append(
            f"| {name} | {area_files[name]} | {area_lines[name]} | {len(group)} "
            f"| {sum(r[3] for r in group)} | {share:.1f}% |"
        )
    out.append("")
    return out


def bucket_sections(buckets) -> list[str]:
    out: list[str] = []
    for name in sorted(buckets):
        group = sorted(buckets[name], key=lambda r: -r[3])
        out.append(f"## {name} ({len(group)})")
        out.append("")
        out.append("| fn | lines | mentions | where |")
        out.append("| --- | ---: | ---: | --- |")
        for path, line_no, fn, loc, mentions in group:
            out.append(f"| `{fn}` | {loc} | {mentions} | `{path}:{line_no}` |")
        out.append("")
    return out


def main() -> None:
    rows = read_tsv()
    tree = read_lines()
    buckets: dict[str, list[tuple[str, int, str, int, int]]] = defaultdict(list)
    for row in rows:
        buckets[bucket_of(row[0])].append(row)

    source = [r for r in tree if r[1] == "source"]
    test = [r for r in tree if r[1] == "test"]

    jobs = sum(len(v) for k, v in buckets.items() if k in MENU_JOBS)
    plumbing = sum(len(v) for k, v in buckets.items() if k.startswith("plumbing"))
    review = len(buckets.get("REVIEW", []))
    body_lines = sum(sum(r[3] for r in v) for v in buckets.values())

    out: list[str] = []
    out.append("# Function map")
    out.append("")
    out.append("Every function in `src/`, filed by the job it serves. Generated by")
    out.append("`scripts/api-function-map.py` from `off/functions.tsv` and")
    out.append("`off/lines.tsv`.")
    out.append("")
    out.append(
        f"The tree: {len(source)} source files, {sum(r[2] for r in source)} lines."
    )
    out.append(
        f"Tests: the {len(test)} files `scripts/api-census.py` skips, "
        f"{sum(r[2] for r in test)} lines."
    )
    out.append("")
    out.append(f"{len(rows)} functions, {body_lines} lines of body, in three piles:")
    out.append("")
    out.append(f"- **{jobs}** serve a job on [`module-api.md`](module-api.md)")
    out.append(f"- **{plumbing}** are named plumbing (tui, transport, runtime, tools-core, process)")
    out.append(f"- **{review}** are `REVIEW` — nothing claimed them")
    out.append("")
    out.append("`REVIEW` is a list, not an answer. A row there is either a job nobody")
    out.append("named, or code that should go.")
    out.append("")
    out += bucket_table(buckets, source, rows)
    out += bucket_sections(buckets)

    OUT.write_text("\n".join(out) + "\n")
    print(f"wrote {OUT.relative_to(ROOT)}")
    print(f"functions {len(rows)} — jobs {jobs}, plumbing {plumbing}, REVIEW {review}")
    counts = Counter({k: len(v) for k, v in buckets.items()})
    for name, n in counts.most_common():
        print(f"  {n:5} {name}")


if __name__ == "__main__":
    main()
