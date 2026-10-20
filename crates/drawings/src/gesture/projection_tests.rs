//! The magnet's price and time halves, moved here with the functions.

use super::*;
use crate::gesture::{Bands, MAGNET_REACH_PX};

/// A bar spanning 98 … 102, for the magnet.
fn magnet_candle() -> quantick_engine::Bar {
    use rust_decimal::Decimal;
    quantick_engine::Bar {
        open_time: 0,
        close_time: 59_999,
        open: Decimal::from(100),
        high: Decimal::from(102),
        low: Decimal::from(98),
        close: Decimal::from(101),
        buy_volume: Decimal::ONE,
        sell_volume: Decimal::ONE,
        trade_count: 2,
    }
}

/// Ten pixels per price unit over 90 … 110, so the bar's four levels are
/// far enough apart on screen for "nearest" to be unambiguous.
fn magnet_scale() -> PriceScale {
    PriceScale::from_range(90.0, 110.0, 0.0, 200.0)
}

#[test]
fn the_magnet_takes_the_nearest_ohlc_within_reach() {
    let candle = magnet_candle();
    let scale = magnet_scale();
    let high_y = scale.y(102.0);
    assert_eq!(
        magnet_price_of(&candle, high_y, &scale, MAGNET_REACH_PX),
        Some(102.0),
        "exactly on the high"
    );
    assert_eq!(
        magnet_price_of(&candle, high_y + 4.0, &scale, MAGNET_REACH_PX),
        Some(102.0),
        "inside the reach, and still nearer the high than the close"
    );
}

/// The point of the reach: away from every level the anchor stays free,
/// so a diagonal drawn through open space is still a diagonal.
#[test]
fn the_magnet_lets_go_outside_its_reach() {
    let candle = magnet_candle();
    let scale = magnet_scale();
    assert_eq!(
        magnet_price_of(&candle, scale.y(105.0), &scale, MAGNET_REACH_PX),
        None
    );
}

/// The candle magnet's time half: a bar right of the tape lands on the
/// newest bar, left of it on the oldest, and a one-bar tape is slot 0.
#[test]
fn the_candle_magnet_clamps_the_bar_onto_the_tape() {
    assert_eq!(snap_bar_to_tape(99.9, 20), 19.0);
    assert_eq!(snap_bar_to_tape(-3.0, 20), 0.0);
    assert_eq!(snap_bar_to_tape(7.25, 20), 7.25, "inside stays put");
    assert_eq!(snap_bar_to_tape(5.0, 1), 0.0);
    assert_eq!(snap_bar_to_tape(5.0, 0), 0.0, "an empty tape never panics");
}

/// The candle magnet has no reach: however far the pointer floats above
/// or below the candle, the anchor lands on its nearest level — the rule
/// [`AnchorSnap::NearestOhlc`] glues the anchored VWAP's ball with.
#[test]
fn the_candle_magnet_never_lets_go() {
    let candle = magnet_candle();
    let scale = magnet_scale();
    // Far above every level (y of 110, the top of the scale).
    assert_eq!(
        magnet_price_of(&candle, scale.y(110.0), &scale, f32::INFINITY),
        Some(102.0),
        "way above the candle still lands on its high"
    );
    assert_eq!(
        magnet_price_of(&candle, scale.y(90.0), &scale, f32::INFINITY),
        Some(98.0),
        "way below still lands on its low"
    );
}

/// Near-ties resolve to the genuinely nearest level, never to the first
/// one in the list.
#[test]
fn the_magnet_picks_the_nearest_level_not_the_first() {
    let candle = magnet_candle();
    let scale = magnet_scale();
    // Just under the low: 98 is nearer than the open at 100.
    assert_eq!(
        magnet_price_of(&candle, scale.y(97.6), &scale, MAGNET_REACH_PX),
        Some(98.0)
    );
    // Just over the close: 101 is nearer than the high at 102.
    assert_eq!(
        magnet_price_of(&candle, scale.y(101.2), &scale, MAGNET_REACH_PX),
        Some(101.0)
    );
}

