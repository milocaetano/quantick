//! The magnet's price and time halves, moved here with the functions.

use super::*;
use crate::gesture::MAGNET_REACH_PX;

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

/// A hundred flat candles at 50 with one tall visible candle and one tall
/// off-screen one, on the default viewport (8 px a bar, following the
/// newest) over a 400 px chart, so about the newest fifty are in view.
/// Prices 0 … 100 over 200 px: two pixels a unit, so the 24 px candle
/// reach is twelve units.
fn with_visible_tape(test: impl FnOnce(&DrawingProjection<'_>, &Band)) {
    use quantick_chart::state::{BarSpec, ChartState, SpecSelector};
    let tape: Vec<quantick_engine::Bar> = (0..TAPE)
        .map(|slot| match slot {
            DISTANT_BAR => ohlc(slot as i64, 60, 70, 30, 40),
            OFF_SCREEN_BAR => ohlc(slot as i64, 60, 80, 30, 40),
            _ => ohlc(slot as i64, 50, 50, 50, 50),
        })
        .collect();
    let spec = SpecSelector::new(BarSpec::Tick(10));
    let state = ChartState::new(BarSpec::Tick(10));
    let viewport = Viewport::new();
    let indicators = IndicatorViews::new();
    let projection = DrawingProjection {
        series: PaneSeriesRead {
            history_prefix: &tape,
            state: &state,
            spec: &spec,
        },
        viewport: &viewport,
        indicators: &indicators,
    };
    let (start, end) = viewport.visible_range(HISTORY_RIGHT, TAPE);
    assert!(
        (start..end).contains(&DISTANT_BAR) && !(start..end).contains(&OFF_SCREEN_BAR),
        "fixture: the distant candle is in view and the other is not ({start}..{end})"
    );
    let band = Band {
        key: DrawingBand::Price,
        rect: PxRect::from_min_max(Px::default(), crate::pos2(HISTORY_RIGHT, 200.0)),
        scale: Some(PriceScale::from_range(0.0, 100.0, 0.0, 200.0)),
        label: "price".into(),
        refusal: None,
    };
    test(&projection, &band);
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
