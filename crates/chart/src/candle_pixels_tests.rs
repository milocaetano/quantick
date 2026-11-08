use super::*;

/// `(pitch, body, border, wick, solid)` at one scale; `border` is checked
/// only for a body with a fill.
type Row = (f32, i32, i32, i32, bool);

/// The zoom spec's matrix (`zoom-spec.md` section 5), with the fixed band
/// starting at 2.5 as Lightweight Charts has it: pitches 2.5 and 2.6 are
/// inside it.
const AT_1_0: [Row; 14] = [
    (1.0, 1, 1, 1, true),
    (1.5, 1, 1, 1, true),
    (2.0, 1, 1, 1, true),
    (2.5, 3, 1, 1, false),
    (2.6, 3, 1, 1, false),
    (2.75, 3, 1, 1, false),
    (3.0, 3, 1, 1, false),
    (4.0, 3, 1, 1, false),
    (4.5, 3, 1, 1, false),
    (6.0, 5, 1, 1, false),
    (8.0, 5, 1, 1, false),
    (10.0, 7, 1, 1, false),
    (20.0, 15, 1, 1, false),
    (50.0, 39, 1, 1, false),
];
/// At 1.5 px/pt a 1 px line rounds to two device px, the thickness the
/// trader's TradingView (scale 2) shows on the same screen, and the layout
/// counts in those lines: each row is TradingView's scale-2 row for the same
/// pitch on screen (pitch x 1.5 / 2 logical px). A body no wider than two
/// contours is a solid bar.
const AT_1_5: [Row; 14] = [
    (1.0, 2, 2, 2, true),
    (1.5, 2, 2, 2, true),
    (2.0, 2, 2, 2, true),
    (2.5, 2, 2, 2, true),
    (2.6, 2, 2, 2, true),
    (2.75, 4, 2, 2, true),
    (3.0, 4, 2, 2, true),
    (4.0, 6, 2, 2, false),
    (4.5, 6, 2, 2, false),
    (6.0, 8, 2, 2, false),
    (8.0, 10, 2, 2, false),
    (10.0, 12, 2, 2, false),
    (20.0, 24, 2, 2, false),
    (50.0, 60, 2, 2, false),
];
/// The thin option at 1.5 px/pt: a 0.5 px line is one device px, and the
/// rows are TradingView's scale-1 rows for the same pitch on screen.
const THIN_AT_1_5: [Row; 14] = [
    (1.0, 1, 1, 1, true),
    (1.5, 1, 1, 1, true),
    (2.0, 3, 1, 1, false),
    (2.5, 3, 1, 1, false),
    (2.6, 3, 1, 1, false),
    (2.75, 3, 1, 1, false),
    (3.0, 3, 1, 1, false),
    (4.0, 5, 1, 1, false),
    (4.5, 5, 1, 1, false),
    (6.0, 7, 1, 1, false),
    (8.0, 9, 1, 1, false),
    (10.0, 11, 1, 1, false),
    (20.0, 23, 1, 1, false),
    (50.0, 59, 1, 1, false),
];

const AT_2_0: [Row; 14] = [
    (1.0, 2, 2, 2, true),
    (1.5, 2, 2, 2, true),
    (2.0, 4, 2, 2, true),
    (2.5, 6, 2, 2, false),
    (2.6, 6, 2, 2, false),
    (2.75, 6, 2, 2, false),
    (3.0, 6, 2, 2, false),
    (4.0, 6, 2, 2, false),
    (4.5, 8, 2, 2, false),
    (6.0, 10, 2, 2, false),
    (8.0, 12, 2, 2, false),
    (10.0, 16, 2, 2, false),
    (20.0, 32, 2, 2, false),
    (50.0, 80, 2, 2, false),
];

#[test]
fn at_one_and_a_half_the_default_draws_tradingview_s_scale_two_candles() {
    // The same pitch on screen, in device px: what TradingView lays out at
    // scale 2 is what the default lays out at 1.5. Off the band edges by a
    // hair, where the two divisions may round to either side.
    for step in 1..400 {
        let device = step as f32 * 0.25 + 0.01;
        let ours = candle_widths(device / 1.5, 1.5, 1.0, 1.0);
        let tradingview = candle_widths(device / 2.0, 2.0, 1.0, 1.0);
        assert_eq!(ours, tradingview, "pitch {device} device px");
    }
}

