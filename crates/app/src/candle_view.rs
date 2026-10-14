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

use candles::color32;
pub use candles::{CandleSurface, draw_candle};
pub use quantick_chart::candle_slot::{BarSlot, is_bullish};
pub use style_window::{StylePanelResponse, draw_presets, draw_style_contents};

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

    #[test]
    fn candle_clearance_preserves_fills_and_never_enables_hidden_contours() {
        let ctx = eframe::egui::Context::default();
        let candle = bar("100", "101");
        let scale = crate::chart::PriceScale::from_range(99.0, 102.0, 0.0, 100.0);
        let background = eframe::egui::Color32::from_rgb(19, 23, 34);
        let paint = |style: &CandleStyle, backing| {
            ctx.run(Default::default(), |ctx| {
                draw_candle(
                    &ctx.layer_painter(eframe::egui::LayerId::background()),
                    BarSlot {
                        xc: 40.0,
                        half_width: 3.0,
                    },
                    &scale,
                    &candle,
                    false,
                    CandleSurface {
                        style,
                        contour_backing: backing,
                    },
                    None,
                );
            })
            .shapes
        };
        let style = CandleStyle::default();
        let base = paint(&style, None);
        let isolated = paint(&style, Some(background));
        assert!(isolated.len() > base.len());
        let fills = |shapes: &[eframe::egui::epaint::ClippedShape]| {
            shapes
                .iter()
                .filter_map(|s| {
                    if let eframe::egui::Shape::Rect(rect) = &s.shape {
                        (rect.fill != eframe::egui::Color32::TRANSPARENT)
                            .then_some((rect.rect, rect.fill))
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(fills(&base), fills(&isolated));
        let mut hidden = style;
        hidden.outline_opacity = 0.0;
        hidden.wick_opacity = 0.0;
        assert_eq!(paint(&hidden, None), paint(&hidden, Some(background)));
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
