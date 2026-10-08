#!/usr/bin/env python3
"""Render a summonr splash JSON dump (cell grid of ch/fg/bg) to a PNG.

The dump comes from the env-gated unit test:

    SUMMONR_DUMP=space cargo test --lib dump_splash_json
    python3 scripts/render-splash.py /tmp/summonr-space.json /tmp/space.png

Requires Pillow and fontTools. Cell glyphs are drawn with SF Mono, or Menlo where SF
Mono is missing, so block and box-drawing characters render close to a
real terminal. A third argument draws at that scale with the font at
full size. A cell whose bg is null stays transparent, which is how a
terminal-background session looks.
"""
import json
import sys

from fontTools.ttLib import TTFont
from PIL import Image, ImageDraw, ImageFont

SF_MONO = "/System/Applications/Utilities/Terminal.app/Contents/Resources/Fonts/SF-Mono-{}.otf"
MENLO = "/System/Library/Fonts/Menlo.ttc"
SYMBOLS = "/System/Library/Fonts/Apple Symbols.ttf"
CELL_W, CELL_H, SIZE = 11, 22, 18


def chain(style, size):
    sf = SF_MONO.format(style)
    menlo = (MENLO, {"Regular": 0, "Bold": 1, "RegularItalic": 2, "BoldItalic": 3}[style])
    found = []
    for path, index in ((sf, 0), menlo, (SYMBOLS, 0)):
        try:
            font = ImageFont.truetype(path, size, index=index)
        except OSError:
            continue
        found.append((set(TTFont(path, fontNumber=index).getBestCmap()), font))
    return found


LIGHT, HEAVY = 1, 2
BOX = {
    "─": (0, 0, 1, 1, LIGHT), "━": (0, 0, 1, 1, HEAVY), "│": (1, 1, 0, 0, LIGHT), "┃": (1, 1, 0, 0, HEAVY),
    "┌": (0, 1, 0, 1, LIGHT), "┐": (0, 1, 1, 0, LIGHT), "└": (1, 0, 0, 1, LIGHT), "┘": (1, 0, 1, 0, LIGHT),
    "┏": (0, 1, 0, 1, HEAVY), "┓": (0, 1, 1, 0, HEAVY), "┗": (1, 0, 0, 1, HEAVY), "┛": (1, 0, 1, 0, HEAVY),
    "├": (1, 1, 0, 1, LIGHT), "┤": (1, 1, 1, 0, LIGHT), "┬": (0, 1, 1, 1, LIGHT), "┴": (1, 0, 1, 1, LIGHT), "┼": (1, 1, 1, 1, LIGHT),
    "╭": (0, 1, 0, 1, LIGHT), "╮": (0, 1, 1, 0, LIGHT), "╰": (1, 0, 0, 1, LIGHT), "╯": (1, 0, 1, 0, LIGHT),
}
ROUND = {"╭": 180, "╮": 270, "╰": 90, "╯": 0}


def box(draw, glyph, x, y, cw, ch, fill, scale):
    up, down, left, right, weight = BOX[glyph]
    t = max(1, scale * weight)
    mx, my = x + cw // 2, y + ch // 2
    if glyph in ROUND:
        r = cw // 2
        cx = mx + r if right else mx - r
        cy = my + r if down else my - r
        start = ROUND[glyph]
        draw.arc([cx - r, cy - r, cx + r, cy + r], start, start + 90, fill=fill, width=t)
        top, bottom = (my + r, y + ch - 1) if down else (y, my - r)
        draw.rectangle([mx - t // 2, top, mx - t // 2 + t - 1, bottom], fill=fill)
        return
    if left:
        draw.rectangle([x, my - t // 2, mx + t // 2, my - t // 2 + t - 1], fill=fill)
    if right:
        draw.rectangle([mx - t // 2, my - t // 2, x + cw - 1, my - t // 2 + t - 1], fill=fill)
    if up:
        draw.rectangle([mx - t // 2, y, mx - t // 2 + t - 1, my + t // 2], fill=fill)
    if down:
        draw.rectangle([mx - t // 2, my - t // 2, mx - t // 2 + t - 1, y + ch - 1], fill=fill)


def pick(fonts, glyph):
    return next((font for cmap, font in fonts if ord(glyph[0]) in cmap), fonts[0][1])


def main(src: str, dst: str, scale: int = 1) -> None:
    with open(src) as f:
        doc = json.load(f)
    w, h, cells = doc["w"], doc["h"], doc["cells"]
    cw, ch_h = CELL_W * scale, CELL_H * scale
    faces = {(b, i): chain(("Bold" if b else "Regular") + ("Italic" if i else ""), SIZE * scale) for b in (False, True) for i in (False, True)}
    img = Image.new("RGBA", (w * cw, h * ch_h), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)
    for i, cell in enumerate(cells):
        x, y = (i % w) * cw, (i // w) * ch_h
        if cell.get("bg") is not None:
            draw.rectangle([x, y, x + cw - 1, y + ch_h - 1], fill=tuple(cell["bg"]))
        glyph = cell["ch"]
        if glyph in BOX:
            box(draw, glyph, x, y, cw, ch_h, tuple(cell["fg"]), scale)
        elif glyph and glyph != " ":
            use = pick(faces[(bool(cell.get("bold")), bool(cell.get("italic")))], glyph)
            draw.text((x, y - 2 * scale), glyph, font=use, fill=tuple(cell["fg"]))
    img.save(dst)
    print(dst, img.size)


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2], int(sys.argv[3]) if len(sys.argv) > 3 else 1)
