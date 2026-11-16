#!/usr/bin/env python3
"""Draws Craftworks' icons (our own artwork, made here in code) in the Liquid Glass manner:
a tinted glass tile lit from above (a sheen across its top, a bright rim that fades down its
sides, a shade along its foot, a soft shadow under it), and on it the glyph as a pane of
frosted white glass floating over a fainter pane behind.

    make_icons.py            writes assets/icons/*.png (what the app embeds)
    make_icons.py --sheet F  also writes a contact sheet to F, for looking at them

`app_master()` is the app icon's 1024 px master (packaging/macos/make_icon.py uses it).
"""
import math
import sys
from pathlib import Path
from PIL import Image, ImageChops, ImageDraw, ImageFilter, ImageFont

HERE = Path(__file__).resolve().parent
FONT = HERE.parents[2] / "vendor/craft-fonts/fonts/inter/Inter-Bold.ttf"
SS = 4  # supersampling

# Tile tints: (top, bottom).
BLUE = ((0x6F, 0x92, 0xFF), (0x2A, 0x49, 0xD1))
GREEN = ((0x42, 0xD4, 0x86), (0x0B, 0x7A, 0x3E))
ORANGE = ((0xFF, 0xA3, 0x57), (0xD6, 0x4C, 0x0C))
RED = ((0xFF, 0x74, 0x74), (0xC9, 0x26, 0x2C))
AMBER = ((0xFF, 0xCF, 0x5C), (0xE3, 0x8B, 0x00))
TEAL = ((0x4D, 0xD6, 0xD6), (0x0A, 0x7B, 0x8A))
VIOLET = ((0xB7, 0x96, 0xFF), (0x6A, 0x3F, 0xD9))


def squircle(n: int, inset: float) -> Image.Image:
    """A superellipse mask (the continuous-corner tile), `inset` px from each edge."""
    m = Image.new("L", (n, n), 0)
    c, r, e = n / 2, n / 2 - inset, 5.0
    pts = []
    for i in range(720):
        a = 2 * math.pi * i / 720
        ca, sa = math.cos(a), math.sin(a)
        pts.append((c + r * math.copysign(abs(ca) ** (2 / e), ca), c + r * math.copysign(abs(sa) ** (2 / e), sa)))
    ImageDraw.Draw(m).polygon(pts, fill=255)
    return m


def vgrad(n: int, top, bottom, y0=0.0, y1=1.0) -> Image.Image:
    col = Image.new("RGBA", (1, n))
    for y in range(n):
        t = min(1.0, max(0.0, (y / n - y0) / (y1 - y0)))
        col.putpixel((0, y), tuple(int(a + (b - a) * t) for a, b in zip(top, bottom)))
    return col.resize((n, n))


def scaled(mask: Image.Image, k: float) -> Image.Image:
    return mask.point(lambda v: int(v * k))


def over(base: Image.Image, colour, alpha: Image.Image) -> Image.Image:
    layer = Image.new("RGBA", base.size, tuple(colour) + (0,))
    layer.putalpha(alpha)
    return Image.alpha_composite(base, layer)


def glass_tile(n: int, tint) -> Image.Image:
    """The tinted glass tile at n x n (already supersampled by the caller)."""
    top, bottom = tint
    inset = n * 0.085
    mask = squircle(n, inset)
    out = Image.new("RGBA", (n, n), (0, 0, 0, 0))
    # Its shadow: soft, a little below, in a dark shade of the tint.
    shade = tuple(int(c * 0.35) for c in bottom)
    sh = ImageChops.offset(mask.filter(ImageFilter.GaussianBlur(n * 0.028)), 0, int(n * 0.026))
    out = over(out, shade, scaled(sh, 0.42))
    # The body.
    body = vgrad(n, top + (255,), bottom + (255,), 0.08, 0.92)
    out.paste(body, (0, 0), mask)
    # A sheen across the top: a wide soft ellipse of light.
    sheen = Image.new("L", (n, n), 0)
    ImageDraw.Draw(sheen).ellipse([n * -0.1, n * -0.42, n * 1.1, n * 0.46], fill=255)
    sheen = ImageChops.multiply(sheen.filter(ImageFilter.GaussianBlur(n * 0.05)), mask)
    out = over(out, (255, 255, 255), scaled(sheen, 0.26))
    # A shade along the foot.
    foot = ImageChops.multiply(vgrad(n, (0,) * 4, (255,) * 4, 0.62, 1.0).getchannel("R"), mask)
    out = over(out, shade, scaled(foot, 0.30))
    # The rim: bright along the top, fading down the sides; a faint bounce of light at the foot.
    ring = ImageChops.subtract(mask, squircle(n, inset + n * 0.014))
    lit = ImageChops.multiply(ring, vgrad(n, (255,) * 4, (0,) * 4, 0.05, 0.6).getchannel("R"))
    out = over(out, (255, 255, 255), scaled(lit, 0.85))
    bounce = ImageChops.multiply(ring, vgrad(n, (0,) * 4, (255,) * 4, 0.7, 0.95).getchannel("R"))
    out = over(out, (255, 255, 255), scaled(bounce, 0.22))
    return out


