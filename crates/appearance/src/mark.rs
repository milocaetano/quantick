//! The Light theme's plate-less Q, rasterised at the exact physical size it is
//! drawn at.
//!
//! A downscaled 256-texel raster loses the black edges at tab-strip size, so
//! the mark is rendered per size the way the approved W6 mock was
//! (`docs/thema/white/newlayout/logo_compose.py`): `logo/icon.svg`'s rects on
//! a transparent square spanning the whole viewBox, opaque white faces on the
//! faded side, one teal per rect taken from the gradient at the rect's centre,
//! a black edge inside every rect, drawn 4x supersampled and Lanczos-filtered
//! down.

const SVG: &str = include_str!("../../../logo/icon.svg");

/// The square the viewBox fills, in points: W6 drew it 42 px wide at the
/// trader's 1.5 px/pt.
pub const MARK_PT: f32 = 28.0;
/// The Q glyph's centre in viewBox units, where the Dark frame centres it.
pub const GLYPH_CENTRE: (f32, f32) = (129.5, 133.0);
/// The glyph's right edge in viewBox units, the side that faces the wordmark.
pub const GLYPH_RIGHT: f32 = 206.0;
/// How far up and left the square sits from the Dark-anchored place: a test
/// capture at 1.5 px/pt showed the Q 2 px right of and 2 px below c06, with the
/// wordmark aligned.
pub const W6_OFFSET_PT: f32 = 2.0 / 1.5;
pub const VIEWBOX: f32 = 256.0;

/// Samples per pixel along each axis, as the mock's 4x supersampling.
const SUPERSAMPLE: usize = 4;
/// Lanczos lobes, as Pillow's `LANCZOS`.
const LOBES: f32 = 3.0;
/// The black edge's width in viewBox units, rounded to whole samples (W6).
const EDGE_UNITS: f32 = 4.0;
const FACE: [u8; 3] = [0xFF, 0xFF, 0xFF];
const EDGE: [u8; 3] = [0x00, 0x00, 0x00];

/// One of icon.svg's Q rects, in viewBox units.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Bar {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    radius: f32,
    teal: bool,
}

/// The Light Q's square side in physical pixels at `pixels_per_point`.
#[must_use]
pub fn side_px(pixels_per_point: f32) -> u32 {
    (MARK_PT * pixels_per_point).round().max(1.0) as u32
}

/// Where the Light Q's square goes, in points: `[left, top, side]`. `frame`
/// is the Dark Q's box (centre, width), whose frame scales the viewBox by 4/3
/// about the glyph's centre. The square is centred on the box's height, the
/// glyph's right edge (the side facing the wordmark) on the Dark glyph's, and
/// both then move [`W6_OFFSET_PT`] up and left, onto c06. On the pixel grid.
#[must_use]
pub fn light_q_box(centre: [f32; 2], frame_width: f32, pixels_per_point: f32) -> [f32; 3] {
    let ppp = pixels_per_point;
    let side = side_px(ppp) as f32 / ppp;
    let unit = side / VIEWBOX;
    let dark_unit = frame_width * 4.0 / 3.0 / VIEWBOX;
    let right = centre[0] + (GLYPH_RIGHT - GLYPH_CENTRE.0) * dark_unit;
    let on_grid = |v: f32| (v * ppp).round() / ppp;
    let left = on_grid(right - GLYPH_RIGHT * unit - W6_OFFSET_PT);
    [left, on_grid(centre[1] - side / 2.0 - W6_OFFSET_PT), side]
}

/// The Light Q as premultiplied sRGB RGBA bytes, `side` x `side` pixels,
/// row-major, the viewBox spanning the square.
#[must_use]
pub fn light_q(side: u32) -> Vec<u8> {
    let side = side.max(1) as usize;
    let fine = supersampled(side);
    let weights = lanczos_weights(side);
    let n = side * SUPERSAMPLE;
    // Separable: rows first, then columns, on premultiplied channels.
    let mut rows = vec![[0f32; 4]; side * n];
    for y in 0..n {
        for (x, taps) in weights.iter().enumerate() {
            rows[y * side + x] = taps.iter().fold([0f32; 4], |acc, &(i, w)| {
                let texel = fine[y * n + i];
                std::array::from_fn(|c| acc[c] + texel[c] * w)
            });
        }
    }
    let mut out = Vec::with_capacity(side * side * 4);
    for taps in &weights {
        for x in 0..side {
            let sum = taps.iter().fold([0f32; 4], |acc, &(i, w)| {
                let texel = rows[i * side + x];
                std::array::from_fn(|c| acc[c] + texel[c] * w)
            });
            let alpha = sum[3].round().clamp(0.0, 255.0);
            out.extend((0..4).map(|c| {
                let limit = if c == 3 { 255.0 } else { alpha };
                sum[c].round().clamp(0.0, limit) as u8
            }));
        }
    }
    out
}

