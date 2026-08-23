#!/usr/bin/env python3
"""Render a mage splash JSON dump (cell grid of ch/fg/bg) to a PNG.

The dump comes from the env-gated unit test:

    MAGE_DUMP=fantasy cargo test --lib dump_splash_json
    python3 scripts/render-splash.py /tmp/mage-fantasy.json /tmp/fantasy.png

Requires Pillow. Cell glyphs are drawn with Menlo, so block and
box-drawing characters render close to a real terminal.
"""
import json
import sys

from PIL import Image, ImageDraw, ImageFont

FONT = "/System/Library/Fonts/Menlo.ttc"
FALLBACK = "/System/Library/Fonts/Apple Symbols.ttf"
# glyphs Menlo lacks; real terminals find them via font fallback
FALLBACK_GLYPHS = set("⎿⏹✻✓✗▁▂▃▄▅▆▇█⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
CELL_W, CELL_H = 11, 22


def main(src: str, dst: str, scale: int = 1) -> None:
    with open(src) as f:
        doc = json.load(f)
    w, h, cells = doc["w"], doc["h"], doc["cells"]
    font = ImageFont.truetype(FONT, 18)
    fallback = ImageFont.truetype(FALLBACK, 18)
    img = Image.new("RGB", (w * CELL_W, h * CELL_H), (12, 12, 20))
    draw = ImageDraw.Draw(img)
    for i, cell in enumerate(cells):
        x, y = (i % w) * CELL_W, (i // w) * CELL_H
        draw.rectangle([x, y, x + CELL_W, y + CELL_H], fill=tuple(cell["bg"]))
        ch = cell["ch"]
        if ch and ch != " ":
            use = fallback if ch in FALLBACK_GLYPHS else font
            draw.text((x, y - 2), ch, font=use, fill=tuple(cell["fg"]))
    if scale != 1:
        img = img.resize((img.width * scale, img.height * scale), Image.NEAREST)
    img.save(dst)
    print(dst, img.size)


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2], int(sys.argv[3]) if len(sys.argv) > 3 else 1)
