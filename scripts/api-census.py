#!/usr/bin/env python3
"""Callable surface census and the API ratchet.

Two jobs:

1. Census. Every `pub`, `pub(crate)`, `pub(super)` item in `src/`, then every
   function with its body length and how many times its name is mentioned,
   then the line count of every file. Writes `off/functions.tsv` and
   `off/lines.tsv`. Totals only, no rows, on stdout.
2. Ratchet. Counts the names on `docs/api/module-api.md` under "Names we
   delete". The counts are shrink-only against `scripts/api-census-baseline.txt`:
   a count may fall, it may never rise. `--update` rewrites the baseline after a
   cut lands.

Item scanning matches the local callable census so the two agree.

    python3 scripts/api-census.py            # census, and ratchet report
    python3 scripts/api-census.py --check    # non-zero if a count rose
    python3 scripts/api-census.py --update   # re-baseline after a cut
"""

from __future__ import annotations

import re
import sys
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SKIP = re.compile(r"(^|/)(tests/)|_tests\.rs$|tests\.rs$")
ITEM = re.compile(
    r"^(pub(?:\((crate|super)\))?)\s+(?:async\s+)?(?:unsafe\s+)?"
    r"(fn|struct|enum|trait|type|const|static|use|mod)\s+(.+)$"
)
FN = re.compile(
    r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:const\s+|async\s+|unsafe\s+)*"
    r"fn\s+([A-Za-z_][A-Za-z0-9_]*)"
)

# One row per name on the delete list. The replacement is what the row keeps.
# A count that rises is a regression; a count that falls is a cut landing.
SOURCE_RULES: list[tuple[str, str, str]] = [
    ("shadow editor apply_replace", r"\bapply_replace\b", "delete"),
    ("shadow editor apply_insert", r"\bapply_insert\b", "delete"),
    ("shadow editor apply_delete", r"\bapply_delete\b", "delete"),
    ("shadow editor join_lines", r"\bjoin_lines\b", "delete"),
    ("orphan token_labels", r"\btoken_labels\b", "delete"),
    ("orphan with_session", r"\bwith_session\b", "delete"),
    ("orphan tool_names", r"\btool_names\b", "delete"),
    ("orphan from_env", r"\bfrom_env\b", "delete"),
    ("orphan file_via_security", r"\bfile_via_security\b", "delete"),
    ("orphan with_security", r"\bwith_security\b", "delete"),
    ("orphan fps_parse", r"\bfps_parse\b", "delete"),
    ("orphan fps_cycle", r"\bfps_cycle\b", "delete"),
    ("orphan set_plan", r"\bset_plan\b", "delete"),
    ("orphan with_plan_first", r"\bwith_plan_first\b", "delete"),
    ("orphan stdout_item", r"\bstdout_item\b", "delete"),
    ("orphan contract_prompt", r"\bcontract_prompt\b", "delete"),
    ("orphan retry_prompt", r"\bretry_prompt\b", "delete"),
    ("orphan add_children", r"\badd_children\b", "delete"),
    ("orphan from_u128", r"\bfrom_u128\b", "delete"),
    ("orphan file_only", r"\bfile_only\b", "delete"),
    ("orphan from_io", r"\bfrom_io\b", "delete"),
    ("orphan last_entry", r"\blast_entry\b", "delete"),
    ("orphan placements", r"\bplacements\b", "delete"),
    ("orphan replay_session", r"\breplay_session\b", "delete"),
    ("session dialect Msg", r"enum Msg\b", "one session event"),
    ("session dialect Cmd", r"enum Cmd\b", "one session event"),
    ("session dialect Line", r"enum Line\b", "one session event"),
    ("session dialect Entry", r"enum Entry\b", "one session event"),
    ("name collision: Backend", r"enum Backend\b", "one word, one domain"),
    ("name collision: Source", r"enum Source\b", "one word, one domain"),
    ("name collision: Kind", r"enum Kind\b", "one word, one domain"),
    ("name collision: Role", r"enum Role\b", "one word, one domain"),
    ("name collision: Event", r"enum Event\b", "one word, one domain"),
    # Base API. Each count may only fall.
    ("clip-shaped helper", r"fn\s+(clip|clip_line|truncate|truncate_line|abbreviate)\b", "check the job, then name it"),
    ("short-shaped helper", r"fn\s+short(_model|_path|_session)?\b", "check the job, then name it"),
    ("wrap-shaped helper", r"fn\s+(wrap|words)\b", "check the job, then name it"),
    ("surface painter", r"fn\s+(grid|paint_bars|paint_source|paint_table)\b", "rows::paint"),
    ("sigil prelude glob", r"use sigil::prelude::\*", "named imports"),
    ("direct file write", r"fs::write\(|File::create", "store::write"),
    # Rounds 2 and 3. Each count may only fall.
    ("legacy tool alias literal", r'"(read_file|edit_file|write_file|list_dir|web_search|web_fetch|replace_text|replace_texts|insert_after|delete_lines)"', "one alias table"),
    ("match wildcard arm", r"_\s*(?:=>|\|\s*\w)", "name the arm"),
    ("duration literal", r"Duration::from_(?:secs|millis|micros)", "one timeout policy"),
    ("timeout mention", r"\btimeout\b", "one timeout policy"),
]

