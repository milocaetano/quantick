//! View-models for the widget panes: what each pane draws, as text, colour
//! roles and pixel geometry, built from `orderflow::widgets`' models. A
//! drawer paints these and decides nothing; a read capability serialises the
//! same values, so the screen and the read cannot disagree.

use quantick_appearance::Token;
use quantick_civil::{TzOffset, civil_utc};
use rust_decimal::Decimal;

use crate::footprint_projection::fmt_qty;

pub mod big_trades;
pub mod depth;
pub mod liquidations;
pub mod open_interest;
pub mod spot_perp_cvd;

#[cfg(test)]
mod big_trades_tests;
#[cfg(test)]
mod depth_tests;
#[cfg(test)]
mod derivatives_tests;

/// Pixels a line keeps clear of its plot's top and bottom edge.
pub const LINE_PAD_PX: f32 = 4.0;

/// One text cell and the theme token it is inked with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WidgetCell {
    pub text: String,
    pub token: Token,
}

impl WidgetCell {
    fn new(text: impl Into<String>, token: Token) -> Self {
        Self {
            text: text.into(),
            token,
        }
    }
}

/// One horizontal bar: its width and the token it is filled with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WidgetBar {
    pub width_px: f32,
    pub token: Token,
}

/// `price` at the tick's precision, never with a thousands separator:
/// `76200.00` for a `0.50` tick.
fn price_text(price: Decimal, tick: Decimal) -> String {
    let scale = tick.scale() as usize;
    format!("{:.scale$}", price.round_dp(tick.scale()))
}

/// `part` of `whole` as a share of `full_px`; nothing when `whole` is zero.
fn bar_px(part: Decimal, whole: Decimal, full_px: f32) -> f32 {
    use rust_decimal::prelude::ToPrimitive as _;
    if whole <= Decimal::ZERO {
        return 0.0;
    }
    (part / whole).to_f32().unwrap_or(0.0) * full_px
}

/// `value` written with the unit's prefix: `$942.1k`.
fn amount(value: Decimal, unit: big_trades::NotionalUnit) -> String {
    format!("{}{}", unit.prefix, fmt_qty(value))
}

/// `00:13:52` in `tz`.
fn clock_text(timestamp_ms: i64, tz: TzOffset) -> String {
    let (_, _, _, hour, minute, second) = civil_utc(timestamp_ms.saturating_add(tz.offset_ms()));
    format!("{hour:02}:{minute:02}:{second:02}")
}

/// `(time, value)` points as pixels in a `width_px` by `height_px` plot:
/// time across `[t0, t1]`, value down from `hi` to `lo`, [`LINE_PAD_PX`]
/// clear of each edge. A flat range sits mid-height; a single instant at
/// the left edge.
fn line_px(
    points: impl IntoIterator<Item = (i64, f64)>,
    (t0, t1): (i64, i64),
    (lo, hi): (f64, f64),
    (width_px, height_px): (f32, f32),
) -> Vec<[f32; 2]> {
    let span_ms = (t1 - t0) as f64;
    let usable = f64::from(height_px - 2.0 * LINE_PAD_PX);
    points
        .into_iter()
        .map(|(time_ms, value)| {
            let x = if span_ms > 0.0 {
                (time_ms - t0) as f64 * f64::from(width_px) / span_ms
            } else {
                0.0
            };
            let y = if hi > lo {
                f64::from(LINE_PAD_PX) + (hi - value) * usable / (hi - lo)
            } else {
                f64::from(height_px) / 2.0
            };
            [x as f32, y as f32]
        })
        .collect()
}

/// The smallest and largest of `values`; nothing when there are none.
fn value_range(values: impl IntoIterator<Item = f64>) -> Option<(f64, f64)> {
    values
        .into_iter()
        .filter(|value| value.is_finite())
        .fold(None, |range, value| match range {
            None => Some((value, value)),
            Some((lo, hi)) => Some((lo.min(value), hi.max(value))),
        })
}
