"""Render the Home tab's plate-less Q from logo/icon.svg's rect geometry.

The Light theme's mark (crates/app/assets/logo-q-light.png) keeps the teal
gradient, paints the faded side opaque white and edges every rect in black,
4/256 of the viewBox wide, inside the rx-3 rounded rect. The frame is the one
crates/app/assets/logo-q.png uses: the glyph scaled by 4/3 about its centre
into a 256-texel square, straight (unpremultiplied) alpha.

    python tools/logo/light_mark.py            # write logo-q-light.png
    python tools/logo/light_mark.py --check    # also render today's dark Q and
                                               # compare it with logo-q.png

Requires Pillow and NumPy.
"""

import re
import sys
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parents[2]
SVG = ROOT / "logo" / "icon.svg"
ASSETS = ROOT / "crates" / "app" / "assets"

SIZE = 256          # texels of the output square
SUPER = 8           # supersampling factor per axis
SCALE = 4 / 3       # viewBox units to texels
GLYPH_CENTRE = (129.5, 133.0)  # centre of the Q's rect box in viewBox units
OUTLINE = 4         # viewBox units, drawn inside each rect
RADIUS = 3          # viewBox units, as every rect's rx in icon.svg
TEAL = ("#3AF0B2", "#12C68C")
LIGHT_FADE = ("#FFFFFF", 1.0)
DARK_FADE = ("#E6EEF6", 0.647)
EDGE = "#000000"


def rgb(hex_colour):
    return np.array([int(hex_colour[i:i + 2], 16) for i in (1, 3, 5)], dtype=np.float64)


def svg_rects():
    """The Q's rects in icon.svg, split into the teal and the faded side."""
    teal, fade = [], []
    for tag in re.findall(r"<rect [^>]*/>", SVG.read_text(encoding="utf-8")):
        attrs = dict(re.findall(r'([\w-]+)="([^"]*)"', tag))
        box = tuple(float(attrs[k]) for k in ("x", "y", "width", "height"))
        if attrs["fill"] == "url(#tg)":
            teal.append(box)
        elif attrs["fill"].startswith("#"):
            fade.append(box)
    return teal, fade


def to_texels(x, y):
    """A viewBox point in supersampled texels."""
    cx, cy = GLYPH_CENTRE
    k = SUPER * SCALE
    return (x - cx) * k + SIZE * SUPER / 2, (y - cy) * k + SIZE * SUPER / 2


def mask(box, inset):
    """Coverage of one rounded rect, shrunk by `inset` viewBox units."""
    x, y, w, h = box
    x0, y0 = to_texels(x + inset, y + inset)
    x1, y1 = to_texels(x + w - inset, y + h - inset)
    radius = max(RADIUS - inset, 0) * SUPER * SCALE
    layer = Image.new("L", (SIZE * SUPER, SIZE * SUPER), 0)
    ImageDraw.Draw(layer).rounded_rectangle([x0, y0, x1, y1], radius=radius, fill=255)
    return np.asarray(layer, dtype=np.float64) / 255


def fill_colour(box, colour):
    """Per-pixel straight RGB of a fill: flat, or the SVG's per-rect vertical gradient."""
    n = SIZE * SUPER
    if isinstance(colour, tuple):
        top, bottom = rgb(colour[0]), rgb(colour[1])
        _, y0 = to_texels(0, box[1])
        _, y1 = to_texels(0, box[1] + box[3])
        t = np.clip((np.arange(n) + 0.5 - y0) / (y1 - y0), 0, 1)[:, None, None]
        return np.broadcast_to(top + (bottom - top) * t, (n, n, 3))
    return np.broadcast_to(rgb(colour), (n, n, 3))


def render(fade, outline):
    """Premultiplied RGBA at supersampled resolution, rects composited source-over."""
    n = SIZE * SUPER
    out = np.zeros((n, n, 4))
    teal, faded = svg_rects()
    layers = [(box, TEAL, 1.0) for box in teal] + [(box, fade[0], fade[1]) for box in faded]
    for box, colour, opacity in layers:
        outer = mask(box, 0) * opacity
        if outline:
            inner = mask(box, OUTLINE) * opacity
            colour_px = fill_colour(box, colour) * inner[..., None] + rgb(EDGE) * (outer - inner)[..., None]
        else:
            colour_px = fill_colour(box, colour) * outer[..., None]
        src = np.dstack([colour_px, outer * 255])
        out = src + out * (1 - outer[..., None])
    return out


def downsample(premultiplied):
    """Box-filter to SIZE and store straight alpha, as logo-q.png does."""
    small = premultiplied.reshape(SIZE, SUPER, SIZE, SUPER, 4).mean(axis=(1, 3))
    alpha = small[..., 3]
    straight = np.where(alpha[..., None] > 0, small[..., :3] * 255 / np.maximum(alpha, 1e-9)[..., None], 0)
    return np.dstack([np.clip(straight, 0, 255), alpha]).round().astype(np.uint8)


def main():
    light = downsample(render(LIGHT_FADE, outline=True))
    Image.fromarray(light, "RGBA").save(ASSETS / "logo-q-light.png", optimize=True)
    print("wrote", ASSETS / "logo-q-light.png")
    if "--check" in sys.argv:
        dark = downsample(render(DARK_FADE, outline=False)).astype(int)
        shipped = np.asarray(Image.open(ASSETS / "logo-q.png").convert("RGBA")).astype(int)
        a = dark[..., 3]
        print("dark alpha diff: max", np.abs(a - shipped[..., 3]).max(), "mean", round(np.abs(a - shipped[..., 3]).mean(), 3))


if __name__ == "__main__":
    main()
