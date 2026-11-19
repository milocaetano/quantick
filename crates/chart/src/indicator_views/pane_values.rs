//! Indicator headline and value-range calculations shared by renderers.
use super::IndicatorView;

/// Vertical padding inside a pane, as a fraction of the value range.
const PANE_PAD_FRAC: f64 = 0.08;

/// The value range a pane auto-fits to: its visible values plus breathing
/// room above and below. `None` while everything visible is still NaN warmup
/// — there is nothing to scale to yet, and no axis to draw.
pub fn pane_auto_range(view: &IndicatorView, start: usize, end: usize) -> Option<(f64, f64)> {
    let (lo, hi) = value_range(view, start, end)?;
    let pad = (hi - lo).max(f64::EPSILON) * PANE_PAD_FRAC;
    Some((lo - pad, hi + pad))
}

/// The newest value of the view's first plot (preview first, else the last
/// committed row) — the pane's headline number.
pub fn last_value(view: &IndicatorView) -> Option<f64> {
    let previewed = view
        .preview
        .as_ref()
        .and_then(|frame| frame.values.first().copied())
        .filter(|v| !v.is_nan());
    previewed.or_else(|| {
        view.columns
            .first()
            .and_then(|column| column.iter().rev().find(|v| !v.is_nan()).copied())
    })
}

/// Min/max over every plot's visible cells (preview included); `None` when
/// everything visible is still NaN warmup.
fn value_range(view: &IndicatorView, start: usize, end: usize) -> Option<(f64, f64)> {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for (index, column) in view.columns.iter().enumerate() {
        // A non-absolute marker column stages 1.0 as a flag, not as a value:
        // it anchors to the pane's own edges and is never read as a y. Folding
        // it into the fit collapses a real plot into a sliver — a CVD pane
        // spanning 1e6..1.1e6 becomes 1.0..1.1e6.
        let is_flag_column = view.descriptor.plots.get(index).is_some_and(|spec| {
            spec.marker.as_ref().is_some_and(|marker| {
                marker.location != quantick_indicators::MarkerLocation::Absolute
            })
        });
        if is_flag_column {
            continue;
        }
        let preview = view
            .preview
            .as_ref()
            .and_then(|frame| frame.values.get(index).copied());
        for value in column[start.min(column.len())..end.min(column.len())]
            .iter()
            .copied()
            .chain(preview)
        {
            if value.is_nan() {
                continue;
            }
            lo = lo.min(value);
            hi = hi.max(value);
        }
    }
    (lo.is_finite() && hi.is_finite()).then_some((lo, hi))
}

#[cfg(test)]
#[path = "pane_values_tests.rs"]
mod tests;
