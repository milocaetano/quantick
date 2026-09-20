//! Egui adapter for candle appearance.
//!
//! Direction, opacity and input sanitization stay in [`crate::style`], while
//! pixel geometry stays in [`crate::chart`]. This module only turns those pure
//! descriptions into egui shapes and widgets.

// The bar itself in `candles`, the appearance window in `style_window`.
// The leaves are private: a surface that draws one candle no longer reads
// the settings panel to find it.
mod candles;
mod style_window;

pub use candles::{BarSlot, draw_candle, is_bullish};
use candles::color32;
pub use style_window::draw_style_window;

#[cfg(test)]
mod tests {
    use quantick_engine::Bar;

    use super::*;
    use crate::style::CandleStyle;
    use rust_decimal::Decimal;
    use std::str::FromStr as _;

    fn bar(open: &str, close: &str) -> Bar {
        let open = Decimal::from_str(open).unwrap();
        let close = Decimal::from_str(close).unwrap();
        Bar {
            open_time: 0,
            close_time: 0,
            open,
            high: open.max(close),
            low: open.min(close),
            close,
            buy_volume: Decimal::ZERO,
            sell_volume: Decimal::ZERO,
            trade_count: 1,
        }
    }

    #[test]
    fn direction_reads_a_flat_bar_as_bullish() {
        assert!(is_bullish(&bar("100", "101")));
        assert!(is_bullish(&bar("100", "100")));
        assert!(!is_bullish(&bar("100", "99")));
    }

    /// The last-price chip picks its colour straight from the candle palette
    /// using this predicate. Both are on the same canvas at the same price, so
    /// a reader would take any disagreement as information — this pins them
    /// to one another.
    #[test]
    fn the_candle_outline_follows_the_same_direction_the_chip_would_use() {
        let style = CandleStyle::default();
        for (open, close) in [("100", "101"), ("100", "100"), ("100", "99")] {
            let bar = bar(open, close);
            let chip = if is_bullish(&bar) {
                style.bull_outline
            } else {
                style.bear_outline
            };
            let candle = style.resolved(is_bullish(&bar), false);
            assert_eq!(
                [candle.outline[0], candle.outline[1], candle.outline[2]],
                chip,
                "{open}->{close} painted a candle and a chip of different colours"
            );
        }
    }
}