MANIFEST_RULES: list[tuple[str, str, str]] = [
    ("dead dependency unicode-width", r"^\s*unicode-width\s*=", "delete"),
    ("dead dependency winapi-util", r"^\s*winapi-util\s*=", "delete"),
]

BASELINE = ROOT / "scripts" / "api-census-baseline.txt"


def rust_files() -> list[Path]:
    return sorted((ROOT / "src").rglob("*.rs"))


def source_files() -> list[Path]:
    return [
        p
        for p in rust_files()
        if not SKIP.search(str(p.relative_to(ROOT)))
    ]


def block_end(text: str, start: int) -> int:
    """Index just past the braced block beginning at or after `start`."""
    depth = 0
    seen = False
    i = start
    n = len(text)
    while i < n:
        c = text[i]
        if c == "/" and i + 1 < n and text[i + 1] == "/":
            j = text.find("\n", i)
            i = n if j < 0 else j
            continue
        if c == '"':
            j = i + 1
            while j < n:
                if text[j] == "\\":
                    j += 2
                    continue
                if text[j] == '"':
                    break
                j += 1
            i = j + 1
            continue
        if c == "'":
            m = re.match(r"'(?:\\.|[^\\'])'", text[i:])
            if m:
                i += m.end()
                continue
        if c == "{":
            depth += 1
            seen = True
        elif c == "}":
            depth -= 1
            if seen and depth == 0:
                return i + 1
        i += 1
    return n


def items() -> list[tuple[str, str, str, str]]:
    rows: list[tuple[str, str, str, str]] = []
    for path in source_files():
        rel = str(path.relative_to(ROOT))
        for i, line in enumerate(path.read_text(errors="replace").splitlines(), 1):
            m = ITEM.match(line.strip())
            if not m:
                continue
            vis, kind, rest = m.group(1), m.group(3), m.group(4)
            name = (
                rest.split("(", 1)[0].strip()
                if kind == "fn"
                else rest.rstrip(";").strip()[:80]
                if kind == "use"
                else re.split(r"[<{(\s;]", rest, maxsplit=1)[0]
            )
            if name:
                rows.append((vis, kind, name, f"{rel}:{i}"))
    return rows


def functions() -> list[tuple[str, int, str, int, int]]:
    text = {p: p.read_text(errors="replace") for p in source_files()}
    blob = "\n".join(text.values())
    rows: list[tuple[str, int, str, int, int]] = []
    for path, body in text.items():
        rel = str(path.relative_to(ROOT))
        lines = body.splitlines(keepends=True)
        offs = [0]
        for line in lines:
            offs.append(offs[-1] + len(line))
        for i, line in enumerate(lines):
            m = FN.match(line)
            if not m:
                continue
            name = m.group(1)
            start = offs[i] + m.start()
            end = block_end(body, offs[i])
            loc = body[start:end].count("\n") + 1
            mentions = len(re.findall(r"\b" + re.escape(name) + r"\b", blob))
            rows.append((rel, i + 1, name, loc, mentions))
    return rows


def tree_lines() -> list[tuple[str, str, int]]:
    rows: list[tuple[str, str, int]] = []
    for path in rust_files():
        rel = str(path.relative_to(ROOT))
        kind = "test" if SKIP.search(rel) else "source"
        rows.append((rel, kind, len(path.read_text(errors="replace").splitlines())))
    return rows


