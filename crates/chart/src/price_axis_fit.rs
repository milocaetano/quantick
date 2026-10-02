//! Price framing and the separate visible-bar reference for gutter inversion.
use crate::geometry::{price_window, tape_price_window};
use quantick_engine::Bar;
use rust_decimal::prelude::ToPrimitive as _;

/// Inputs shared by automatic framing and the gutter's flip threshold.
pub struct PriceAxisFit {
    pub native_tape: bool,
    pub tape_range: Option<(f64, f64)>,
    pub previous_auto: Option<(f64, f64)>,
    pub bounds: (f32, f32),
    pub tape_padding_px: f32,
}
impl PriceAxisFit {
    /// The native Tape frames its prints, while inversion still measures how
    /// flat the visible candles are. Ordinary charts use their own fit for both.
    pub fn resolve<'a>(
        &self,
        visible: impl Iterator<Item = &'a Bar> + Clone,
        partial: Option<&'a Bar>,
        newest: Option<&'a Bar>,
    ) -> Option<((f64, f64), f64)> {
        let (top, bottom) = self.bounds;
        let auto = if self.native_tape {
            tape_price_window(
                self.tape_range,
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
