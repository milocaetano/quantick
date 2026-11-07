use super::*;

#[test]
fn the_q_has_icon_svgs_twenty_one_bars_and_no_plate() {
    let bars = bars();
    assert_eq!(bars.iter().filter(|bar| bar.teal).count(), 13);
    assert_eq!(bars.iter().filter(|bar| !bar.teal).count(), 8);
    assert!(
        bars.iter().all(|bar| bar.w < 100.0),
        "the plate is not a bar"
    );
    assert_eq!(teal_stops(), ([0x3A, 0xF0, 0xB2], [0x12, 0xC6, 0x8C]));
}

#[test]
fn the_square_is_w6s_42_px_at_the_traders_scale() {
    assert_eq!(side_px(1.5), 42);
    assert_eq!(side_px(1.0), 28);
    assert_eq!(side_px(2.0), 56);
}

/// At W6's own size every faded bar keeps a white core between black edges,
/// and every teal bar a teal core, as in the approved crop.
#[test]
fn at_w6s_size_each_bar_has_a_black_edge_and_a_lit_core() {
    let side = 42;
    let image: Vec<[u8; 4]> = light_q(side).as_chunks::<4>().0.to_vec();
    let unit = side as f32 / VIEWBOX;
    // Per bar: the darkest pixel's brightest channel, and the lightest pixel.
    let column = |x: f32, rows: &[(f32, f32)]| -> Vec<(u8, [u8; 4])> {
        let px = (x * unit) as usize;
        rows.iter()
            .map(|&(y, h)| {
                let (y0, y1) = ((y * unit) as usize, ((y + h) * unit).ceil() as usize);
                let span: Vec<[u8; 4]> =
                    (y0..y1).map(|py| image[py * side as usize + px]).collect();
                let darkest = span.iter().map(|p| p[..3].iter().copied().max().unwrap());
                let lightest = span.iter().max_by_key(|p| p[1]).unwrap();
                (darkest.min().unwrap(), *lightest)
            })
            .collect()
    };
    // The faded rects that span x 78, and the teal ones that span x 181.
    let faded = [
        (60.0, 14.0),
        (79.0, 14.0),
        (98.0, 14.0),
        (116.0, 14.0),
        (135.0, 14.0),
    ];
    for (darkest, [r, g, b, _]) in column(78.0, &faded) {
        assert!(
            r.min(g).min(b) > 140 && r == g && g == b,
            "a white core: {r} {g} {b}"
        );
        assert!(darkest < 90, "a black edge: {darkest}");
    }
    let teal = [(79.0, 14.0), (98.0, 14.0), (117.0, 13.0), (136.0, 13.0)];
    for (darkest, [r, g, b, _]) in column(181.0, &teal) {
        assert!(g > 120 && g > r + 60 && g > b, "a teal core: {r} {g} {b}");
        assert!(darkest < 90, "a black edge: {darkest}");
    }
}

#[test]
fn pixels_are_premultiplied_and_the_corners_clear() {
    for side in [28, 35, 42, 56] {
        let image: Vec<[u8; 4]> = light_q(side).as_chunks::<4>().0.to_vec();
        assert_eq!(image.len(), (side * side) as usize);
        assert!(image.iter().all(|[r, g, b, a]| r <= a && g <= a && b <= a));
        assert_eq!(image[0], [0; 4], "no plate at {side}");
    }
}

#[test]
fn the_raster_is_deterministic() {
    assert_eq!(light_q(42), light_q(42));
}

/// The square sits on the pixel grid, one texel per pixel, W6's offset away
/// from the box's height centre and from Dark's glyph right edge.
#[test]
fn the_box_keeps_the_dark_glyphs_right_edge_on_the_pixel_grid() {
    for ppp in [1.0, 1.25, 1.5, 1.75, 2.0] {
        let [left, top, side] = light_q_box([30.0, 17.0], 22.0, ppp);
        for v in [left, top] {
            assert!(((v * ppp) - (v * ppp).round()).abs() < 1e-3, "{v} at {ppp}");
        }
        assert!((side * ppp - side_px(ppp) as f32).abs() < 1e-3);
        let dark_right = 30.0 + (GLYPH_RIGHT - GLYPH_CENTRE.0) * 22.0 * 4.0 / 3.0 / VIEWBOX;
        let right = left + GLYPH_RIGHT * side / VIEWBOX + W6_OFFSET_PT;
        assert!(
            (right - dark_right).abs() <= 0.5 / ppp + 1e-4,
            "{right} vs {dark_right}"
        );
        let middle = top + side / 2.0 + W6_OFFSET_PT;
        assert!(
            (middle - 17.0).abs() <= 0.5 / ppp + 1e-4,
            "{middle} at {ppp}"
        );
    }
}
