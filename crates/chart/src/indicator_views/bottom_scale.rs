//! Independent, zero-based scales for native overlays inside the price plot.

use std::sync::Arc;

use quantick_indicators::native::native;

use super::{IndicatorView, IndicatorViews};
/// A zero-based volume mapping independent of all price-axis policy.
#[derive(Clone, Copy)]
pub struct BottomScale {
    maximum: f64,
    height: f32,
    bottom: f32,
}

impl BottomScale {
    /// Map the measured quantity to its bottom-anchored chart band.
    pub fn y(self, value: f64) -> f32 {
        self.bottom - (value / self.maximum) as f32 * self.height
    }
}

impl IndicatorView {
    /// A data-provenance caveat the legend and automation can expose.
    pub fn source_note(&self) -> Option<&'static str> {
        native(&self.kind)?.source_note()
    }

    /// The legend's value; split volume columns describe one measurement.
    pub fn headline_value(&self) -> Option<f64> {
        if !native(&self.kind).is_some_and(|entry| entry.measures_volume) {
            return self.columns.first()?.last().copied();
        }
        (0..2)
            .filter_map(|index| {
                if let Some(frame) = &self.preview {
                    frame.values.get(index).copied()
                } else {
                    self.columns.get(index)?.last().copied()
                }
            })
            .find(|value| value.is_finite())
    }

    /// Bottom overlays never use the candle scale, even when it is inverted.
    pub fn bottom_scale(
        &self,
        start: usize,
        end: usize,
        partial_slot: Option<usize>,
        top: f32,
        bottom: f32,
    ) -> Option<BottomScale> {
        let fraction = native(&self.kind)?.bottom_scale_fraction?;
        let mut maximum = 0.0_f64;
        for (index, column) in self.columns.iter().enumerate() {
            if !self.plot_style(index).is_some_and(|style| style.visible) {
                continue;
            }
            let preview = partial_slot
                .filter(|slot| *slot == column.len() && (start..end).contains(slot))
                .and_then(|_| self.preview.as_ref()?.values.get(index).copied());
            for value in column[start.min(end).min(column.len())..end.min(column.len())]
                .iter()
                .copied()
                .chain(preview)
            {
                if value.is_finite() {
                    maximum = maximum.max(value);
                }
            }
        }
        Some(BottomScale {
            maximum: if maximum > 0.0 { maximum } else { 1.0 },
            height: (bottom - top) * fraction,
            bottom,
        })
    }
}

impl IndicatorViews {
    /// Label the source's declared volume unit; never guess it from a venue id.
    /// Missing/non-finite latest volume is unavailable, while real zero is valid.
    pub fn label_volume(&mut self, traded_volume: bool) {
        for view in &mut self.views {
            if !native(&view.kind).is_some_and(|entry| entry.measures_volume) {
                continue;
            }
            let available = view.headline_value().is_some();
            let label = match (traded_volume, available) {
                (true, true) => "Volume",
                (false, true) => "Volume · Ticks",
                (true, false) => "Volume · Unavailable",
                (false, false) => "Volume · Ticks · Unavailable",
            };
            if &*view.label != label {
                view.label = Arc::from(label);
            }
        }
    }
}

#[cfg(test)]
#[path = "bottom_scale_tests.rs"]
mod tests;
