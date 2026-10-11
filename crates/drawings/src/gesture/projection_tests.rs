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
