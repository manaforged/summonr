#!/usr/bin/env python3
"""Callable surface census and the API ratchet.

Two jobs:

1. Census. Every `pub`, `pub(crate)`, `pub(super)` item in `src/`, then every
   function with its body length and how many times its name is mentioned,
   then the line count of every file. Writes `off/functions.tsv` and
   `off/lines.tsv`. Totals only, no rows, on stdout.
2. Ratchet. Reads the "Names we delete" table on
   `docs/guide/internals/module-api.md` and counts every backticked name in
   its Delete column, plus the pattern rules below and the source files over
   `CAP` lines. The counts are shrink-only
   against `scripts/api-census-baseline.txt`: a count may fall, it may never
   rise. `--update` rewrites the baseline after a cut lands.

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

API_DOC = ROOT / "docs" / "guide" / "internals" / "module-api.md"

SOURCE_RULES: list[tuple[str, str, str]] = [
    ("session dialect Msg", r"enum Msg\b", "one session event"),
    ("session dialect Cmd", r"enum Cmd\b", "one session event"),
    ("session dialect Entry", r"enum Entry\b", "one session event"),
    ("name collision: Kind", r"enum Kind\b", "one word, one domain"),
    ("clip-shaped helper", r"fn\s+(clip|clip_line|truncate|truncate_line|abbreviate)\b", "text::ellipsize"),
    ("short-shaped helper", r"fn\s+short(_model|_path|_session)?\b", "check the job, then name it"),
    ("wrap-shaped helper", r"fn\s+(wrap|words)\b", "check the job, then name it"),
    ("surface painter", r"fn\s+(grid|paint_bars|paint_source|paint_table)\b", "rows::paint"),
    ("sigil prelude glob", r"use sigil::prelude::\*", "named imports"),
    ("legacy tool alias literal", r'"(read_file|edit_file|write_file|list_dir|web_search|web_fetch|replace_text|replace_texts|insert_after|delete_lines)"', "one alias table"),
    ("match wildcard arm", r"(?m)^\s*_\s*(?:=>|\|)", "name the arm"),
    ("timeout mention", r"\btimeout\b", "one timeout policy"),
]


def delete_table() -> list[tuple[str, str, bool]]:
    text = API_DOC.read_text()
    body = text.split("## Names we delete", 1)[1].split("\n#", 1)[0]
    names: list[tuple[str, str, bool]] = []
    for line in body.splitlines():
        cells = [cell.strip() for cell in line.strip().strip("|").split("|")]
        if len(cells) < 3 or cells[0] in {"Delete", "---"}:
            continue
        manifest = "Cargo.toml" in cells[2]
        for token in re.findall(r"`([^`]+)`", cells[0]):
            names.append((token, cells[1], manifest))
    return names


def expand(token: str) -> list[str]:
    m = re.fullmatch(r"(.*::)\{([^}]*)\}", token)
    if not m:
        return [token]
    return [m.group(1) + part.strip() for part in m.group(2).split(",")]


def name_pattern(name: str, manifest: bool) -> str:
    if manifest:
        return r"(?m)^\s*" + re.escape(name) + r"\s*="
    if name.startswith("summonr "):
        return '"' + re.escape(name.split(" ", 1)[1]) + '"'
    if name.startswith("--"):
        return '"' + re.escape(name) + '"'
    if "::" in name:
        return r"(?<![\w:])" + re.escape(name) + r"\b"
    if "." in name:
        return r"\b" + re.escape(name.rsplit(".", 1)[1]) + r"\b"
    return r"\b" + re.escape(name) + r"\b"


def delete_rules() -> tuple[list[tuple[str, str, str]], list[tuple[str, str, str]]]:
    source: list[tuple[str, str, str]] = []
    manifest: list[tuple[str, str, str]] = []
    for token, keep, in_manifest in delete_table():
        for name in expand(token):
            rule = (f"delete {name}", name_pattern(name, in_manifest), keep)
            (manifest if in_manifest else source).append(rule)
    return source, manifest


BASELINE = ROOT / "scripts" / "api-census-baseline.txt"
CAP = 400


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


def ratchet(tree: list[tuple[str, str, int]]) -> list[tuple[str, str, int]]:
    blob = "\n".join(p.read_text(errors="replace") for p in source_files())
    manifest = (ROOT / "Cargo.toml").read_text(errors="replace")
    doc_source, doc_manifest = delete_rules()
    hits = [
        (label, keep, len(re.findall(pattern, blob)))
        for label, pattern, keep in doc_source + SOURCE_RULES
    ]
    hits += [
        (label, keep, len(re.findall(pattern, manifest, re.MULTILINE)))
        for label, pattern, keep in doc_manifest
    ]
    over = sum(1 for _, kind, lines in tree if kind == "source" and lines > CAP)
    hits.append((f"source file over {CAP} lines", "split it", over))
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


def command_drift() -> list[str]:
    specs = (ROOT / "src/command/specs.rs").read_text().split("const ALIASES")[0]
    code = {
        name
        for name, category in re.findall(
            r'spec\(\s*"(/[^"]+)",\s*"[^"]*",\s*SlashCommand::\w+,\s*\w+,\s*(\w+)', specs
        )
        if category != "Developer"
    }
    doc = set(
        re.findall(
            r"^\| `(/[a-z]+(?: accept| reject)?)[ `]",
            (ROOT / "docs/guide/reference.md").read_text(),
            re.M,
        )
    )
    return sorted(f"{name} missing from reference.md" for name in code - doc) + sorted(
        f"{name} in reference.md but not a visible command" for name in doc - code
    )


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
    print(f"files over {CAP} lines: {sum(1 for _, k, n in source if n > CAP)}")

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

    hits = ratchet(tree)
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

    drift = command_drift()
    if drift:
        print("api-census deny: commands table")
        for line in drift:
            print(f"  {line}")
        return 1

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
