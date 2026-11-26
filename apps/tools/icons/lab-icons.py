#!/usr/bin/env python3
# Copyright (C) 2026 Paul Lambert
# SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
"""Draw the test7 app icons (32x32 RGBA, rounded squares like the other
app icons): packages/{usb-lab,ble-lab,wifi-lab,synth}/icon.png.
Drawn at 8x and downsampled. Needs Pillow. Run from apps/."""
import math
from PIL import Image, ImageDraw

S = 256
W = (255, 255, 255, 255)

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

def usb():
    im = base((96, 110, 130), (52, 62, 80)); d = ImageDraw.Draw(im)
    d.line([(128, 50), (128, 200)], fill=W, width=16)
    d.polygon([(128, 30), (100, 70), (156, 70)], fill=W)
    d.line([(128, 150), (78, 110), (78, 90)], fill=W, width=14)
    d.ellipse([62, 66, 94, 98], fill=W)
    d.line([(128, 130), (178, 100), (178, 84)], fill=W, width=14)
    d.rectangle([162, 60, 194, 90], fill=W)
    d.ellipse([106, 188, 150, 232], fill=W)
    return im

def arcs(d, cx, cy, n, w):
    for i in range(1, n + 1):
        r = 30 + i * 30
        d.arc([cx - r, cy - r, cx + r, cy + r], 225, 315, fill=W, width=w)

def ble():
    im = base((40, 120, 240), (20, 60, 170)); d = ImageDraw.Draw(im)
    x0, x1, x2 = 104, 128, 160
    pts = [(x0, 92), (x2, 152), (x1, 184), (x1, 72), (x2, 104), (x0, 164)]
    d.line(pts, fill=W, width=16, joint="curve")
    return im

def wifi():
    im = base((40, 190, 150), (20, 120, 100)); d = ImageDraw.Draw(im)
    arcs(d, 128, 196, 3, 18)
    d.ellipse([112, 180, 144, 212], fill=W)
    return im

def synth():
    im = base((170, 80, 230), (110, 40, 170)); d = ImageDraw.Draw(im)
    d.rounded_rectangle([40, 70, 216, 196], radius=14, fill=W)
    c = (110, 40, 170, 255)
    for i in range(1, 6):
        x = 40 + i * 176 // 6
        d.line([(x, 70), (x, 196)], fill=c, width=5)
    for i in (1, 2, 4, 5):
        x = 40 + i * 176 // 6
        d.rectangle([x - 11, 70, x + 11, 140], fill=c)
    return im

for name, f in [("usb-lab", usb), ("ble-lab", ble), ("wifi-lab", wifi), ("synth", synth)]:
    f().resize((32, 32), Image.LANCZOS).save(f"packages/{name}/icon.png")
    print(f"packages/{name}/icon.png")
