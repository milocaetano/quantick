//! View-models for the widget panes: what each pane draws, as text, colour
//! roles and pixel geometry, built from `orderflow::widgets`' models. A
//! drawer paints these and decides nothing; a read capability serialises the
//! same values, so the screen and the read cannot disagree.

use quantick_appearance::Token;
use rust_decimal::Decimal;

pub mod big_trades;
pub mod depth;

#[cfg(test)]
mod big_trades_tests;
#[cfg(test)]
mod depth_tests;

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
