//! Price framing and the separate visible-bar reference for gutter inversion.
use crate::geometry::{PriceScale, price_window, tape_price_window};
use quantick_engine::Bar;
use rust_decimal::prelude::ToPrimitive as _;

/// Inputs shared by automatic framing and the gutter's flip threshold.
pub struct PriceAxisFit {
    pub native_tape: bool,
    pub tape_only: bool,
    pub tape_range: Option<(f64, f64)>,
    pub previous_auto: Option<(f64, f64)>,
    pub bounds: (f32, f32),
    pub tape_padding_px: f32,
}

impl PriceAxisFit {
    /// The native Tape frames its prints together with any visible candles.
    /// Tape-only panes keep their own fit; ordinary charts fit their candles.
    pub fn resolve<'a>(
        &self,
        visible: impl Iterator<Item = &'a Bar> + Clone,
        partial: Option<&'a Bar>,
        newest: Option<&'a Bar>,
    ) -> Option<((f64, f64), f64)> {
        let (top, bottom) = self.bounds;
        let auto = if self.native_tape {
            let tape_range = if self.tape_only {
                self.tape_range
            } else {
                PriceScale::auto_including(
                    visible.clone(),
                    partial,
                    self.tape_range,
                    top,
                    bottom,
                    0.0,
                )
                .map(|scale| scale.range())
                .or(self.tape_range)
            };
            tape_price_window(
                tape_range,
                newest.and_then(|bar| bar.close.to_f64()),
                self.previous_auto,
                top,
                bottom,
                self.tape_padding_px,
            )
        } else {
            price_window(
                visible.clone(),
                partial,
                None,
                self.previous_auto,
                newest,
                top,
                bottom,
            )
        }?;
        let range = auto.range();
        let span = range.1 - range.0;
        let flip_span = if self.native_tape {
            price_window(visible, partial, Some(range), None, None, top, bottom).map_or(
                span,
                |bars| {
                    let (lo, hi) = bars.range();
                    (hi - lo).max(span)
                },
            )
        } else {
            span
        };
        Some((range, flip_span))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;

    fn bar(low: i64, high: i64, close: i64) -> Bar {
        Bar {
            open_time: 0,
            close_time: 0,
            open: Decimal::from(close),
            high: Decimal::from(high),
            low: Decimal::from(low),
            close: Decimal::from(close),
            buy_volume: Decimal::ONE,
            sell_volume: Decimal::ONE,
            trade_count: 1,
        }
    }

    fn split_fit() -> PriceAxisFit {
        PriceAxisFit {
            native_tape: true,
            tape_only: false,
            tape_range: Some((90.0, 130.0)),
            previous_auto: None,
            bounds: (20.0, 220.0),
            tape_padding_px: 30.0,
        }
    }

    #[test]
    fn split_fit_pads_the_union_once_and_holds_across_frames() {
        let bars = [bar(70, 100, 95)];
        let mut fit = split_fit();
        let (range, _) = fit.resolve(bars.iter(), None, bars.last()).unwrap();
        let scale = PriceScale::from_range(range.0, range.1, 20.0, 220.0);
        assert!((scale.y(70.0) - 190.0).abs() < 1e-4);
        assert!((scale.y(130.0) - 50.0).abs() < 1e-4);
        for _ in 0..20 {
            fit.previous_auto = Some(range);
            assert_eq!(
                fit.resolve(bars.iter(), None, bars.last()).unwrap().0,
                range
            );
        }
    }

    #[test]
    fn empty_split_falls_back_to_tape_and_partial_only_split_fits_its_candle() {
        let mut fit = split_fit();
        let empty = fit.resolve(std::iter::empty(), None, None).unwrap().0;
        let expected = tape_price_window(fit.tape_range, None, None, 20.0, 220.0, 30.0)
            .unwrap()
            .range();
        assert_eq!(empty, expected);
        fit.tape_range = None;
        fit.previous_auto = Some(empty);
        assert_eq!(
            fit.resolve(std::iter::empty(), None, None).unwrap().0,
            empty
        );
        let partial = bar(40, 160, 100);
        let partial_fit = fit
            .resolve(std::iter::empty(), Some(&partial), Some(&partial))
            .unwrap()
            .0;
        assert!(partial_fit.0 < 40.0 && partial_fit.1 > 160.0);
    }

    #[test]
    fn tape_only_ignores_closed_and_forming_candle_extremes() {
        let mut fit = split_fit();
        fit.tape_only = true;
        let bars = [bar(-10_000, 20_000, 100)];
        let partial = bar(-20_000, 30_000, 100);
        let range = fit
            .resolve(bars.iter(), Some(&partial), Some(&partial))
            .unwrap()
            .0;
        let expected = tape_price_window(fit.tape_range, Some(100.0), None, 20.0, 220.0, 30.0)
            .unwrap()
            .range();
        assert_eq!(range, expected);
    }
}
