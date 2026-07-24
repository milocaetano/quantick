"""Faithful reproduction of quantick's orderflow_render.rs pipeline.

Renders a deterministic synthetic Bookmap scene to PNG so the heatmap look can
be inspected and iterated outside the egui app. Color math mirrors the Rust
functions (sample_ramp, mix_rgb, resting_rgb, normalized_log_intensity) exactly.

Two renderers:
  - style="current"  -> reproduces the Codex renderer as-is
  - style="proposed" -> the refined look (cleaner ramp + calmer markers)
"""

import math
import random
import numpy as np
from PIL import Image

# ----------------------------------------------------------------------------
# Color ramps (verbatim from orderflow_render.rs)
# ----------------------------------------------------------------------------
BOOKMAP_RAMP = [
    (0.00, (3, 7, 18)),
    (0.16, (7, 28, 84)),
    (0.42, (0, 126, 213)),
    (0.67, (29, 219, 224)),
    (0.86, (255, 207, 42)),
    (1.00, (255, 247, 218)),
]

# Proposed refined Bookmap ramp: pure black floor, adds the green + orange
# phases the real Bookmap traverses, smoother perceptual steps.
BOOKMAP_RAMP_V2 = [
    (0.00, (0, 0, 0)),
    (0.09, (4, 10, 40)),
    (0.22, (10, 46, 120)),
    (0.38, (0, 120, 196)),
    (0.55, (0, 194, 196)),
    (0.70, (60, 208, 120)),
    (0.83, (208, 220, 60)),
    (0.93, (250, 158, 44)),
    (1.00, (255, 250, 232)),
]

WARM_EDGE = (255, 247, 218)
WARM_EDGE_V2 = (255, 250, 232)


def sample_ramp(stops, t):
    t = max(0.0, min(1.0, t))
    if t <= stops[0][0]:
        return stops[0][1]
    for i in range(len(stops) - 1):
        a_at, a_rgb = stops[i]
        b_at, b_rgb = stops[i + 1]
        if t <= b_at:
            span = max(b_at - a_at, 1e-6)
            return mix_rgb(a_rgb, b_rgb, (t - a_at) / span)
    return stops[-1][1]


def mix_rgb(a, b, amt):
    amt = max(0.0, min(1.0, amt))
    return tuple(a[i] + (b[i] - a[i]) * amt for i in range(3))


def resting_rgb(ramp, side, intensity):
    base = sample_ramp(ramp, intensity)
    # bid=blue tint, ask=red tint; secondary cue only (5.5% at zero intensity)
    tint = (0, 174, 231) if side == "bid" else (255, 90, 108)
    return mix_rgb(base, tint, (1.0 - clamp01(intensity)) * 0.055)


def resting_rgb_v2(ramp, side, intensity):
    # Matches the shipped Rust resting_rgb: same tints, factor 0.045 (subtler
    # than the old 0.055) so brightness stays the primary magnitude cue.
    base = sample_ramp(ramp, intensity)
    tint = (0, 174, 231) if side == "bid" else (255, 90, 108)
    return mix_rgb(base, tint, (1.0 - clamp01(intensity)) * 0.045)


def clamp01(v):
    if v != v:  # NaN
        return 0.0
    return max(0.0, min(1.0, v))


def log_intensity(quantity, reference, gamma):
    if quantity <= 0 or reference <= 0:
        return 0.0
    ratio = max(0.0, quantity / reference)
    logv = min(1.0, max(0.0, math.log(1.0 + 9.0 * ratio) / math.log(10.0)))
    return logv ** gamma


