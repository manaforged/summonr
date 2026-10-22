#!/usr/bin/env python3
from __future__ import annotations

import re
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
GUIDE = ROOT / "docs" / "guide"
SUMMARY = ROOT / "docs" / "SUMMARY.md"
MODELS = GUIDE / "models.md"
PRESETS = ROOT / "src" / "endpoint" / "presets.toml"
PAGES = [ROOT / "README.md", *sorted(GUIDE.glob("*.md"))]
PARAGRAPH_WORDS = 80
CELL_WORDS = 40


def blocks(text: str):
    fenced = False
    block: list[str] = []
    start = 0
    for number, line in enumerate(text.split("\n"), 1):
        if line.startswith("```"):
            fenced = not fenced
            continue
        if fenced:
            continue
        item = re.match(r"\s*(?:[-*]|\d+\.)\s", line)
        if not line.strip() or line.startswith(("#", "|", "<", "!")) or item:
            if block:
                yield start, " ".join(block)
            block = []
            if item:
                yield number, line
            if line.startswith("|"):
                for cell in line.strip("|").split("|"):
                    yield -number, cell
            continue
        if not block:
            start = number
        block.append(line)
    if block:
        yield start, " ".join(block)


def slug(heading: str) -> str:
    text = re.sub(r"[`*_\[\]()]", "", heading.strip().lower())
    return re.sub(r"[^\w\- ]", "", text).replace(" ", "-")


def anchors(path: Path) -> set[str]:
    text = re.sub(r"```.*?```", "", path.read_text(), flags=re.S)
    return {slug(line.lstrip("#")) for line in text.split("\n") if line.startswith("#")}


def provider_rows() -> list[str]:
    presets = {row["name"]: row for row in tomllib.loads(PRESETS.read_text())["preset"]}
    text = MODELS.read_text()
    table = text[text.index("| Provider | Name |") :].split("\n\n")[0]
    rows = re.findall(r"^\| [^|]+ \| `([\w-]+)` \| [^|]+ \| ([^|]+) \|", table, re.M)
    found = [f"models.md: no provider row for {name}" for name, row in presets.items() if not row.get("off") and name not in dict(rows)]
    for name, keys in rows:
        if name not in presets:
            found.append(f"models.md: {name} is not a preset")
        elif re.findall(r"`([A-Z0-9_]+)`", keys) != presets[name].get("env_keys", []):
            found.append(f"models.md: {name} keys differ from presets.toml")
    return found


def problems(pages: list[Path]) -> list[str]:
    found = provider_rows()
    listed = set(re.findall(r"\]\(guide/([\w-]+\.md)\)", SUMMARY.read_text()))
    for page in pages:
        name = page.relative_to(ROOT)
        if page.parent == GUIDE and page.name not in listed:
            found.append(f"{name}: not in docs/SUMMARY.md")
        text = page.read_text()
        for number, block in blocks(text):
            words = len(re.sub(r"`[^`]*`", "x", block).split())
            limit = CELL_WORDS if number < 0 else PARAGRAPH_WORDS
            if words > limit:
                found.append(f"{name}:{abs(number)}: {words} words, over {limit}")
        prose = re.sub(r"`[^`]*`", "", re.sub(r"```.*?```", "", text, flags=re.S))
        for target, anchor in re.findall(r"\]\((?!https?:|mailto:)([^)#\s]*)(?:#([^)\s]+))?\)", prose):
            resolved = (page.parent / target).resolve() if target else page
            if not resolved.exists():
                found.append(f"{name}: link to missing {target}")
            elif anchor and resolved.suffix == ".md" and anchor not in anchors(resolved):
                found.append(f"{name}: link to missing anchor {target}#{anchor}")
    return found


def main() -> int:
    found = problems([ROOT / name for name in sys.argv[1:]] or PAGES)
    for line in found:
        print(line)
    print("docs-check: clean" if not found else f"docs-check: {len(found)} problems")
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main())
