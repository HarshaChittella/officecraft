#!/usr/bin/env python3
"""Draws Craftworks' app icon (our own artwork, made here in code) into an .iconset folder.

A rounded tile in the window's light surface colour with four small kind tiles (Document blue,
Sheet green, Presentation orange, PDF red, the colours in docs/design.md), each a sheet with a
folded corner. Usage: make_icon.py OUT.iconset
"""
import sys
from pathlib import Path
from PIL import Image, ImageDraw

KINDS = [(0x3B, 0x82, 0xF6), (0x16, 0xA3, 0x4A), (0xD9, 0x65, 0x0B), (0xDC, 0x35, 0x45)]


def draw(size: int) -> Image.Image:
    s = size / 1024
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    # The tile, inset as macOS icons are (824 of 1024), with a soft edge.
    pad = 100 * s
    d.rounded_rectangle([pad, pad, size - pad, size - pad], radius=185 * s, fill=(244, 244, 246, 255), outline=(220, 220, 226, 255), width=max(1, int(6 * s)))
    # Four sheets in a 2×2 grid.
    cell = 300 * s
    gap = 44 * s
    x0 = (size - (2 * cell + gap)) / 2
    for i, colour in enumerate(KINDS):
        cx = x0 + (i % 2) * (cell + gap)
        cy = x0 + (i // 2) * (cell + gap)
        w, h = cell * 0.78, cell
        left = cx + (cell - w) / 2
        fold = w * 0.28
        sheet = [(left, cy), (left + w - fold, cy), (left + w, cy + fold), (left + w, cy + h), (left, cy + h)]
        d.polygon(sheet, fill=colour)
        d.polygon([(left + w - fold, cy), (left + w - fold, cy + fold), (left + w, cy + fold)], fill=(255, 255, 255, 140))
        # Two white lines: text on the sheet.
        for k in range(2):
            y = cy + h * (0.55 + 0.17 * k)
            d.rounded_rectangle([left + w * 0.18, y, left + w * (0.82 - 0.2 * k), y + h * 0.07], radius=h * 0.035, fill=(255, 255, 255, 230))
    return img


def main() -> None:
    out = Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    for base in (16, 32, 128, 256, 512):
        draw(base).save(out / f"icon_{base}x{base}.png")
        draw(base * 2).save(out / f"icon_{base}x{base}@2x.png")


if __name__ == "__main__":
    main()