# ----------------------------------------------------------------------------
# Simple float RGB canvas with alpha compositing
# ----------------------------------------------------------------------------
class Canvas:
    def __init__(self, w, h, bg=(19, 23, 34)):
        self.w = w
        self.h = h
        self.buf = np.zeros((h, w, 3), dtype=np.float32)
        self.buf[:, :] = bg

    def blend_rect(self, x0, y0, x1, y1, color, alpha):
        if alpha <= 0:
            return
        xi0 = max(0, int(round(min(x0, x1))))
        xi1 = min(self.w, int(round(max(x0, x1))))
        yi0 = max(0, int(round(min(y0, y1))))
        yi1 = min(self.h, int(round(max(y0, y1))))
        if xi1 <= xi0 or yi1 <= yi0:
            return
        a = clamp01(alpha)
        col = np.array(color, dtype=np.float32)
        region = self.buf[yi0:yi1, xi0:xi1, :]
        region[:] = col * a + region * (1.0 - a)

    def blend_rect_hgrad(self, x0, y0, x1, y1, cleft, aleft, cright, aright):
        # Horizontal gradient in color and alpha (mirrors add_gradient_rect).
        xi0 = max(0, int(round(min(x0, x1))))
        xi1 = min(self.w, int(round(max(x0, x1))))
        yi0 = max(0, int(round(min(y0, y1))))
        yi1 = min(self.h, int(round(max(y0, y1))))
        if xi1 <= xi0 or yi1 <= yi0:
            return
        width = xi1 - xi0
        ts = np.linspace(0.0, 1.0, width, dtype=np.float32)
        cl = np.array(cleft, dtype=np.float32)
        cr = np.array(cright, dtype=np.float32)
        cols = cl[None, :] * (1 - ts[:, None]) + cr[None, :] * ts[:, None]  # (width,3)
        alphas = aleft * (1 - ts) + aright * ts  # (width,)
        region = self.buf[yi0:yi1, xi0:xi1, :]
        a = alphas[None, :, None]
        region[:] = cols[None, :, :] * a + region * (1.0 - a)

    def blend_vgrad(self, x0, y0, x1, y1, ctop, atop, cbot, abot):
        xi0 = max(0, int(round(min(x0, x1))))
        xi1 = min(self.w, int(round(max(x0, x1))))
        yi0 = max(0, int(round(min(y0, y1))))
        yi1 = min(self.h, int(round(max(y0, y1))))
        if xi1 <= xi0 or yi1 <= yi0:
            return
        height = yi1 - yi0
        ts = np.linspace(0.0, 1.0, height, dtype=np.float32)
        ct = np.array(ctop, dtype=np.float32)
        cb = np.array(cbot, dtype=np.float32)
        cols = ct[None, :] * (1 - ts[:, None]) + cb[None, :] * ts[:, None]
        alphas = atop * (1 - ts) + abot * ts
        region = self.buf[yi0:yi1, xi0:xi1, :]
        a = alphas[:, None, None]
        region[:] = cols[:, None, :] * a + region * (1.0 - a)

    def circle(self, cx, cy, r, color, alpha, fill=True, width=1.2):
        if r <= 0 or alpha <= 0:
            return
        xi0 = max(0, int(math.floor(cx - r - 2)))
        xi1 = min(self.w, int(math.ceil(cx + r + 2)))
        yi0 = max(0, int(math.floor(cy - r - 2)))
        yi1 = min(self.h, int(math.ceil(cy + r + 2)))
        if xi1 <= xi0 or yi1 <= yi0:
            return
        ys, xs = np.mgrid[yi0:yi1, xi0:xi1]
        dist = np.sqrt((xs - cx) ** 2 + (ys - cy) ** 2)
        if fill:
            mask = clamp01(r - dist + 0.5) if False else np.clip(r - dist + 0.5, 0.0, 1.0)
        else:
            mask = np.clip(width * 0.5 - np.abs(dist - r) + 0.5, 0.0, 1.0)
        a = mask * clamp01(alpha)
        col = np.array(color, dtype=np.float32)
        region = self.buf[yi0:yi1, xi0:xi1, :]
        a3 = a[:, :, None]
        region[:] = col[None, None, :] * a3 + region * (1.0 - a3)

    def line(self, x0, y0, x1, y1, color, alpha, width=1.0):
        # Thin AA line via distance field.
        xi0 = max(0, int(math.floor(min(x0, x1) - width - 1)))
        xi1 = min(self.w, int(math.ceil(max(x0, x1) + width + 1)))
        yi0 = max(0, int(math.floor(min(y0, y1) - width - 1)))
        yi1 = min(self.h, int(math.ceil(max(y0, y1) + width + 1)))
        if xi1 <= xi0 or yi1 <= yi0:
            return
        ys, xs = np.mgrid[yi0:yi1, xi0:xi1].astype(np.float32)
        dx, dy = x1 - x0, y1 - y0
        L2 = dx * dx + dy * dy
        if L2 < 1e-6:
            return
        t = np.clip(((xs - x0) * dx + (ys - y0) * dy) / L2, 0.0, 1.0)
        px = x0 + t * dx
        py = y0 + t * dy
        dist = np.sqrt((xs - px) ** 2 + (ys - py) ** 2)
        mask = np.clip(width * 0.5 - dist + 0.5, 0.0, 1.0)
        a = mask * clamp01(alpha)
        col = np.array(color, dtype=np.float32)
        region = self.buf[yi0:yi1, xi0:xi1, :]
        a3 = a[:, :, None]
        region[:] = col[None, None, :] * a3 + region * (1.0 - a3)

    def poly(self, points, color, alpha):
        xs = [p[0] for p in points]
        ys = [p[1] for p in points]
        xi0 = max(0, int(math.floor(min(xs))))
        xi1 = min(self.w, int(math.ceil(max(xs))))
        yi0 = max(0, int(math.floor(min(ys))))
        yi1 = min(self.h, int(math.ceil(max(ys))))
        if xi1 <= xi0 or yi1 <= yi0:
            return
        gy, gx = np.mgrid[yi0:yi1, xi0:xi1]
        inside = np.zeros((yi1 - yi0, xi1 - xi0), dtype=bool)
        n = len(points)
        j = n - 1
        for i in range(n):
            xi_, yi_ = points[i]
            xj_, yj_ = points[j]
            cond = ((yi_ > gy) != (yj_ > gy)) & (
                gx < (xj_ - xi_) * (gy - yi_) / (yj_ - yi_ + 1e-9) + xi_
            )
            inside ^= cond
            j = i
        a = inside.astype(np.float32) * clamp01(alpha)
        col = np.array(color, dtype=np.float32)
        region = self.buf[yi0:yi1, xi0:xi1, :]
        a3 = a[:, :, None]
        region[:] = col[None, None, :] * a3 + region * (1.0 - a3)

    def to_image(self):
        arr = np.clip(self.buf, 0, 255).astype(np.uint8)
        return Image.fromarray(arr, "RGB")