/// The 4x image: each sample takes the colour of the bar it falls in, edge or
/// face, premultiplied, transparent elsewhere.
fn supersampled(side: usize) -> Vec<[f32; 4]> {
    let n = side * SUPERSAMPLE;
    let unit = n as f32 / VIEWBOX;
    // Pillow's rounding: ties to even, and a rect's far bound is inclusive.
    let snap = |v: f32| v.round_ties_even();
    let edge = snap(EDGE_UNITS * unit).max(1.0);
    let (top, bottom) = teal_stops();
    let mut fine = vec![[0f32; 4]; n * n];
    for bar in bars() {
        let fill = if bar.teal {
            lerp(top, bottom, (bar.y + bar.h / 2.0) / VIEWBOX)
        } else {
            FACE
        };
        let outer = [
            snap(bar.x * unit),
            snap(bar.y * unit),
            snap((bar.x + bar.w) * unit) + 1.0,
            snap((bar.y + bar.h) * unit) + 1.0,
        ];
        let inner = [
            outer[0] + edge,
            outer[1] + edge,
            outer[2] - edge,
            outer[3] - edge,
        ];
        let radius = bar.radius * unit;
        let (x0, x1) = (outer[0].floor() as usize, (outer[2].ceil() as usize).min(n));
        let (y0, y1) = (outer[1].floor() as usize, (outer[3].ceil() as usize).min(n));
        for sy in y0..y1 {
            for sx in x0..x1 {
                let (x, y) = (sx as f32 + 0.5, sy as f32 + 0.5);
                if !inside(x, y, outer, radius) {
                    continue;
                }
                let [r, g, b] = if inside(x, y, inner, (radius - edge).max(0.0)) {
                    fill
                } else {
                    EDGE
                };
                fine[sy * n + sx] = [f32::from(r), f32::from(g), f32::from(b), 255.0];
            }
        }
    }
    fine
}

/// Per output pixel, the 4x samples it reads and their normalised weights.
fn lanczos_weights(side: usize) -> Vec<Vec<(usize, f32)>> {
    let n = side * SUPERSAMPLE;
    let scale = SUPERSAMPLE as f32;
    let support = LOBES * scale;
    (0..side)
        .map(|o| {
            let centre = (o as f32 + 0.5) * scale;
            let first = (centre - support).floor().max(0.0) as usize;
            let last = ((centre + support).ceil() as usize).min(n);
            let taps: Vec<(usize, f32)> = (first..last)
                .map(|i| (i, lanczos((i as f32 + 0.5 - centre) / scale)))
                .filter(|&(_, w)| w != 0.0)
                .collect();
            let total: f32 = taps.iter().map(|&(_, w)| w).sum();
            taps.into_iter().map(|(i, w)| (i, w / total)).collect()
        })
        .collect()
}

fn lanczos(x: f32) -> f32 {
    let sinc = |v: f32| {
        if v.abs() < 1e-6 {
            1.0
        } else {
            let p = std::f32::consts::PI * v;
            p.sin() / p
        }
    };
    if x.abs() >= LOBES {
        0.0
    } else {
        sinc(x) * sinc(x / LOBES)
    }
}

/// Whether a point lies in the rounded rect `[x0, y0, x1, y1]`.
fn inside(x: f32, y: f32, [x0, y0, x1, y1]: [f32; 4], radius: f32) -> bool {
    if x < x0 || x >= x1 || y < y0 || y >= y1 {
        return false;
    }
    let radius = radius.min((x1 - x0) / 2.0).min((y1 - y0) / 2.0);
    let dx = (x0 + radius - x).max(x - (x1 - radius)).max(0.0);
    let dy = (y0 + radius - y).max(y - (y1 - radius)).max(0.0);
    dx * dx + dy * dy <= radius * radius
}

fn lerp(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
    let t = t.clamp(0.0, 1.0);
    std::array::from_fn(|i| {
        (f32::from(a[i]) + (f32::from(b[i]) - f32::from(a[i])) * t).round() as u8
    })
}

/// icon.svg's teal gradient (`tg`), top and bottom stops.
fn teal_stops() -> ([u8; 3], [u8; 3]) {
    let start = SVG
        .find("id=\"tg\"")
        .expect("icon.svg defines the teal gradient");
    let end = start
        + SVG[start..]
            .find("</linearGradient>")
            .expect("gradient closes");
    let stops: Vec<[u8; 3]> = SVG[start..end]
        .split("stop-color=\"")
        .skip(1)
        .map(|rest| hex(&rest[..7]))
        .collect();
    (stops[0], stops[stops.len() - 1])
}

/// The Q's rects: teal ones fill with the gradient, the rest are the faded
/// side; the plate's gradient fill is neither.
fn bars() -> Vec<Bar> {
    SVG.split("<rect ")
        .skip(1)
        .filter_map(|tag| {
            let tag = format!(" {}", &tag[..tag.find("/>")?]);
            let attr = |name: &str| {
                let key = format!(" {name}=\"");
                let value = &tag[tag.find(&key)? + key.len()..];
                Some(value[..value.find('"')?].to_owned())
            };
            let num = |name: &str| attr(name)?.parse::<f32>().ok();
            let fill = attr("fill")?;
            let teal = fill == "url(#tg)";
            (teal || fill.starts_with('#')).then_some(Bar {
                x: num("x")?,
                y: num("y")?,
                w: num("width")?,
                h: num("height")?,
                radius: num("rx").unwrap_or(0.0),
                teal,
            })
        })
        .collect()
}

fn hex(text: &str) -> [u8; 3] {
    std::array::from_fn(|i| {
        u8::from_str_radix(&text[1 + 2 * i..3 + 2 * i], 16).expect("hex colour")
    })
}

#[cfg(test)]
#[path = "mark_tests.rs"]
mod mark_tests;