class Glyph:
    """Draws on a 100-unit grid over the tile: frosted white panes and marks in the tint."""

    def __init__(self, n: int, tint):
        self.n, self.tint = n, tint
        self.u = n / 100
        self.back = Image.new("L", (n, n), 0)   # the fainter pane behind
        self.front = Image.new("L", (n, n), 0)  # the pane in front
        self.marks = Image.new("RGBA", (n, n), (0, 0, 0, 0))  # tinted marks on the front pane
        self.deep = tuple(int(c * 0.92) for c in tint[1])

    def _box(self, xy):
        u = self.u
        return [xy[0] * u, xy[1] * u, xy[2] * u, xy[3] * u]

    def pane(self, xy, r, back=False):
        ImageDraw.Draw(self.back if back else self.front).rounded_rectangle(self._box(xy), radius=r * self.u, fill=255)

    def poly(self, pts, back=False, fill=255):
        ImageDraw.Draw(self.back if back else self.front).polygon([(x * self.u, y * self.u) for x, y in pts], fill=fill)

    def ellipse(self, xy, fill=255):
        ImageDraw.Draw(self.front).ellipse(self._box(xy), fill=fill)

    def stroke(self, pts, w):
        d, u = ImageDraw.Draw(self.front), self.u
        p = [(x * u, y * u) for x, y in pts]
        d.line(p, fill=255, width=int(w * u), joint="curve")
        for x, y in p:
            d.ellipse([x - w * u / 2, y - w * u / 2, x + w * u / 2, y + w * u / 2], fill=255)

    def mark(self, xy, r, alpha=255):
        ImageDraw.Draw(self.marks).rounded_rectangle(self._box(xy), radius=r * self.u, fill=self.deep + (alpha,))

    def text(self, s, centre, size):
        font = ImageFont.truetype(str(FONT), int(size * self.u))
        ImageDraw.Draw(self.marks).text((centre[0] * self.u, centre[1] * self.u), s, font=font, fill=(255, 255, 255, 255), anchor="mm")

    def onto(self, tile: Image.Image) -> Image.Image:
        n = self.n
        shade = tuple(int(c * 0.3) for c in self.tint[1])
        out = tile
        # The pane behind: faint, as glass seen through glass.
        if self.back.getbbox():
            out = over(out, (255, 255, 255), scaled(self.back, 0.34))
        # The front pane's shadow on the tile, then the pane, a touch clearer towards its foot.
        sh = ImageChops.offset(self.front.filter(ImageFilter.GaussianBlur(n * 0.022)), 0, int(n * 0.02))
        out = over(out, shade, scaled(sh, 0.40))
        fade = vgrad(n, (255,) * 4, (224,) * 4, 0.2, 0.85).getchannel("R")
        out = over(out, (255, 255, 255), ImageChops.multiply(self.front, fade))
        # Marks belong to the front pane only.
        m = self.marks.copy()
        m.putalpha(ImageChops.multiply(m.getchannel("A"), self.front))
        return Image.alpha_composite(out, m)


def page(g: Glyph, x0, y0, x1, y1, fold=11.0, back=False):
    """An upright page with its top-right corner turned down."""
    g.pane((x0, y0 + fold, x1, y1), 5.5, back)
    g.pane((x0, y0, x1 - fold, y1 - 10), 5.5, back)
    g.poly([(x1 - fold - 0.5, y0), (x1, y0 + fold + 0.5), (x1 - fold - 0.5, y0 + fold + 0.5)], back)
    if not back:
        # The turned corner, shaded.
        ImageDraw.Draw(g.marks).polygon([(p[0] * g.u, p[1] * g.u) for p in [(x1 - fold, y0), (x1, y0 + fold), (x1 - fold, y0 + fold)]], fill=g.deep + (70,))


