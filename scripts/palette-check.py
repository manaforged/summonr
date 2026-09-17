#!/usr/bin/env python3
"""Palette lock. Six presets, twenty tokens each, frozen in a baseline file.

The interface colours are approved as they stand. This script reads the
presets out of src/tui/sigil_view/theme.rs and compares them with
scripts/palette-baseline.txt. Any token that moves is reported by name.

    python3 scripts/palette-check.py            # check, exit 1 on drift
    python3 scripts/palette-check.py --update   # rewrite the baseline
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
THEME = ROOT / "src" / "tui" / "sigil_view" / "theme.rs"
BASELINE = ROOT / "scripts" / "palette-baseline.txt"

PRESETS = {
    "SPACE": "space",
    "FANTASY": "fantasy",
    "NORD": "nord",
    "GRUVBOX": "gruvbox",
    "SOLARIZED": "solarized",
    "LIGHT": "light",
}


def read_presets() -> dict[tuple[str, str], str]:
    text = THEME.read_text()
    blocks = re.findall(r"const (SPACE|FANTASY|NORD|GRUVBOX|SOLARIZED|LIGHT): Self = Self \{(.*?)\n    \};", text, re.S)
    if len(blocks) != len(PRESETS):
        raise SystemExit(f"theme.rs: expected {len(PRESETS)} presets, found {len(blocks)}")
    table: dict[tuple[str, str], str] = {}
    for const, body in blocks:
        preset = PRESETS[const]
        for token, red, green, blue in re.findall(r"(\w+): rgb\((\d+), (\d+), (\d+)\)", body):
            table[(preset, token)] = f"#{int(red):02x}{int(green):02x}{int(blue):02x}"
    return table


def read_baseline() -> dict[tuple[str, str], str]:
    if not BASELINE.is_file():
        return {}
    rows: dict[tuple[str, str], str] = {}
    for line in BASELINE.read_text().splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        preset, token, value = line.split()
        rows[(preset, token)] = value
    return rows


def write(table: dict[tuple[str, str], str]) -> None:
    lines = ["# Frozen interface palette. Rewrite with:", "#   python3 scripts/palette-check.py --update", "# preset token value", ""]
    for (preset, token), value in table.items():
        lines.append(f"{preset} {token} {value}")
    BASELINE.write_text("\n".join(lines) + "\n")


def main() -> int:
    table = read_presets()
    if "--update" in sys.argv:
        write(table)
        print(f"palette baseline: {len(table)} tokens written")
        return 0
    baseline = read_baseline()
    drift = []
    for key, value in table.items():
        if key not in baseline:
            drift.append(f"new {key[0]} {key[1]} = {value}")
        elif baseline[key] != value:
            drift.append(f"{key[0]} {key[1]}: {baseline[key]} -> {value}")
    for key in baseline:
        if key not in table:
            drift.append(f"gone {key[0]} {key[1]}")
    if drift:
        print("palette deny:")
        for row in drift:
            print(f"  {row}")
        print("  the approved palette is frozen; move it on purpose with --update")
        return 1
    print(f"palette clean: {len(table)} tokens match scripts/palette-baseline.txt")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
