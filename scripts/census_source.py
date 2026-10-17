"""Rust source reading for the census: which files are production, and
their text with every `#[cfg(test)]` item blanked."""

from __future__ import annotations

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SKIP = re.compile(r"(^|/)(tests|[a-z0-9_]+_tests)/|_tests\.rs$|tests\.rs$")
CFG_TEST = re.compile(r"(?m)^\s*#\[cfg\(test\)\]\s*$")


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


def production(text: str) -> str:
    """The text with every `#[cfg(test)]` item blanked, line count kept."""
    out = text
    for m in reversed(list(CFG_TEST.finditer(text))):
        start = m.end()
        semi = text.find(";", start)
        brace = text.find("{", start)
        end = semi + 1 if semi >= 0 and (brace < 0 or semi < brace) else block_end(text, start)
        span = out[m.start():end]
        out = out[: m.start()] + "\n" * span.count("\n") + out[end:]
    return out


def source_text(path: Path) -> str:
    return production(path.read_text(errors="replace"))
