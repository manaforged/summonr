#!/usr/bin/env python3
"""Turn an asciicast from tui-record.py into compact screen frames for a web player.

usage: cast-frames.py RUN.cast OUT.json [--gap SECONDS] [--tick CELLS] [--fast FACTOR] [--fast-from SECONDS] [--base RRGGBB] [--fg RRGGBB]

The cast is replayed through pyte. Output events that arrive within 40 ms
are one frame. Each frame stores only the cells that changed, as runs of
text that share a style; styles are listed once. Idle time between frames
is capped at --gap seconds, and a frame that changes no more than --tick
cells, such as a spinner, plays --fast times quicker once the recording
passes --fast-from seconds, so typing before then keeps its pace. Cells
with no background take --base, and cells with the default foreground
take --fg.
"""
import argparse, json

import pyte

NAMED = {"black": "000000", "red": "cd3131", "green": "0dbc79", "yellow": "e5e510", "blue": "2472c8", "magenta": "bc3fbc", "cyan": "11a8cd", "white": "e5e5e5", "brown": "e5e510"}


def color(value, fallback):
    if value == "default":
        return fallback
    if len(value) == 6 and all(c in "0123456789abcdef" for c in value):
        return value
    return NAMED.get(value, fallback)


def snapshot(screen, fg, base):
    cells = []
    for y in range(screen.lines):
        row = screen.buffer[y]
        for x in range(screen.columns):
            c = row[x]
            front, back = (c.bg, c.fg) if c.reverse else (c.fg, c.bg)
            cells.append((c.data or " ", color(front, fg), color(back, base), c.bold, c.italics))
    return cells


def runs(previous, current, columns, styles, index):
    out = []
    for y in range(len(current) // columns):
        x = 0
        while x < columns:
            at = y * columns + x
            if previous is not None and previous[at] == current[at]:
                x += 1
                continue
            style = current[at][1:]
            key = index.setdefault(style, len(styles))
            if key == len(styles):
                styles.append(list(style))
            text = ""
            start = x
            while x < columns:
                at = y * columns + x
                if current[at][1:] != style or (previous is not None and previous[at] == current[at] and x > start):
                    break
                text += current[at][0]
                x += 1
            out.append([y, start, key, text])
    return out


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("cast")
    parser.add_argument("out")
    parser.add_argument("--gap", type=float, default=1.2)
    parser.add_argument("--tick", type=int, default=0)
    parser.add_argument("--fast", type=float, default=1.0)
    parser.add_argument("--fast-from", type=float, default=0.0)
    parser.add_argument("--base", default="0c0c14")
    parser.add_argument("--fg", default="e6e6e6")
    args = parser.parse_args()
    lines = open(args.cast).read().splitlines()
    head = json.loads(lines[0])
    screen = pyte.Screen(head["width"], head["height"])
    stream = pyte.ByteStream(screen)
    events = [json.loads(line) for line in lines[1:]]
    styles, index, frames = [], {}, []
    previous, clock, last_time, pending_at = None, 0.0, 0.0, None
    for at, kind, data in events + [[None, "o", ""]]:
        if pending_at is not None and (at is None or at - pending_at > 0.04):
            current = snapshot(screen, args.fg, args.base)
            changed = runs(previous, current, head["width"], styles, index)
            if changed:
                cells = sum(len(run[3]) for run in changed)
                step = min(pending_at - last_time, args.gap)
                quick = cells <= args.tick and pending_at >= args.fast_from
                clock += step / args.fast if quick else step
                last_time = pending_at
                frames.append([round(clock, 3), changed])
                previous = current
            pending_at = None
        if at is None:
            break
        if kind == "o":
            stream.feed(data.encode("utf-8"))
            pending_at = at if pending_at is None else pending_at
    json.dump({"cols": head["width"], "rows": head["height"], "base": args.base, "styles": styles, "frames": frames}, open(args.out, "w"), separators=(",", ":"))
    print(args.out, len(frames), "frames", round(clock, 1), "s")


main()