def ratchet() -> list[tuple[str, str, int]]:
    blob = "\n".join(p.read_text(errors="replace") for p in source_files())
    manifest = (ROOT / "Cargo.toml").read_text(errors="replace")
    hits = [
        (label, keep, len(re.findall(pattern, blob)))
        for label, pattern, keep in SOURCE_RULES
    ]
    hits += [
        (label, keep, len(re.findall(pattern, manifest, re.MULTILINE)))
        for label, pattern, keep in MANIFEST_RULES
    ]
    return hits


def read_baseline() -> dict[str, int]:
    if not BASELINE.is_file():
        return {}
    out: dict[str, int] = {}
    for line in BASELINE.read_text().splitlines():
        if not line.strip() or line.startswith("#"):
            continue
        fields = line.split("\t")
        if len(fields) < 2:
            continue
        label, count = fields[0], fields[1]
        out[label] = int(count)
    return out


def write_baseline(hits: list[tuple[str, str, int]]) -> None:
    lines = [
        "# Shrink-only. Rewrite with: python3 scripts/api-census.py --update",
        "# label\tcount\tkeep",
    ]
    lines += [f"{label}\t{count}\t{keep}" for label, keep, count in hits]
    BASELINE.write_text("\n".join(lines) + "\n")


def main() -> int:
    args = set(sys.argv[1:])
    check = "--check" in args
    update = "--update" in args

    rows = items()
    fns = functions()
    tree = tree_lines()
    pub = sum(1 for r in rows if r[0] == "pub")
    crate = sum(1 for r in rows if r[0] == "pub(crate)")
    sup = sum(1 for r in rows if r[0] == "pub(super)")
    kinds = Counter(r[1] for r in rows)
    orphans = [f for f in fns if f[4] <= 1 and f[2] not in {"main"}]

    print(f"items {len(rows)} (pub {pub}, pub(crate) {crate}, pub(super) {sup})")
    print("by kind: " + ", ".join(f"{k} {n}" for k, n in kinds.most_common()))
    print(f"functions {len(fns)}, bodies with no other mention {len(orphans)}")
    source = [r for r in tree if r[1] == "source"]
    test = [r for r in tree if r[1] == "test"]
    source_lines = sum(r[2] for r in source)
    test_lines = sum(r[2] for r in test)
    print(
        f"lines {source_lines + test_lines}: source {source_lines} in "
        f"{len(source)} files, test {test_lines} in {len(test)} files"
    )

    out = ROOT / "off" / "functions.tsv"
    out.parent.mkdir(exist_ok=True)
    out.write_text(
        "file\tline\tname\tloc\tmentions\n"
        + "\n".join(f"{f}\t{l}\t{n}\t{loc}\t{m}" for f, l, n, loc, m in fns)
        + "\n"
    )
    (out.parent / "lines.tsv").write_text(
        "file\tkind\tlines\n"
        + "\n".join(f"{f}\t{k}\t{n}" for f, k, n in tree)
        + "\n"
    )

    hits = ratchet()
    live = [(label, keep, count) for label, keep, count in hits if count]
    print(f"delete-list names still present: {len(live)} of {len(hits)}")

    if update or not BASELINE.is_file():
        write_baseline(hits)
        print(f"baseline written: {BASELINE.relative_to(ROOT)}")
        return 0

    base = read_baseline()
    grew = [
        (label, keep, count, base.get(label))
        for label, keep, count in hits
        if count > base.get(label, 0)
    ]
    fell = [
        (label, base[label], count)
        for label, _, count in hits
        if label in base and count < base[label]
    ]

    if fell:
        print("counts fell (re-baseline with --update):")
        for label, was, now in fell:
            print(f"  {label}: {was} -> {now}")

    if grew:
        print("api-census deny:")
        for label, keep, now, was in grew:
            print(f"  {label}: {was} -> {now} (keep {keep})")
        return 1

    if not check:
        for label, keep, count in live:
            print(f"  {count:4}x {label} (keep {keep})")
    print("api-census deny: clean")
    return 0


if __name__ == "__main__":
    sys.exit(main())