fn matrix() -> impl Iterator<Item = (f32, Row)> {
    let at = |dpr: f32, rows: [Row; 14]| rows.into_iter().map(move |row| (dpr, row));
    at(1.0, AT_1_0)
        .chain(at(1.5, AT_1_5))
        .chain(at(2.0, AT_2_0))
}

#[test]
fn widths_follow_the_tradingview_matrix_at_every_scale() {
    for (dpr, (pitch, body, border, wick, solid)) in matrix() {
        let got = candle_widths(pitch, dpr, 1.0, 1.0);
        let cell = format!("pitch {pitch} at {dpr} px/pt: {got:?}");
        assert_eq!(got.body, body, "{cell}");
        assert_eq!(got.wick, wick, "{cell}");
        assert_eq!(got.solid, solid, "{cell}");
        if !solid {
            assert_eq!(got.border, border, "{cell}");
        }
    }
    for (pitch, body, border, wick, solid) in THIN_AT_1_5 {
        let got = candle_widths(pitch, 1.5, 0.5, 0.5);
        let cell = format!("thin, pitch {pitch} at 1.5 px/pt: {got:?}");
        assert_eq!(
            (got.body, got.wick, got.solid),
            (body, wick, solid),
            "{cell}"
        );
        if !solid {
            assert_eq!(got.border, border, "{cell}");
        }
    }
}

#[test]
fn a_body_no_wider_than_two_contours_is_solid_at_every_line_width() {
    // The rule that keeps a thick contour clean when zoomed out: never a
    // ring of contour around a pixel or two of fill.
    for dpr in [1.0_f32, 1.25, 1.5, 2.0] {
        for line in [0.5_f32, 1.0, 1.5, 2.0] {
            for step in 1..200 {
                let pitch = step as f32 * 0.25;
                let got = candle_widths(pitch, dpr, line, line);
                let cell = format!("pitch {pitch}, line {line} at {dpr}: {got:?}");
                assert_eq!(got.solid, got.body <= 2 * got.border, "{cell}");
                if !got.solid {
                    assert!(got.body - 2 * got.border >= 1, "{cell}");
                }
                assert!(got.body >= got.wick, "{cell}");
            }
        }
    }
}

