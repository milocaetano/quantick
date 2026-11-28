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
pub mod scene;

#[cfg(test)]
mod big_trades_tests;
#[cfg(test)]
mod depth_tests;
#[cfg(test)]
mod derivatives_tests;
#[cfg(test)]
mod scene_tests;

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

/// Where [`amount`] switches to billions.
const BILLION: u64 = 1_000_000_000;

/// `value` written with the unit's prefix: `$942.1k`, and `$6.98B` from a
/// billion, where the quantity format would write `6980.0M`.
fn amount(value: Decimal, unit: big_trades::NotionalUnit) -> String {
    use rust_decimal::prelude::ToPrimitive as _;
    let figure = if value.abs() >= Decimal::from(BILLION) {
        crate::geometry::compact_value(value.to_f64().unwrap_or(f64::NAN))
    } else {
        fmt_qty(value)
    };
    format!("{}{figure}", unit.prefix)
}

/// `00:13:52` in `tz`.
fn clock_text(timestamp_ms: i64, tz: TzOffset) -> String {
    let (_, _, _, hour, minute, second) = civil_utc(timestamp_ms.saturating_add(tz.offset_ms()));
    format!("{hour:02}:{minute:02}:{second:02}")
}