# ----------------------------------------------------------------------------
# Deterministic synthetic Bookmap scene
# ----------------------------------------------------------------------------
class Scene:
    """A trending order-flow scene built to show aggressions EATING walls.

    Price index space: p=0 is the top (high price). Ask walls sit above the
    price (smaller p) and are consumed by BUYS when price rises (p decreases);
    bid walls sit below (larger p) and are consumed by SELLS when price falls.
    Each wall is a bright horizontal band that STOPS exactly where the price
    line (carrying its bubble) reaches it — that is the wall being eaten.
    """

    def __init__(self, seed=7, T=220, P=64):
        self.T = T
        self.P = P
        rng = random.Random(seed)
        self.rng = rng

        # Scripted mid path: strong trends so it crosses many resting levels.
        keys = [(0.00, 44), (0.14, 44), (0.40, 20), (0.50, 26),
                (0.74, 9), (0.86, 17), (1.00, 12)]
        mid = []
        for t in range(T):
            f = t / (T - 1)
            for i in range(len(keys) - 1):
                f0, p0 = keys[i]
                f1, p1 = keys[i + 1]
                if f <= f1 or i == len(keys) - 2:
                    a = 0.0 if f1 == f0 else (f - f0) / (f1 - f0)
                    a = max(0.0, min(1.0, a))
                    # ease + a little noise so bubbles aren't a perfect line
                    ease = a * a * (3 - 2 * a)
                    mid.append(p0 + (p1 - p0) * ease + rng.uniform(-0.4, 0.4))
                    break
        self.mid = mid

        liq = np.zeros((T, P), dtype=np.float32)
        # thin diffuse book hugging the price (best bid/ask depth)
        for t in range(T):
            m = mid[t]
            for p in range(P):
                d = abs(p - m)
                if d < 18:
                    liq[t, p] += max(0.0, (1.3 - d * 0.06)) * (0.5 + rng.random() * 0.7)

        events = []
        trades = []

        def crossing_time(level):
            for t in range(1, T):
                if (mid[t - 1] - level) * (mid[t] - level) <= 0 and abs(mid[t] - level) < 1.5:
                    return t
            return None

        # Resting walls at fixed levels. Consumed when the price reaches them.
        wall_levels = list(range(6, P - 6, 3))
        for p in wall_levels:
            t_cross = crossing_time(p)
            mag = rng.uniform(30, 130)
            born = rng.randint(0, 8)
            # side relative to where price starts
            side = "ask" if p < mid[0] else "bid"
            end = t_cross if t_cross is not None else T - 1
            for t in range(born, end):
                # per-column fluctuation: the real book jitters every update,
                # which fragments RLE runs and (with continuous intensity)
                # makes bands look like flowing "meteors".
                f = mag * (0.93 + 0.14 * rng.random())
                liq[t, p] += f
                if p + 1 < P:
                    liq[t, p + 1] += f * 0.35  # give the band some thickness
            if t_cross is not None:
                # consume over a few columns right before the crossing
                clen = 4
                for k in range(clen):
                    tt = t_cross - clen + k
                    if 0 <= tt < T:
                        liq[tt, p] *= (1.0 - (k + 1) / clen)
                        if p + 1 < P:
                            liq[tt, p + 1] *= (1.0 - (k + 1) / clen)
                # aligned consumption event at the wall level
                events.append({
                    "t": t_cross, "p": p, "side": side,
                    "fraction": 1.0, "full": True, "evidence": "aligned",
                })
                # REALISTIC RE-STACK: new resting liquidity refills the level
                # right after it was eaten (as it does on a busy book), so the
                # band looks continuous unless a hole marks the consumption.
                if rng.random() < 0.7:
                    refill = rng.uniform(20, 90)
                    for t in range(t_cross + 1, T):
                        f = refill * (0.8 + 0.4 * rng.random())
                        liq[t, p] += f
                        if p + 1 < P:
                            liq[t, p + 1] += f * 0.35
                # AGGRESSION BUBBLES exactly at the wall level and crossing time
                aside = "buy" if side == "ask" else "sell"
                for k in range(rng.randint(2, 4)):
                    trades.append({
                        "t": t_cross + rng.randint(-1, 1),
                        "p": p + rng.uniform(-0.5, 0.5),
                        "side": aside, "size": rng.uniform(0.55, 1.0), "linked": True,
                    })
            elif rng.random() < 0.35:
                # a few unconsumed walls withdraw on their own (depth-only)
                events.append({
                    "t": end, "p": p, "side": side,
                    "fraction": min(1.0, 0.5 + rng.random() * 0.5),
                    "full": rng.random() < 0.5, "evidence": "depth",
                })

        # Continuous best-price prints riding the price line (the "tape").
        for t in range(0, T):
            if rng.random() < 0.6:
                # direction follows the local trend
                rising = t + 1 < T and mid[min(t + 1, T - 1)] < mid[t]
                aside = "buy" if rising else "sell"
                trades.append({
                    "t": t, "p": mid[t] + rng.uniform(-0.8, 0.8),
                    "side": aside, "size": rng.uniform(0.10, 0.40), "linked": False,
                })

        self.liq = liq
        self.events = events
        self.trades = trades
        pos = liq[liq > 0]
        pos.sort()
        self.reference = float(pos[int(0.99 * len(pos)) - 1]) if len(pos) else 1.0
        self.gap_end_t = 8