#[test]
fn the_trader_s_tradingview_zoom_sweep_replays_at_scale_two() {
    // (pitch, body, solid) for zoom/image1..13, then the reference and the
    // spacing captures (`zoom-spec.md` section 1).
    let sweep = [
        (4.34, 8, false),
        (3.92, 6, false),
        (3.52, 6, false),
        (3.17, 6, false),
        (2.85, 6, false),
        // Measured as a 4 px solid bar; at 2.56 the pitch is within a few
        // per cent of the published 2.5 bound this follows.
        (2.56, 6, false),
        (2.31, 4, true),
        (2.07, 4, true),
        (1.87, 2, true),
        (1.69, 2, true),
        (1.37, 2, true),
        (1.10, 2, true),
        (7.75, 12, false),
    ];
    for (pitch, body, solid) in sweep {
        let got = candle_widths(pitch, 2.0, 1.0, 1.0);
        assert_eq!((got.body, got.solid, got.wick), (body, solid, 2), "{pitch}");
    }
    let spacing = candle_widths(6.69, 4.0 / 3.0, 1.0, 1.0);
    assert_eq!(
        (spacing.body, spacing.border, spacing.wick, spacing.solid),
        (7, 1, 1, false)
    );
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Ink {
    Background,
    Border,
    Fill,
    Wick,
}

/// Twenty bars across the pitch at one scale, painted into a device-pixel
/// grid that refuses a pixel painted twice.
fn paint_run(pitch: f32, dpr: f32) -> (CandleWidths, Vec<CandlePixels>, Vec<Vec<Ink>>) {
    let widths = candle_widths(pitch, dpr, 1.0, 1.0);
    let mut run = CandleRun::new(widths, dpr);
    let candles: Vec<_> = (0..20)
        .map(|i| {
            let swing = (i % 5) as f32 * 3.0;
            let ys = CandleYs {
                high: 10.0 + swing,
                body_top: 20.0 + swing,
                body_bottom: 20.0 + swing + (i % 7) as f32 * 2.5,
                low: 50.0 + swing,
            };
            run.place(5.3 + i as f32 * pitch, ys)
        })
        .collect();
    let mut grid = vec![vec![Ink::Background; 2200]; 200];
    let mut ink = |rect: Option<DeviceRect>, what: Ink| {
        let Some(rect) = rect else { return };
        for row in grid
            .iter_mut()
            .take(rect.bottom as usize)
            .skip(rect.top as usize)
        {
            for pixel in row
                .iter_mut()
                .take(rect.right as usize)
                .skip(rect.left as usize)
            {
                assert_eq!(*pixel, Ink::Background, "painted twice: {rect:?}");
                *pixel = what;
            }
        }
    };
    for candle in &candles {
        if candle.solid {
            ink(Some(candle.body), Ink::Border);
        } else {
            ink(candle.fill, Ink::Fill);
            candle
                .bands
                .into_iter()
                .for_each(|band| ink(band, Ink::Border));
        }
        ink(candle.upper_wick, Ink::Wick);
        ink(candle.lower_wick, Ink::Wick);
    }
    (widths, candles, grid)
}

#[test]
fn every_cell_paints_whole_pixels_without_overlap_with_a_centred_wick() {
    for (dpr, (pitch, ..)) in matrix() {
        let (widths, candles, grid) = paint_run(pitch, dpr);
        let cell = format!("pitch {pitch} at {dpr} px/pt");
        for pair in candles.windows(2) {
            assert!(pair[1].body.left >= pair[0].body.right, "{cell}: {pair:?}");
        }
        for candle in &candles {
            let body = candle.body;
            let wick = candle.upper_wick.or(candle.lower_wick).expect("a wick");
            if body.right - body.left == widths.body {
                assert_eq!(
                    wick.left - body.left,
                    body.right - wick.right,
                    "{cell}: wick off centre in {candle:?}"
                );
            }
            if candle.solid {
                assert_eq!(candle.fill, None, "{cell}");
            }
        }
        let fills = grid
            .iter()
            .flatten()
            .filter(|ink| **ink == Ink::Fill)
            .count();
        assert_eq!(fills == 0, widths.solid, "{cell}: fill pixels {fills}");
        // Every edge lands on a device-pixel boundary in logical px too.
        let edge = candles[3].body.in_points(dpr);
        for value in [edge.left, edge.right, edge.top, edge.bottom] {
            let px = value * dpr;
            assert!((px - px.round()).abs() < 1e-3, "{cell}: {value}");
        }
    }
}

#[test]
fn a_body_that_would_overlap_is_cut_on_its_left_and_keeps_its_fill_in_place() {
    // zoom/image5: 6 px bodies 5.7 px apart; TradingView draws `K1F2K2`.
    let (widths, candles, _) = paint_run(2.85, 2.0);
    assert_eq!(widths.body, 6);
    let cut = candles
        .iter()
        .find(|candle| candle.body.right - candle.body.left == 5)
        .expect("a cut body");
    let fill = cut.fill.expect("a fill");
    assert_eq!(
        (
            fill.left - cut.body.left,
            fill.right - fill.left,
            cut.body.right - fill.right
        ),
        (1, 2, 2)
    );
}

#[test]
fn a_doji_and_a_thin_body_are_solid_contour_bars() {
    let widths = candle_widths(10.0, 2.0, 1.0, 1.0);
    let ys = |top: f32, bottom: f32| CandleYs {
        high: 0.0,
        body_top: top,
        body_bottom: bottom,
        low: 40.0,
    };
    let doji = CandleRun::new(widths, 2.0).place(20.0, ys(10.0, 10.0));
    assert!(doji.solid);
    assert_eq!(
        doji.body.bottom - doji.body.top,
        2,
        "a doji is one line tall"
    );
    let thin = CandleRun::new(widths, 2.0).place(20.0, ys(10.0, 11.5));
    assert!(thin.solid, "3 rows leave no fill inside 2 px bands");
    let full = CandleRun::new(widths, 2.0).place(20.0, ys(10.0, 14.0));
    assert_eq!(full.fill.map(|fill| fill.bottom - fill.top), Some(4));
    assert_eq!(
        full.upper_wick.map(|wick| (wick.top, wick.bottom)),
        Some((0, 20))
    );
    assert_eq!(
        full.lower_wick.map(|wick| (wick.top, wick.bottom)),
        Some((28, 80))
    );
}

#[test]
fn style_line_widths_round_to_whole_device_pixels_and_zero_drops_the_contour() {
    // The default 1 px line at the trader's 1.5: two device px, as thick
    // as TradingView's at scale 2 on the same screen. Half of it is the thin
    // single pixel, the option the outline thickness setting reaches.
    assert_eq!(candle_widths(10.0, 1.5, 1.0, 1.0).border, 2);
    assert_eq!(candle_widths(10.0, 1.5, 0.5, 0.5).border, 1);
    assert_eq!(candle_widths(10.0, 1.5, 0.5, 0.5).wick, 1);
    assert_eq!(candle_widths(10.0, 1.5, 1.25, 1.0).border, 2);
    assert_eq!(candle_widths(10.0, 1.5, 2.0, 1.0).border, 3);
    assert_eq!(candle_widths(10.0, 1.0, 1.0, 1.0).border, 1);
    assert_eq!(
        candle_widths(10.0, 1.0, 0.5, 0.5).border,
        1,
        "never under one"
    );
    assert_eq!(candle_widths(10.0, 2.0, 2.0, 2.0).wick, 4);
    let bare = candle_widths(10.0, 1.5, 0.0, 1.0);
    assert_eq!((bare.border, bare.solid), (0, false));
    let candle = CandleRun::new(bare, 1.5).place(
        20.0,
        CandleYs {
            high: 0.0,
            body_top: 10.0,
            body_bottom: 20.0,
            low: 30.0,
        },
    );
    assert_eq!(candle.fill, Some(candle.body));
    assert_eq!(candle.bands, [None; 4]);
}

#[test]
fn unusable_inputs_still_draw_a_one_pixel_candle() {
    for (pitch, dpr) in [
        (f32::NAN, 1.5),
        (-3.0, 1.5),
        (8.0, 0.0),
        (8.0, f32::INFINITY),
    ] {
        let got = candle_widths(pitch, dpr, f32::NAN, f32::NAN);
        assert!(got.body >= 1 && got.wick >= 1 && got.border >= 1, "{got:?}");
    }
}

#[test]
fn the_time_chart_takes_zoom_widths_and_a_dressing_footprint_keeps_its_own() {
    assert_eq!(pixel_bodies(false, false), Some(PixelBodies::Zoom));
    assert_eq!(pixel_bodies(false, true), Some(PixelBodies::Fitted));
    assert_eq!(pixel_bodies(true, false), None, "the flow pane");
    assert_eq!(pixel_bodies(true, true), None);
    // A fitted body keeps the footprint's width, on whole pixels, with the
    // same lines and wick parity as the zoom rule.
    let fitted = fitted_widths(28.8, 1.5, 1.0, 1.0);
    assert_eq!((fitted.body, fitted.border, fitted.wick), (42, 2, 2));
    let thin = fitted_widths(28.8, 1.5, 0.5, 0.5);
    assert_eq!((thin.body, thin.border, thin.wick), (43, 1, 1));
    assert_eq!(fitted_widths(28.8, 2.0, 1.0, 1.0).body, 56);
    assert_eq!(fitted_widths(f32::NAN, 2.0, 1.0, 1.0).body, 2);
}

#[test]
fn paints_put_a_solid_body_in_the_contour_colour_and_skip_invisible_parts() {
    let style = crate::style::CandlePreset::Classic.style();
    let paint = ResolvedCandlePaint {
        fill: Some([1, 2, 3, 255]),
        outline: [9, 9, 9, 255],
        wick: Some([5, 5, 5, 0]),
        ..style.resolved(true, false)
    };
    let ys = CandleYs {
        high: 0.0,
        body_top: 10.0,
        body_bottom: 30.0,
        low: 40.0,
    };
    let solid = CandleRun::new(candle_widths(1.5, 1.5, 1.0, 1.0), 1.5).place(10.0, ys);
    let painted: Vec<_> = solid.paints(&paint).into_iter().flatten().collect();
    assert_eq!(
        painted,
        vec![(solid.body, [9, 9, 9, 255])],
        "wicks invisible"
    );
    let wide = CandleRun::new(candle_widths(10.0, 1.5, 1.0, 1.0), 1.5).place(10.0, ys);
    let painted: Vec<_> = wide.paints(&paint).into_iter().flatten().collect();
    assert_eq!(painted.len(), 5, "a fill and four bands");
    assert_eq!(painted[0], (wide.fill.expect("a fill"), [1, 2, 3, 255]));
    let run = CandleRun::for_frame(PixelBodies::Zoom, 10.0, 0.0, &style, 1.5);
    assert_eq!(run.widths(), candle_widths(10.0, 1.5, 1.0, 1.0));
}