/// One candle at `slot`'s time with these OHLC prices.
fn ohlc(slot: i64, open: i64, high: i64, low: i64, close: i64) -> quantick_engine::Bar {
    use rust_decimal::Decimal;
    quantick_engine::Bar {
        open_time: slot * 60_000,
        close_time: slot * 60_000 + 59_999,
        open: Decimal::from(open),
        high: Decimal::from(high),
        low: Decimal::from(low),
        close: Decimal::from(close),
        buy_volume: Decimal::ONE,
        sell_volume: Decimal::ONE,
        trade_count: 2,
    }
}

const TAPE: usize = 100;
const HISTORY_RIGHT: f32 = 400.0;
/// The bar the pointer stands on: a flat candle at 50, far from every
/// level the tests aim at.
const POINTER_BAR: f32 = 95.0;
/// A visible candle well left of the pointer: open 60, high 70, low 30,
/// close 40.
const DISTANT_BAR: usize = 80;
/// A candle scrolled off the left edge, high 80.
const OFF_SCREEN_BAR: usize = 10;

/// A [`TAPE`]-bar tape built by `bar`, on `viewport` over a 400 px chart.
/// Prices 0 … 100 over 200 px: two pixels a unit, so the 24 px candle reach
/// is twelve units.
fn with_tape(
    viewport: &Viewport,
    bar: impl Fn(usize) -> quantick_engine::Bar,
    test: impl FnOnce(&DrawingProjection<'_>, &Bands),
) {
    use quantick_chart::state::{BarSpec, ChartState, SpecSelector};
    let tape: Vec<quantick_engine::Bar> = (0..TAPE).map(bar).collect();
    let spec = SpecSelector::new(BarSpec::Tick(10));
    let state = ChartState::new(BarSpec::Tick(10));
    let indicators = IndicatorViews::new();
    let projection = DrawingProjection {
        series: PaneSeriesRead {
            history_prefix: &tape,
            state: &state,
            spec: &spec,
        },
        viewport,
        indicators: &indicators,
    };
    let bands: Bands = smallvec::smallvec![Band {
        key: DrawingBand::Price,
        rect: PxRect::from_min_max(Px::default(), crate::pos2(HISTORY_RIGHT, 200.0)),
        scale: Some(PriceScale::from_range(0.0, 100.0, 0.0, 200.0)),
        label: "price".into(),
        refusal: None,
    }];
    test(&projection, &bands);
}

/// A hundred flat candles at 50 with one tall visible candle and one tall
/// off-screen one, on the default viewport (8 px a bar, following the
/// newest), so about the newest fifty are in view.
fn with_visible_tape(test: impl FnOnce(&DrawingProjection<'_>, &Band)) {
    with_visible_bands(|projection, bands| test(projection, &bands[0]));
}

/// [`with_visible_tape`], handing over the whole carve.
fn with_visible_bands(test: impl FnOnce(&DrawingProjection<'_>, &Bands)) {
    let viewport = Viewport::new();
    let (start, end) = viewport.visible_range(HISTORY_RIGHT, TAPE);
    assert!(
        (start..end).contains(&DISTANT_BAR) && !(start..end).contains(&OFF_SCREEN_BAR),
        "fixture: the distant candle is in view and the other is not ({start}..{end})"
    );
    let bar = |slot: usize| match slot {
        DISTANT_BAR => ohlc(slot as i64, 60, 70, 30, 40),
        OFF_SCREEN_BAR => ohlc(slot as i64, 60, 80, 30, 40),
        _ => ohlc(slot as i64, 50, 50, 50, 50),
    };
    with_tape(&viewport, bar, test);
}

fn tool_snap(id: &str) -> AnchorSnap {
    crate::DrawingTool::by_id(id)
        .expect("a registered tool")
        .anchor_snap()
}

/// Where an anchor of `tool` lands with the pointer over [`POINTER_BAR`] at
/// `price`, magnet on.
fn placed(projection: &DrawingProjection<'_>, band: &Band, tool: &str, price: f64) -> ChartPoint {
    let scale = band.scale.as_ref().unwrap();
    let x = projection
        .viewport
        .x_at_bar_position(POINTER_BAR, HISTORY_RIGHT, TAPE);
    projection
        .drawing_point_at(
            crate::pos2(x, scale.y(price)),
            HISTORY_RIGHT,
            TAPE,
            true,
            tool_snap(tool),
            band,
        )
        .expect("a point on the price band")
}

/// A level is drawn off a swing anywhere on screen, not only off the
/// candle under the hand: each of a distant visible candle's four prints
/// takes a horizontal line's anchor and a rectangle's edge, and the bar the
/// trader pointed at stays put.
#[test]
fn a_level_tool_snaps_to_any_visible_candles_ohlc() {
    with_visible_tape(|projection, band| {
        for tool in ["horizontal-line", "rectangle"] {
            for (pointer, print) in [(69.0, 70.0), (59.0, 60.0), (41.0, 40.0), (31.0, 30.0)] {
                let point = placed(projection, band, tool, pointer);
                assert_eq!(point.price, print, "{tool} at {pointer}");
                assert!(
                    (point.bar - POINTER_BAR).abs() < 0.5,
                    "{tool}: the anchor's bar never moves ({})",
                    point.bar
                );
            }
        }
    });
}

/// Only what is in view counts: a candle scrolled off the left edge is no
/// candidate, and out of every visible print's reach the price stays free.
#[test]
fn a_level_tool_ignores_an_off_screen_candle() {
    with_visible_tape(|projection, band| {
        let point = placed(projection, band, "horizontal-line", 81.0);
        assert_eq!(
            point.price, 70.0,
            "the off-screen 80 is nearer, but only the visible 70 is in reach"
        );
        let point = placed(projection, band, "horizontal-line", 85.0);
        assert!(
            (point.price - 85.0).abs() < 0.01,
            "nothing visible in reach leaves the free price, got {}",
            point.price
        );
    });
}

/// A trend line keeps the candle-under-the-pointer magnet: a distant
/// visible high is not a place to bend a diagonal's end to.
#[test]
fn a_trend_line_still_reads_only_the_candle_under_the_pointer() {
    with_visible_tape(|projection, band| {
        let point = placed(projection, band, "trend-line", 69.0);
        assert!(
            (point.price - 69.0).abs() < 0.01,
            "free price, got {}",
            point.price
        );
        let point = placed(projection, band, "trend-line", 55.0);
        assert_eq!(
            point.price, 50.0,
            "the candle under the pointer still snaps"
        );
    });
}

/// A body drag lands a level on a distant visible print: the horizontal
/// line takes the print, and the rectangle's nearer edge takes it while the
/// other edge shifts by the same amount.
#[test]
fn a_level_body_drag_snaps_to_a_distant_visible_print() {
    with_visible_tape(|projection, band| {
        let scale = band.scale.as_ref().unwrap();
        let x = projection
            .viewport
            .x_at_bar_position(POINTER_BAR, HISTORY_RIGHT, TAPE);
        let mut line = [ChartPoint::at(POINTER_BAR, 59.0)];
        projection.snap_body(
            &mut line,
            BodySnap::Levels,
            band,
            crate::pos2(x, scale.y(59.0)),
            (HISTORY_RIGHT, TAPE),
        );
        assert_eq!(line[0].price, 60.0);
        assert_eq!(line[0].bar, POINTER_BAR, "the bar never moves");

        let mut rectangle = [ChartPoint::at(90.0, 69.0), ChartPoint::at(98.0, 20.0)];
        projection.snap_body(
            &mut rectangle,
            BodySnap::Levels,
            band,
            crate::pos2(x, scale.y(69.0)),
            (HISTORY_RIGHT, TAPE),
        );
        assert_eq!(rectangle[0].price, 70.0, "the top edge takes the high");
        assert_eq!(rectangle[1].price, 21.0, "the bottom edge keeps the shape");
        assert_eq!((rectangle[0].bar, rectangle[1].bar), (90.0, 98.0));
    });
}

/// A trend line's body drag is unchanged: its value at the pointer's bar
/// reads only the candle under the pointer.
#[test]
fn a_trend_line_body_drag_ignores_distant_visible_prints() {
    with_visible_tape(|projection, band| {
        let scale = band.scale.as_ref().unwrap();
        let x = projection
            .viewport
            .x_at_bar_position(POINTER_BAR, HISTORY_RIGHT, TAPE);
        let mut line = [ChartPoint::at(90.0, 69.0), ChartPoint::at(99.0, 69.0)];
        projection.snap_body(
            &mut line,
            BodySnap::Line,
            band,
            crate::pos2(x, scale.y(69.0)),
            (HISTORY_RIGHT, TAPE),
        );
        assert_eq!((line[0].price, line[1].price), (69.0, 69.0));
    });
}

/// The edge a level reads is the band's, not the viewport's generous range:
/// a candle mostly scrolled off the left edge is no candidate, even when it
/// is the nearer print, and the first candle fully in view is one.
#[test]
fn a_level_ignores_a_candle_mostly_past_the_left_edge() {
    const CLIPPED: usize = 49;
    const FIRST_IN_VIEW: usize = 50;
    let mut viewport = Viewport::new();
    // Three pixels into the past: bar 49 spans -5 … 3, bar 50 spans 3 … 11.
    viewport.pan_pixels(3.0, TAPE);
    let (start, end) = viewport.visible_range(HISTORY_RIGHT, TAPE);
    assert!(
        (start..end).contains(&CLIPPED),
        "fixture: the generous range still names the clipped candle ({start}..{end})"
    );
    let half = 0.5 * viewport.candle_width();
    assert!(viewport.x_center(CLIPPED, HISTORY_RIGHT, TAPE) < 0.0);
    assert!(viewport.x_center(FIRST_IN_VIEW, HISTORY_RIGHT, TAPE) - half >= 0.0);
    let bar = |slot: usize| match slot {
        CLIPPED => ohlc(slot as i64, 60, 80, 30, 40),
        FIRST_IN_VIEW => ohlc(slot as i64, 60, 70, 30, 40),
        _ => ohlc(slot as i64, 50, 50, 50, 50),
    };
    with_tape(&viewport, bar, |projection, bands| {
        let band = &bands[0];
        let scale = band.scale.as_ref().unwrap();
        let x = viewport.x_at_bar_position(POINTER_BAR, HISTORY_RIGHT, TAPE);
        let point = projection
            .drawing_point_at(
                crate::pos2(x, scale.y(79.0)),
                HISTORY_RIGHT,
                TAPE,
                true,
                tool_snap("horizontal-line"),
                band,
            )
            .unwrap();
        assert_eq!(
            point.price, 70.0,
            "the clipped 80 is nearer, but only the first candle in view counts"
        );
    });
}

/// A level's body drag does not need the pointer to be on a bar: left of
/// the first candle it still lands on a visible print. A trend line still
/// does, and stays where the hand put it.
#[test]
fn a_level_body_drag_snaps_with_the_pointer_left_of_every_bar() {
    let mut viewport = Viewport::new();
    // Pushed all the way into the past: bar 0 sits at the right edge and
    // the rest of the chart is empty canvas.
    viewport.pan_pixels(10_000.0, TAPE);
    let bar = |slot: usize| match slot {
        0 => ohlc(0, 60, 70, 30, 40),
        _ => ohlc(slot as i64, 50, 50, 50, 50),
    };
    with_tape(&viewport, bar, |projection, bands| {
        let band = &bands[0];
        let scale = band.scale.as_ref().unwrap();
        let pointer = crate::pos2(10.0, scale.y(69.0));
        assert_eq!(
            Viewport::slot_of(viewport.bar_at_x(pointer.x, HISTORY_RIGHT, TAPE)),
            None,
            "fixture: the pointer is on no bar"
        );
        let mut line = [ChartPoint::at(-40.0, 69.0)];
        projection.snap_body(
            &mut line,
            BodySnap::Levels,
            band,
            pointer,
            (HISTORY_RIGHT, TAPE),
        );
        assert_eq!(line[0].price, 70.0, "the level takes bar 0's high");
        assert_eq!(line[0].bar, -40.0, "and keeps its bar");

        let before = [ChartPoint::at(-40.0, 69.0), ChartPoint::at(-30.0, 69.0)];
        let mut trend = before;
        projection.snap_body(
            &mut trend,
            BodySnap::Line,
            band,
            pointer,
            (HISTORY_RIGHT, TAPE),
        );
        assert_eq!(trend, before, "a line with no bar under it is left alone");
    });
}

/// A level on an indicator band keeps that band's own magnet: the
/// candles' prints never cross onto it, whatever is in view.
#[test]
fn a_level_on_an_indicator_band_keeps_the_indicator_magnet() {
    with_visible_tape(|projection, price| {
        let band = Band {
            key: DrawingBand::Indicator(crate::PaneKey {
                kind: "native.cvd".into(),
                ordinal: 0,
            }),
            rect: price.rect,
            scale: price.scale,
            label: "CVD".into(),
            refusal: None,
        };
        let scale = band.scale.as_ref().unwrap();
        let x = projection
            .viewport
            .x_at_bar_position(POINTER_BAR, HISTORY_RIGHT, TAPE);
        for value in [69.0, 59.0, 41.0, 1.0] {
            let at = crate::pos2(x, scale.y(value));
            let point_with = |snap| {
                projection
                    .drawing_point_at(at, HISTORY_RIGHT, TAPE, true, snap, &band)
                    .unwrap()
                    .price
            };
            assert_eq!(
                point_with(tool_snap("horizontal-line")),
                point_with(AnchorSnap::Pointer),
                "a level and the old magnet agree at {value}"
            );
            assert!(
                (point_with(tool_snap("horizontal-line")) - value).abs() < 0.01,
                "no candle print reaches an indicator band at {value}"
            );
        }
    });
}

/// One pointer frame on the owning pane.
fn own_pane_frame(
    gestures: &mut super::super::PaneGestures,
    drawings: &mut super::super::Marks,
    projection: &DrawingProjection<'_>,
    bands: &Bands,
    pointer: &super::super::SharedPointer,
) {
    gestures.handle_pointer_tool(
        drawings,
        projection,
        super::super::pointer::PointerFrame {
            bands,
            cached_bands: bands,
            pointer,
            on_pane_chrome: false,
            alt: false,
            shift: false,
            double_clicked: false,
            paper_gesture: false,
            pointer_tool: true,
            shared_pick: None,
            shared: super::super::SharedInteraction::default(),
            held: &|_| false,
        },
    );
}

/// Press a horizontal line placed at `price` over `from_bar`, carry it to
/// `to`, release: a whole drag on the pane that owns it, with the rail's
/// magnet off and the line's own magnet `own`. Answers where the line ends.
fn own_magnet_drag(own: bool, price: f64, from_bar: f32, to: f64) -> f64 {
    let mut result = f64::NAN;
    with_visible_bands(|projection, bands| {
        let scale = bands[0].scale.as_ref().unwrap();
        let mut drawings = super::super::Marks::default();
        assert!(drawings.place(
            crate::DrawingTool::by_id("horizontal-line").unwrap(),
            ChartPoint::at(POINTER_BAR, price),
        ));
        drawings.select(None);
        drawings.set_magnet_at(0, own);
        assert_eq!(drawings.items()[0].own_magnet(), own);
        let x = projection
            .viewport
            .x_at_bar_position(from_bar, HISTORY_RIGHT, TAPE);
        let mut pointer = super::super::SharedPointer {
            position: Some(crate::pos2(x, scale.y(price))),
            area: bands[0].rect,
            over_chrome: false,
            pressed: true,
            down: true,
            released: false,
            history_right: HISTORY_RIGHT,
            total: TAPE,
            magnet: false,
        };
        let mut gestures = super::super::PaneGestures::default();
        own_pane_frame(&mut gestures, &mut drawings, projection, bands, &pointer);
        pointer.pressed = false;
        pointer.position = Some(crate::pos2(x, scale.y(to)));
        own_pane_frame(&mut gestures, &mut drawings, projection, bands, &pointer);
        pointer.down = false;
        pointer.released = true;
        own_pane_frame(&mut gestures, &mut drawings, projection, bands, &pointer);
        result = drawings.items()[0].points[0].price;
    });
    result
}

/// The object's own magnet, with the rail's off, reads every visible candle
/// as the rail's does: a handle drag and a body drag both land a level on a
/// distant candle's print, and without it the line follows the hand.
#[test]
fn a_levels_own_magnet_snaps_to_a_distant_visible_candle() {
    // The handle sits on the anchor, at the pointer's bar.
    assert_eq!(own_magnet_drag(true, 20.0, POINTER_BAR, 69.0), 70.0);
    // The body, grabbed far from the handle.
    assert_eq!(own_magnet_drag(true, 20.0, 60.0, 59.0), 60.0);
    for from_bar in [POINTER_BAR, 60.0] {
        let free = own_magnet_drag(false, 20.0, from_bar, 69.0);
        assert!((free - 69.0).abs() < 0.01, "no magnet stays free: {free}");
    }
}

/// The mirror of a level on another pane snaps where the level's own pane
/// does: one magnet whichever chart the hand is on, by the rail or by the
/// object's own switch.
#[test]
fn a_mirrored_level_snaps_where_its_own_pane_does() {
    use super::super::pointer::PointerOutcome;
    use super::super::{PaneGestures, SharedEdit, SharedPick, SharedPointer};
    for (rail, own) in [(true, false), (false, true)] {
        with_visible_bands(|projection, bands| {
            let scale = bands[0].scale.as_ref().unwrap();
            let own_pane = placed(projection, &bands[0], "horizontal-line", 69.0).price;
            assert_eq!(
                own_pane, 70.0,
                "fixture: its own pane takes the distant high"
            );
            let x = projection
                .viewport
                .x_at_bar_position(POINTER_BAR, HISTORY_RIGHT, TAPE);
            let pick = SharedPick {
                owner: 1,
                index: 0,
                anchor: Some(0),
                locked: false,
                magnet: own,
                snap: tool_snap("horizontal-line"),
            };
            let pointer = |y: f64, pressed: bool| SharedPointer {
                position: Some(crate::pos2(x, scale.y(y))),
                area: bands[0].rect,
                over_chrome: false,
                pressed,
                down: true,
                released: false,
                history_right: HISTORY_RIGHT,
                total: TAPE,
                magnet: rail,
            };
            let mut gestures = PaneGestures::default();
            let mut press = PointerOutcome::default();
            gestures.interact_shared(
                projection,
                bands,
                Some(pick),
                &mut press,
                pointer(20.0, true),
            );
            let mut drag = PointerOutcome::default();
            gestures.interact_shared(projection, bands, None, &mut drag, pointer(69.0, false));
            let Some(SharedEdit::MoveAnchor { price, .. }) = drag.shared.edit else {
                panic!("the mirror moved no anchor: {:?}", drag.shared.edit);
            };
            assert_eq!(price, own_pane, "rail {rail}, own {own}");
        });
    }
}

/// The double click's target in the price band: on a candle's range it is
/// the candle, above it open canvas, and an
/// indicator band is neither; a price band with no scale yet is open canvas.
#[test]
fn a_canvas_double_click_names_the_candle_or_the_open_canvas() {
    with_visible_bands(|projection, bands| {
        let band = &bands[0];
        let scale = band.scale.as_ref().unwrap();
        let x = projection
            .viewport
            .x_center(DISTANT_BAR, HISTORY_RIGHT, TAPE);
        let marks = crate::Drawings::default();
        let hit = |price: f64, band: &Band| {
            let pos = crate::pos2(x, scale.y(price));
            projection.canvas_hit(&marks, pos, band, HISTORY_RIGHT, TAPE)
        };
        assert_eq!(hit(50.0, band), Some(CanvasHit::Candle), "inside 30..70");
        assert_eq!(hit(90.0, band), Some(CanvasHit::Empty), "above the wick");
        let indicator = Band {
            key: DrawingBand::Indicator(crate::PaneKey {
                kind: "native.cvd".into(),
                ordinal: 0,
            }),
            rect: band.rect,
            scale: band.scale,
            label: "CVD".into(),
            refusal: None,
        };
        assert_eq!(hit(50.0, &indicator), None, "a pane band is no canvas");
        // No price scale yet: no candle to hit, so open canvas, never dead.
        let unscaled = Band {
            key: DrawingBand::Price,
            rect: band.rect,
            scale: None,
            label: band.label.clone(),
            refusal: None,
        };
        assert_eq!(hit(50.0, &unscaled), Some(CanvasHit::Empty));
    });
}
