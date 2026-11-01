#!/usr/bin/env python3
"""Rewrites Msg:: variants into Msg::Fact(Fact::..) / Msg::Intent(Intent::..)."""
import pathlib
import re
import sys

INTENTS = {"User", "Image", "Reset", "RunMode", "OpenLogin", "OpenHelp", "Resume", "Bye"}

def rewrite(text: str) -> str:
    out = []
    i = 0
    while True:
        hit = re.search(r"Msg::([A-Z][A-Za-z]*)", text[i:])
        if not hit:
            out.append(text[i:])
            break
        start = i + hit.start()
        end = i + hit.end()
        variant = hit.group(1)
        if variant in ("Fact", "Intent"):
            out.append(text[i:end])
            i = end
            continue
        out.append(text[i:start])
        family = "Intent" if variant in INTENTS else "Fact"
        after = text[end:]
        stripped = after.lstrip()
        lead = len(after) - len(stripped)
        if stripped.startswith("("):
            depth = 0
            for at, ch in enumerate(stripped):
                if ch == "(":
                    depth += 1
                elif ch == ")":
                    depth -= 1
                    if depth == 0:
                        break
            body_end = lead + at + 1
            out.append(
                f"Msg::{family}({family}::{variant}"
                + after[:body_end]
                + ")"
            )
            i = end + body_end
        elif stripped.startswith("{"):
            depth = 0
            for at, ch in enumerate(stripped):
                if ch == "{":
                    depth += 1
                elif ch == "}":
                    depth -= 1
                    if depth == 0:
                        break
            body_end = lead + at + 1
            out.append(
                f"Msg::{family}({family}::{variant}"
                + after[:body_end]
                + ")"
            )
            i = end + body_end
        else:
            out.append(f"Msg::{family}({family}::{variant})")
            i = end
    return "".join(out)

for name in sys.argv[1:]:
    path = pathlib.Path(name)
    path.write_text(rewrite(path.read_text()))
    print("rewrote", path)
