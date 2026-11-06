#!/usr/bin/env python3
"""Draw the launcher's folder icons (32x32 RGBA, rounded squares like the
app icons): assets/icons/folder-{games,generative,radio,folder}.png.
Drawn at 8x and downsampled. Needs Pillow. Run from apps/."""
import math
from PIL import Image, ImageDraw

S = 256  # drawing size; output 32

def base(top, bottom):
    g = Image.new("RGBA", (S, S))
    for y in range(S):
        t = y / (S - 1)
        c = tuple(int(top[i] + (bottom[i] - top[i]) * t) for i in range(3)) + (255,)
        ImageDraw.Draw(g).line([(0, y), (S, y)], fill=c)
    m = Image.new("L", (S, S), 0)
    ImageDraw.Draw(m).rounded_rectangle([4, 4, S - 5, S - 5], radius=56, fill=255)
    out = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    out.paste(g, (0, 0), m)
    return out

W = (255, 255, 255, 255)

def games():
    im = base((236, 72, 112), (176, 38, 92)); d = ImageDraw.Draw(im)
    d.rounded_rectangle([36, 82, 220, 178], radius=46, fill=W)
    c = (196, 52, 100, 255)
    d.rectangle([66, 120, 110, 136], fill=c); d.rectangle([80, 106, 96, 150], fill=c)
    d.ellipse([150, 104, 172, 126], fill=c); d.ellipse([174, 128, 196, 150], fill=c)
    return im

def generative():
    im = base((255, 160, 40), (226, 92, 30)); d = ImageDraw.Draw(im)
    pts = [(x, 128 - 46 * math.sin((x - 40) / 176 * 2 * math.pi) * (0.55 + 0.45 * math.cos((x - 40) / 176 * math.pi))) for x in range(40, 217, 2)]
    d.line(pts, fill=W, width=18, joint="curve")
    for (x, y, r) in [(70, 64, 12), (190, 70, 9), (176, 190, 12), (60, 186, 8)]:
        d.ellipse([x - r, y - r, x + r, y + r], fill=W)
    return im

def radio():
    im = base((40, 200, 160), (10, 140, 120)); d = ImageDraw.Draw(im)
    cx, cy = 128, 150
    for r in (44, 82):
        d.arc([cx - r, cy - r, cx + r, cy + r], 215, 325, fill=W, width=18)
    d.ellipse([cx - 16, cy - 16, cx + 16, cy + 16], fill=W)
    d.polygon([(cx - 10, cy + 6), (cx + 10, cy + 6), (cx + 26, 222), (cx - 26, 222)], fill=W)
    return im

def folder():
    im = base((110, 150, 220), (60, 96, 176)); d = ImageDraw.Draw(im)
    d.rounded_rectangle([46, 72, 120, 104], radius=12, fill=W)
    d.rounded_rectangle([46, 88, 210, 190], radius=16, fill=W)
    return im

if __name__ == "__main__":
    for name, f in [("games", games), ("generative", generative), ("radio", radio), ("folder", folder)]:
        f().resize((32, 32), Image.LANCZOS).save(f"assets/icons/folder-{name}.png", optimize=True)
        print(f"assets/icons/folder-{name}.png")
