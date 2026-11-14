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
    ("read / edit / write", [
        "crates/agent/src/hashline.rs", "crates/agent/src/hunks.rs", "crates/agent/src/diff.rs", "crates/agent/src/diff/*", "crates/agent/src/tools/workspace.rs",
        "crates/agent/src/tools/workspace/*", "crates/agent/src/tools/search.rs", "crates/agent/src/tools/search/*", "crates/agent/src/tools/spill.rs",
        "crates/agent/src/tools/dedup.rs", "crates/agent/src/tools/paths.rs",
    ]),
    ("run_command", ["crates/agent/src/tools/command.rs", "crates/agent/src/tools/command/*", "crates/agent/src/shell.rs", "crates/agent/src/askpass.rs", "crates/agent/src/askpass/*"]),
    ("lsp", ["crates/agent/src/tools/lsp.rs", "crates/agent/src/tools/lsp/*", "crates/agent/src/language.rs", "crates/agent/src/language/*"]),
    ("web / browser", ["crates/agent/src/web.rs", "crates/agent/src/web/*", "crates/agent/src/browser.rs", "crates/agent/src/browser/*"]),
    ("skill", ["crates/agent/src/skills.rs", "crates/agent/src/skills/*"]),
    ("task", ["crates/agent/src/child.rs", "crates/agent/src/child/*", "crates/agent/src/runtime/child.rs", "crates/agent/src/runtime/child/*"]),
    ("verify", ["crates/agent/src/verify.rs", "crates/agent/src/verify/*", "crates/agent/src/classify.rs", "crates/agent/src/classify/*"]),
    ("steps", ["crates/agent/src/steps.rs", "crates/agent/src/steps/*", "crates/agent/src/tools/todo.rs"]),
    ("goal", ["crates/agent/src/goal.rs", "crates/agent/src/goal/*", "crates/agent/src/supervise.rs", "crates/agent/src/supervise/*"]),
    ("session", [
        "crates/agent/src/durable.rs", "crates/agent/src/durable/*", "crates/agent/src/session.rs", "crates/agent/src/session/*", "crates/agent/src/checkpoint.rs",
        "crates/agent/src/checkpoint/*", "crates/agent/src/memory.rs", "crates/agent/src/memory/*",
    ]),
    ("worktree", ["crates/agent/src/worktree.rs", "crates/agent/src/worktree/*"]),
    ("workflow", ["crates/agent/src/workflow.rs", "crates/agent/src/workflow/*", "crates/agent/src/review.rs", "crates/agent/src/review/*"]),
    ("policy", ["crates/agent/src/policy.rs", "crates/agent/src/policy/*"]),
    ("endpoint / config", [
        "crates/agent/src/endpoint.rs", "crates/agent/src/endpoint/*", "crates/agent/src/config.rs", "crates/agent/src/config/*", "crates/agent/src/secret.rs",
        "crates/agent/src/secret/*", "crates/agent/src/credentials.rs", "crates/agent/src/modules.rs", "crates/agent/src/prefs.rs", "crates/agent/src/prefs/*",
        "crates/agent/src/catalog.rs", "crates/agent/src/catalog/*", "crates/agent/src/limits.rs", "crates/agent/src/limits/*", "crates/auth/src/*",
    ]),
    ("mcp", ["crates/agent/src/mcp.rs", "crates/agent/src/mcp/*", "crates/agent/src/rpc.rs", "crates/agent/src/rpc/*"]),
    ("mail / acp", ["crates/agent/src/mail.rs", "crates/agent/src/mail/*", "crates/agent/src/acp.rs", "crates/agent/src/acp/*"]),
    ("cron / gateway", ["crates/agent/src/cron.rs", "crates/agent/src/cron/*", "crates/agent/src/gateway.rs", "crates/agent/src/gateway/*", "crates/agent/src/jobs.rs", "crates/agent/src/jobs/*"]),
    ("plugins / hooks", ["crates/agent/src/plugins.rs", "crates/agent/src/plugins/*", "crates/agent/src/hooks.rs", "crates/agent/src/hooks/*"]),
    ("plumbing: tui", ["crates/ui/src/*"]),
    ("plumbing: hub", ["crates/agent/src/hub.rs", "crates/agent/src/hub/*"]),
    ("plumbing: shared", ["crates/core/src/*", "crates/types/src/*", "crates/agent/src/process.rs", "crates/agent/src/process/*", "src/update.rs"]),
    ("plumbing: transport", ["crates/agent/src/provider.rs", "crates/agent/src/provider/*"]),
    ("plumbing: runtime", ["crates/agent/src/runtime.rs", "crates/agent/src/runtime/*"]),
    ("plumbing: tools-core", ["crates/agent/src/tools.rs", "crates/agent/src/tools/*"]),
    ("plumbing: process", [
        "src/lib.rs", "src/main.rs", "src/main/*", "crates/agent/src/error.rs", "crates/agent/src/command.rs", "crates/agent/src/command/*",
        "crates/agent/src/herdr.rs", "crates/agent/src/recent.rs", "crates/agent/src/splash.rs", "crates/agent/src/splash/*", "crates/agent/src/prompt.rs",
        "crates/agent/src/prompt/*", "crates/agent/src/scout.rs", "crates/agent/src/text.rs", "crates/agent/src/text/*",
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