def doc(g: Glyph):
    page(g, 37, 21, 77, 73, back=True)
    page(g, 25, 27, 65, 81)
    for y, x1 in [(46, 57), (55, 57), (64, 48)]:
        g.mark((32, y, x1, y + 4.2), 2.1)


def sheet(g: Glyph):
    g.pane((28, 20, 82, 66), 7, back=True)
    g.pane((19, 30, 75, 78), 7)
    g.mark((19, 30, 75, 42.5), 0, 150)  # the header row
    for y in (54, 65.5):
        g.mark((19, y, 75, y + 1.9), 0, 190)
    for x in (37, 55.5):
        g.mark((x, 42.5, x + 1.9, 78), 0, 190)


def deck(g: Glyph):
    g.pane((27, 19, 85, 58), 6.5, back=True)
    g.pane((16, 29, 76, 69), 6.5)
    for x, top in [(28, 53), (41, 40), (54, 47)]:
        g.mark((x, top, x + 8, 61), 2.2)
    g.stroke([(46, 69), (46, 78)], 4.5)
    g.stroke([(35, 80), (57, 80)], 4.5)


def pdf(g: Glyph):
    page(g, 37, 21, 77, 73, back=True)
    page(g, 25, 27, 65, 81)
    for y, x1 in [(41, 50), (49, 57)]:
        g.mark((32, y, x1, y + 4.2), 2.1, 150)
    g.mark((30, 58, 60, 74), 4)
    g.text("PDF", (45, 66.4), 11.5)


def home(g: Glyph):
    g.poly([(50, 20), (18, 49), (26, 56), (50, 34), (74, 56), (82, 49)])
    g.poly([(50, 34), (29, 53), (71, 53)])
    g.pane((29, 50, 71, 80), 6)
    g.mark((43.5, 60, 56.5, 80), 3)


def folder(g: Glyph):
    g.pane((19, 26, 50, 44), 6, back=True)
    g.pane((19, 34, 81, 76), 8, back=True)
    g.pane((19, 41, 81, 78), 8)


def project(g: Glyph):
    folder(g)
    g.mark((30, 53, 62, 57.5), 2.2)
    g.mark((30, 63, 51, 67.5), 2.2)


def check(g: Glyph):
    g.stroke([(29, 52), (43, 66), (72, 35)], 10)


def alert(g: Glyph):
    g.pane((44.5, 22, 55.5, 60), 5.5)
    g.ellipse((43.5, 67, 56.5, 80))


def star(g: Glyph, cx, cy, r):
    pts = []
    for i in range(96):
        a = 2 * math.pi * i / 96
        ca, sa = math.cos(a), math.sin(a)
        pts.append((cx + r * math.copysign(abs(ca) ** 3.2, ca), cy + r * math.copysign(abs(sa) ** 3.2, sa)))
    g.poly(pts)


def sparkle(g: Glyph):
    star(g, 44, 56, 30)
    star(g, 73, 28, 13)


ICONS = {
    "doc": (BLUE, doc), "sheet": (GREEN, sheet), "deck": (ORANGE, deck), "pdf": (RED, pdf),
    "home": (BLUE, home), "folder": (AMBER, folder), "project": (TEAL, project),
    "check": (GREEN, check), "alert": (RED, alert), "sparkle": (VIOLET, sparkle),
}  # fmt: skip


def icon(name: str, size: int) -> Image.Image:
    tint, draw = ICONS[name]
    n = size * SS
    g = Glyph(n, tint)
    draw(g)
    return g.onto(glass_tile(n, tint)).resize((size, size), Image.LANCZOS)