def render_scene(scene, style, theme_opacity=0.72, gamma=0.75,
                 group=2, w=1000, h=440, margin=(58, 18, 18, 30)):
    """margin = (left, top, right, bottom)"""
    proposed = (style == "proposed")
    ramp = BOOKMAP_RAMP_V2 if proposed else BOOKMAP_RAMP
    warm = WARM_EDGE_V2 if proposed else WARM_EDGE
    rest_fn = resting_rgb_v2 if proposed else resting_rgb
    bg = (17, 21, 31) if proposed else (19, 23, 34)

    ml, mt, mr, mb = margin
    cv = Canvas(w, h, bg=bg)
    plot_l, plot_t = ml, mt
    plot_w, plot_h = w - ml - mr, h - mt - mb
    T, P = scene.T, scene.P

    def X(t):
        return plot_l + (t / (T - 1)) * plot_w

    def Y(p):
        return plot_t + (p / P) * plot_h

    col_w = plot_w / (T - 1)

    # --- resting liquidity heatmap (aggregated into `group` buckets) ---
    edge_glow = 0.14 if proposed else 0.18
    Pg = P // group
    LEVELS = 9  # quantization steps -> crisp, stable bands
    for t in range(T):
        m = scene.mid[t]
        x0 = X(t) - col_w * 0.5
        x1 = X(t) + col_w * 0.5
        for pg in range(Pg):
            p_lo = pg * group
            q = float(scene.liq[t, p_lo:p_lo + group].sum())
            if q <= 0.2:
                continue
            inten = log_intensity(q, scene.reference * group * 0.55, gamma)
            if inten <= 0.02:
                continue
            side = "ask" if (p_lo + group * 0.5) < m else "bid"
            y0 = Y(p_lo)
            y1 = Y(p_lo + group)
            if proposed:
                # Quantize intensity so small book jitter maps to the SAME
                # color; adjacent runs merge into one crisp, stable band, and a
                # solid fill (no horizontal gradient) removes the "meteor" head.
                qi = max(1.0 / LEVELS, round(inten * LEVELS) / LEVELS)
                rgb = rest_fn(ramp, side, qi)
                alpha = qi * theme_opacity
                if edge_glow > 0:
                    cv.blend_rect(x0, y0 - 1.0, x1, y1 + 1.0, rgb, alpha * edge_glow)
                cv.blend_rect(x0, y0, x1, y1, rgb, alpha)
            else:
                rgb = rest_fn(ramp, side, inten)
                alpha = inten * theme_opacity
                if edge_glow > 0:
                    cv.blend_rect(x0, y0 - 1.0, x1, y1 + 1.0, rgb, alpha * edge_glow)
                trailing = mix_rgb(rgb, warm, 0.05 + inten * 0.06)
                cv.blend_rect_hgrad(x0, y0, x1, y1, rgb, alpha * 0.90, trailing, alpha)

    # --- carve a gap around each consumption bubble (proposed) so re-stacked
    #     liquidity does not slide through it; the fresh wall starts after it ---
    if proposed:
        for tr in scene.trades:
            if not tr["linked"]:
                continue
            bx, by = X(tr["t"]), Y(tr["p"])
            size = tr["size"]
            r = math.sqrt(2.75 ** 2 + (size ** 2) * (13.0 ** 2 - 2.75 ** 2))
            cv.blend_rect(bx - r - 1, by - r - 2, bx + r + 4, by + r + 2, bg, 0.97)

    # --- start gap (book unavailable before capture) ---
    gx0, gx1 = X(0) - col_w * 0.5, X(scene.gap_end_t)
    cv.blend_rect(gx0, plot_t, gx1, plot_t + plot_h, (50, 58, 76), 0.20 if not proposed else 0.16)
    # hatch
    step = 16
    x = gx0 - plot_h
    while x < gx1:
        cv.line(max(gx0, x), plot_t + plot_h, min(gx1, x + plot_h), plot_t,
                (150, 160, 180), 0.10, width=0.8)
        x += step
    for gx in (gx0, gx1):
        yy = plot_t
        while yy < plot_t + plot_h:
            cv.line(gx, yy, gx, min(plot_t + plot_h, yy + 4), (157, 167, 188), 0.45, width=1.0)
            yy += 9

    # --- liquidity consumption / withdrawal markers ---
    for ev in scene.events:
        x = X(ev["t"])
        p_center = (ev["p"] // group) * group + group * 0.5
        yc = Y(p_center)
        band_h = (plot_h / Pg)
        frac = ev["fraction"]
        full = ev["full"]
        if ev["evidence"] == "aligned":
            if proposed:
                render_bite_proposed(cv, x, yc, band_h, frac, full, bg)
            else:
                render_bite_current(cv, x, yc, band_h, frac, full, bg)
        else:  # depth-only withdrawal
            if proposed:
                render_withdrawal_proposed(cv, x, yc, band_h, frac, full)
            else:
                render_withdrawal_current(cv, x, yc, band_h, frac, full)

    # --- price path (subtle) ---
    pts = [(X(t), Y(scene.mid[t])) for t in range(T)]
    for i in range(len(pts) - 1):
        cv.line(pts[i][0], pts[i][1], pts[i + 1][0], pts[i + 1][1],
                (235, 238, 245), 0.42 if not proposed else 0.5, width=1.1)

    # --- aggression bubbles ---
    for tr in scene.trades:
        x = X(tr["t"])
        y = Y(tr["p"])
        size = tr["size"]
        r = math.sqrt(2.75 ** 2 + (size ** 2) * (13.0 ** 2 - 2.75 ** 2))
        buy = tr["side"] == "buy"
        if proposed:
            color = (46, 224, 150) if buy else (255, 82, 96)
        else:
            color = (48, 229, 166) if buy else (255, 75, 99)
        if proposed:
            render_bubble_proposed(cv, x, y, r, color, tr["linked"],
                                   band_h=plot_h / Pg, buy=buy)
        else:
            render_bubble_current(cv, x, y, r, color, tr["linked"])

    # --- frame ---
    cv.line(plot_l, plot_t, plot_l, plot_t + plot_h, (90, 100, 120), 0.5, width=1.0)
    cv.line(plot_l, plot_t + plot_h, plot_l + plot_w, plot_t + plot_h, (90, 100, 120), 0.5, width=1.0)

    return cv


# --- current markers (Codex) ---
def render_bite_current(cv, x, yc, band_h, frac, full, bg):
    depth = 17.0 if full else 4.0 + 10.0 * frac
    hh = band_h * 0.5 if full else band_h * (0.30 + 0.70 * frac) * 0.5
    cv.poly([(x - depth, yc - hh), (x + 1, yc), (x - depth, yc + hh)],
            bg, 0.94 if not full else 1.0)
    half = band_h * (0.25 + 0.35 * frac)
    cons = (255, 244, 190)
    cv.line(x, yc - half, x, yc + half, cons, 0.55 + 0.4 * frac, width=2.0 if full else 1.15)
    radius = min(8.0, max(3.0, band_h * 0.62))
    cv.circle(x, yc, radius, cons, 0.48 + 0.42 * frac, fill=False,
              width=1.8 if full else 1.15)
    if full:
        cv.circle(x, yc, radius + 2.5, cons, 0.42, fill=False, width=0.8)


def render_withdrawal_current(cv, x, yc, band_h, frac, full):
    violet = (194, 112, 255)
    tail = 22.0 if full else 9.0 + 10.0 * frac
    hh = band_h * 0.5 if full else band_h * (0.30 + 0.70 * frac) * 0.5
    cv.blend_rect_hgrad(x, yc - hh, x + tail, yc + hh,
                        violet, (0.46 if full else 0.18 + 0.18 * frac), violet, 0.0)
    yy = yc - hh
    while yy < yc + hh:
        cv.line(x, yy, x, min(yc + hh, yy + (3 if full else 2)), violet,
                1.0 if full else 0.62 + frac * 0.28, width=2.0 if full else 1.15)
        yy += (3 if full else 2) + 2.5
    if full:
        d = 3.25
        cv.line(x + 3.5, yc - d, x + 3.5 + d, yc, violet, 1.0, width=1.3)
        cv.line(x + 3.5 + d, yc, x + 3.5, yc + d, violet, 1.0, width=1.3)
        cv.line(x + 3.5, yc + d, x + 3.5 - d, yc, violet, 1.0, width=1.3)
        cv.line(x + 3.5 - d, yc, x + 3.5, yc - d, violet, 1.0, width=1.3)


def render_bubble_current(cv, x, y, r, color, linked):
    cv.circle(x, y, r + 3.5, color, 0.10 + 0.08, fill=True)
    cv.circle(x, y, r, color, 0.78, fill=True)
    cv.circle(x, y, r, color, 0.98, fill=False, width=1.1)
    cv.circle(x, y, max(1.0, r - 1.5), (244, 255, 252), 0.45, fill=False, width=0.7)
    if linked:
        cons = (255, 244, 190)
        cv.circle(x, y, r + 2.0, cons, 0.7, fill=False, width=1.4)
        for dx, dy in [(0.72, -0.72), (-0.72, 0.72)]:
            cv.line(x + dx * (r + 2.1), y + dy * (r + 2.1),
                    x + dx * (r + 5.0), y + dy * (r + 5.0), cons, 0.8, width=1.15)


# --- proposed markers (cleaner) ---
def render_bite_proposed(cv, x, yc, band_h, frac, full, bg):
    # A HOLE marks where the wall was eaten: carve a dark gap spanning the band
    # right after the consumption instant, so a re-stacked wall reads as a NEW
    # band starting after the gap instead of a seamless continuation. A bright
    # front sits on the left edge of the hole (the eaten wall's end).
    cons = (255, 246, 205)
    hh = band_h * 0.5                       # hole spans the full consumed band
    hole_w = 9.0 if full else (5.0 + 6.0 * frac)
    cv.blend_rect(x + 0.5, yc - hh, x + 0.5 + hole_w, yc + hh, bg, 0.96)
    fh = hh if full else band_h * (0.32 + 0.68 * frac) * 0.5
    cv.line(x + 0.5, yc - fh, x + 0.5, yc + fh, cons, 0.9 if full else 0.55 + 0.35 * frac,
            width=1.9 if full else 1.3)
    if full:
        cv.line(x - 3.0, yc - hh, x + 3.5, yc - hh, cons, 0.8, width=1.4)
        cv.line(x - 3.0, yc + hh, x + 3.5, yc + hh, cons, 0.8, width=1.4)


def render_withdrawal_proposed(cv, x, yc, band_h, frac, full):
    # A calm violet fade to the right = liquidity left without a matching trade.
    # No dashes, no diamond; the fading ghost + a thin cap reads cleanly.
    violet = (176, 130, 240)
    tail = (band_h * 0.0 + 20.0) if full else (7.0 + 9.0 * frac)
    hh = band_h * 0.5 if full else band_h * (0.34 + 0.66 * frac) * 0.5
    cv.blend_rect_hgrad(x, yc - hh, x + tail, yc + hh,
                        violet, (0.40 if full else 0.16 + 0.16 * frac), violet, 0.0)
    cv.line(x, yc - hh, x, yc + hh, violet, 0.85 if full else 0.5 + 0.3 * frac,
            width=1.6 if full else 1.1)


def render_bubble_proposed(cv, x, y, r, color, linked, band_h=0.0, buy=True):
    # Cleaner: soft halo + fill + single crisp rim.
    cv.circle(x, y, r + 2.5, color, 0.12, fill=True)
    cv.circle(x, y, r, color, 0.82, fill=True)
    cv.circle(x, y, r, color, 0.95, fill=False, width=1.0)
    if linked:
        cons = (255, 246, 205)
        # The print ate resting liquidity here: a consumption front ON the
        # bubble (vertical) with a glow leaking into the consumed side, so
        # bubble <-> wall read as one event even when price is going sideways.
        hh = r * 1.7 + 2.0
        cv.blend_rect_hgrad(x, y - hh, x + 7, y + hh, cons, 0.24, cons, 0.0)
        cv.line(x, y - hh, x, y + hh, cons, 0.9, width=1.8)
        cv.circle(x, y, r + 1.6, cons, 0.9, fill=False, width=1.3)


# ----------------------------------------------------------------------------
# Ramp strip comparison
# ----------------------------------------------------------------------------
def render_ramps(w=1000, h=150):
    cv = Canvas(w, h, bg=(17, 21, 31))
    labels = [
        ("Bookmap ramp — atual (Codex)", BOOKMAP_RAMP, (19, 23, 34)),
        ("Bookmap ramp — proposta (refinada)", BOOKMAP_RAMP_V2, (17, 21, 31)),
    ]
    pad = 12
    strip_h = (h - pad * (len(labels) + 1)) / len(labels)
    for i, (name, ramp, _bg) in enumerate(labels):
        y0 = pad + i * (strip_h + pad)
        for x in range(pad, w - pad):
            t = (x - pad) / (w - 2 * pad)
            rgb = sample_ramp(ramp, t)
            cv.buf[int(y0):int(y0 + strip_h), x, :] = np.array(rgb, dtype=np.float32)
    return cv


def render_detail(style, w=760, h=360):
    """A small, clear scene: a persistent wall being eaten by aggression, a
    depth-only withdrawal, and a quiet wall for reference. Zoomed for markers."""
    proposed = (style == "proposed")
    ramp = BOOKMAP_RAMP_V2 if proposed else BOOKMAP_RAMP
    warm = WARM_EDGE_V2 if proposed else WARM_EDGE
    rest_fn = resting_rgb_v2 if proposed else resting_rgb
    bg = (17, 21, 31) if proposed else (19, 23, 34)
    cv = Canvas(w, h, bg=bg)
    ml, mt, mr, mb = 20, 20, 20, 20
    pw, ph = w - ml - mr, h - mt - mb
    ref = 100.0
    gamma = 0.75
    opacity = 0.72

    def band(y_frac, thick_frac, segs, side):
        """segs: list of (x0,x1,qty). draws a horizontal wall with segments."""
        y0 = mt + y_frac * ph
        y1 = y0 + thick_frac * ph
        for (a, b, q) in segs:
            if q <= 0:
                continue
            inten = log_intensity(q, ref, gamma)
            rgb = rest_fn(ramp, side, inten)
            alpha = inten * opacity
            xa = ml + a * pw
            xb = ml + b * pw
            eg = 0.14 if proposed else 0.18
            cv.blend_rect(xa, y0 - 1, xb, y1 + 1, rgb, alpha * eg)
            trailing = mix_rgb(rgb, warm, 0.05 + inten * 0.05)
            cv.blend_rect_hgrad(xa, y0, xb, y1, rgb, alpha * 0.9, trailing, alpha)
        return y0, y1

    # 1) A strong ask wall being consumed at x=0.55 (qty drops 90->10)
    y0, y1 = band(0.16, 0.075, [(0.10, 0.55, 90.0), (0.55, 0.66, 12.0)], "ask")
    yc = (y0 + y1) / 2
    band_h = (y1 - y0)
    x_ev = ml + 0.55 * pw
    if proposed:
        render_bite_proposed(cv, x_ev, yc, band_h, 0.86, False, bg)
    else:
        render_bite_current(cv, x_ev, yc, band_h, 0.86, False, bg)
    # aggression bubbles pushing into it (buy eats ask)
    for k in range(3):
        r = 11 - k * 1.5
        color = (46, 224, 150) if proposed else (48, 229, 166)
        if proposed:
            render_bubble_proposed(cv, x_ev - k * 15, yc, r, color, k == 0)
        else:
            render_bubble_current(cv, x_ev - k * 15, yc, r, color, k == 0)

    # 2) A quiet mid wall (reference, persists)
    band(0.45, 0.06, [(0.10, 0.90, 42.0)], "ask")

    # 3) A bid wall withdrawn without aggression (depth-only) at x=0.62
    y0b, y1b = band(0.72, 0.075, [(0.10, 0.62, 70.0)], "bid")
    ycb = (y0b + y1b) / 2
    bhb = (y1b - y0b)
    x_wd = ml + 0.62 * pw
    if proposed:
        render_withdrawal_proposed(cv, x_wd, ycb, bhb, 0.9, True)
    else:
        render_withdrawal_current(cv, x_wd, ycb, bhb, 0.9, True)

    return cv


if __name__ == "__main__":
    import sys
    out = sys.argv[1] if len(sys.argv) > 1 else "."
    scene = Scene(seed=7)
    for style in ("current", "proposed"):
        render_scene(scene, style, group=2).to_image().save(f"{out}/scene_{style}.png")
        render_detail(style).to_image().save(f"{out}/detail_{style}.png")
    render_ramps().to_image().save(f"{out}/ramps.png")
    print("saved scene_/detail_ current+proposed, ramps.png")
