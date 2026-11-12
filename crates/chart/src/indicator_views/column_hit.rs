//! Column picking uses the same zero baseline and body width as painting.
use quantick_indicators::PlotStyle;

use super::IndicatorView;

/// Wide column bodies occupy this fraction of the candle width.
pub const COLUMNS_WIDTH_FRAC: f32 = 0.8;

/// Visible geometry supplied by the last painted projection.
pub struct ColumnPick {
    pub visible: std::ops::Range<usize>,
    pub partial_slot: Option<usize>,
    pub candle_width: f32,
    pub pointer: (f32, f32),
}

impl IndicatorView {
    /// `None` leaves non-column plots to their existing line picker. Columns
    /// return a body hit, never a segment connecting adjacent column tops.
    pub fn column_hit(
        &self,
        index: usize,
        pick: &ColumnPick,
        x: impl Fn(usize) -> f32,
        y: impl Fn(f64) -> f32,
    ) -> Option<bool> {
        if self.descriptor.plots.get(index)?.style != PlotStyle::Columns {
            return None;
        }
        if self.hidden || !self.plot_style(index)?.visible {
            return Some(false);
        }
        let Some(column) = self.columns.get(index) else {
            return Some(false);
        };
        let preview = pick
            .partial_slot
            .filter(|slot| *slot == column.len() && pick.visible.contains(slot))
            .and_then(|slot| Some((slot, *self.preview.as_ref()?.values.get(index)?)));
        let half = (pick.candle_width * COLUMNS_WIDTH_FRAC / 2.0).max(0.5);
        let zero = y(0.0);
        let mut cells = (pick.visible.start..pick.visible.end.min(column.len()))
            .map(|row| (row, column[row]))
            .chain(preview);
        Some(cells.any(|(row, value)| {
            let top = y(value);
            value.is_finite()
                && top.is_finite()
                && zero.is_finite()
                && (pick.pointer.0 - x(row)).abs() <= half
                && pick.pointer.1 >= top.min(zero)
                && pick.pointer.1 <= top.max(zero)
        }))
    }
}

#[cfg(test)]
#[path = "column_hit_tests.rs"]
mod tests;
