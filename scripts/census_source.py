"""Rust source reading for the census: which files are production, and
their text with every `#[cfg(test)]` item blanked."""

from __future__ import annotations

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SKIP = re.compile(r"(^|/)(tests|[a-z0-9_]+_tests)/|_tests\.rs$|tests\.rs$")
CFG_TEST = re.compile(r"(?m)^[ \t]*#\[cfg\(((?:[^()]|\([^()]*\))*)\)\][ \t]*")


def rust_files() -> list[Path]:
    trees = [ROOT / "src", *sorted((ROOT / "crates").glob("*/src"))]
    return sorted(p for tree in trees for p in tree.rglob("*.rs"))


def source_files() -> list[Path]:
    return [
        p
        for p in rust_files()
        if not SKIP.search(str(p.relative_to(ROOT)))
    ]


def skip_token(text: str, i: int) -> int:
    n = len(text)
    c = text[i]
    if c == "/" and i + 1 < n and text[i + 1] == "/":
        j = text.find("\n", i)
        return n if j < 0 else j
    if c == '"':
        j = i + 1
        while j < n:
            if text[j] == "\\":
                j += 2
                continue
            if text[j] == '"':
                break
            j += 1
        return j + 1
    if c == "'":
        m = re.match(r"'(?:\\.|[^\\'])'", text[i:])
        if m:
            return i + m.end()
    return i


def block_end(text: str, start: int) -> int:
    """Index just past the braced block beginning at or after `start`."""
    depth = 0
    seen = False
    i = start
    n = len(text)
    while i < n:
        j = skip_token(text, i)
        if j != i:
            i = j
            continue
        c = text[i]
        if c == "{":
            depth += 1
            seen = True
        elif c == "}":
            depth -= 1
            if seen and depth == 0:
                return i + 1
        i += 1
    return n


def is_test(predicate: str) -> bool:
    """A cfg predicate that holds only in test builds."""
    bare = re.sub(r"not\([^()]*\)", "", predicate)
    return re.search(r"\btest\b", bare) is not None and not predicate.startswith("any(")


def item_end(text: str, start: int) -> int:
    """Index just past the item that begins at `start`: a field or
    initializer ends at a top-level `,`, a statement at `;`, a braced item
    when its block closes, and nothing runs past a `}` it did not open."""
    depth = 0
    i = start
    n = len(text)
    while i < n:
        j = skip_token(text, i)
        if j != i:
            i = j
            continue
        c = text[i]
        if c in "([{":
            depth += 1
        elif c in ")]}":
            if depth == 0:
                return i
            depth -= 1
            if depth == 0 and c == "}":
                return i + 1
        elif depth == 0 and c in ",;":
            return i + 1
        i += 1
    return n


def production(text: str) -> str:
    """The text with every test-only item blanked, line count kept."""
    out = text
    for m in reversed(list(CFG_TEST.finditer(text))):
        if not is_test(m.group(1)):
            continue
        end = item_end(text, m.end())
        span = out[m.start():end]
        out = out[: m.start()] + "\n" * span.count("\n") + out[end:]
    return out


def source_text(path: Path) -> str:
    return production(path.read_text(errors="replace"))