def app_master() -> Image.Image:
    """The app icon, 1024 px: a fan of four glass sheets on a deep blue glass tile. The white
    page of text in front, and behind it a green, an orange and a red sheet: one app, four
    kinds of work."""
    n = 1024 * 2
    out = glass_tile(n, ((0x5F, 0x80, 0xFF), (0x1E, 0x32, 0xA8)))
    tile_mask = squircle(n, n * 0.085)
    u = n / 1024

    def sheet_layer(colours, angle, lines=False):
        layer = Image.new("RGBA", (n, n), (0, 0, 0, 0))
        m = Image.new("L", (n, n), 0)
        w, h = 372 * u, 500 * u
        x0, y0 = n / 2 - w / 2 - 26 * u, n / 2 - h / 2 - 6 * u
        box = [x0, y0, x0 + w, y0 + h]
        ImageDraw.Draw(m).rounded_rectangle(box, radius=48 * u, fill=255)
        layer.paste(vgrad(n, colours[0] + (255,), colours[1] + (255,), y0 / n, (y0 + h) / n), (0, 0), m)
        # Its own rim of light along the top edge.
        inner = Image.new("L", (n, n), 0)
        ImageDraw.Draw(inner).rounded_rectangle([x0 + 5 * u, y0 + 5 * u, x0 + w - 5 * u, y0 + h - 5 * u], radius=44 * u, fill=255)
        rim = ImageChops.multiply(ImageChops.subtract(m, inner), vgrad(n, (255,) * 4, (0,) * 4, y0 / n, (y0 + h * 0.6) / n).getchannel("R"))
        layer = over(layer, (255, 255, 255), scaled(rim, 0.8))
        if lines:
            d = ImageDraw.Draw(layer)
            lx, top = x0 + 62 * u, y0 + 86 * u
            d.rounded_rectangle([lx, top, lx + 150 * u, top + 34 * u], radius=17 * u, fill=(0x3B, 0x5B, 0xDB, 255))
            for i, frac in enumerate([1.0, 1.0, 0.86, 0.58]):
                y = top + (92 + 66 * i) * u
                d.rounded_rectangle([lx, y, lx + (w - 124 * u) * frac, y + 24 * u], radius=12 * u, fill=(0xC6, 0xCF, 0xEE, 255))
        return layer.rotate(-angle, resample=Image.BICUBIC, center=(n / 2 - 40 * u, n / 2 + 330 * u))

    for colours, angle in [(RED, 27.0), (ORANGE, 17.0), (GREEN, 7.0), (((255, 255, 255), (0xEC, 0xEF, 0xFA)), -4.0)]:
        layer = sheet_layer(colours, angle, lines=colours[0] == (255, 255, 255))
        a = ImageChops.multiply(layer.getchannel("A"), tile_mask)
        layer.putalpha(a)
        sh = ImageChops.multiply(ImageChops.offset(a.filter(ImageFilter.GaussianBlur(18 * u)), 0, int(12 * u)), tile_mask)
        out = over(out, (6, 12, 60), scaled(sh, 0.36))
        out = Image.alpha_composite(out, layer)
    return out.resize((1024, 1024), Image.LANCZOS)


def main() -> None:
    out = HERE / "icons"
    out.mkdir(exist_ok=True)
    for name in ICONS:
        icon(name, 64).save(out / f"{name}-64.png", optimize=True)
    for name in ("doc", "sheet", "deck", "pdf"):
        icon(name, 256).save(out / f"{name}-256.png", optimize=True)
    app = app_master()
    app.resize((64, 64), Image.LANCZOS).save(out / "mark-64.png", optimize=True)
    app.resize((256, 256), Image.LANCZOS).save(out / "mark-256.png", optimize=True)
    if len(sys.argv) > 2 and sys.argv[1] == "--sheet":
        names = list(ICONS)
        sheet_img = Image.new("RGBA", (4 * 280 + 40, 280 + 2 * 96 + 330), (238, 239, 243, 255))
        for i, name in enumerate(names[:4]):
            sheet_img.alpha_composite(icon(name, 256), (20 + i * 280, 12))
        for i, name in enumerate(names):
            big = icon(name, 64)
            sheet_img.alpha_composite(big, (20 + i * 84, 290))
            sheet_img.alpha_composite(big.resize((32, 32), Image.LANCZOS), (36 + i * 84, 370))
            sheet_img.alpha_composite(big.resize((16, 16), Image.LANCZOS), (44 + i * 84, 414))
        dark = Image.new("RGBA", (sheet_img.width, 96), (30, 30, 34, 255))
        sheet_img.alpha_composite(dark, (0, 450))
        for i, name in enumerate(names):
            sheet_img.alpha_composite(icon(name, 64), (20 + i * 84, 466))
        sheet_img.alpha_composite(app.resize((240, 240), Image.LANCZOS), (20, 556))
        sheet_img.alpha_composite(app.resize((64, 64), Image.LANCZOS), (290, 556))
        sheet_img.alpha_composite(app.resize((32, 32), Image.LANCZOS), (290, 640))
        sheet_img.save(sys.argv[2])


if __name__ == "__main__":
    main()
