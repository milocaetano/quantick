//! Font-aware pane height allocation, independent of window geometry.
use super::{MAX_PANES, PaneSizing};
pub const MIN_CHART_HEIGHT_PX: f32 = 120.0;
pub fn pane_heights(band_height: f32, sizing: &[PaneSizing], strip_height: f32) -> Vec<f32> {
    let count = sizing.len().min(MAX_PANES);
    if count == 0 {
        return Vec::new();
    }
    let band_height = band_height.max(0.0);
    // Strips are mandatory chrome; only the expansion above a strip has to be
    // negotiated for.
    let mandatory = strip_height * count as f32;
    let mut budget = (band_height - MIN_CHART_HEIGHT_PX - mandatory).max(0.0);

    // Explicit choices are served first, in two passes over the same order:
    // a pane the user dragged or expanded by hand must get its height even
    // when the automatic ones above it would have spent the budget. Without
    // this, clicking a collapsed strip open is a click that does nothing.
    let mut heights = vec![strip_height; count];
    for explicit in [true, false] {
        for (index, sizing) in sizing[..count].iter().enumerate() {
            if matches!(sizing, PaneSizing::Manual(_)) != explicit {
                continue;
            }
            if matches!(sizing, PaneSizing::Collapsed) {
                continue;
            }
            let desired = sizing.desired(band_height).max(strip_height);
            let extra = desired - strip_height;
            if extra <= budget {
                budget -= extra;
                heights[index] = desired;
            }
        }
    }

    heights
}

/// Above this magnitude a headline value is shown with fewer decimals: a
/// five-figure price needs no hundredths.
const LARGE_VALUE_THRESHOLD: f64 = 1000.0;
/// Decimals shown at or above [`LARGE_VALUE_THRESHOLD`].
const LARGE_VALUE_DECIMALS: usize = 1;
pub fn format_value(v: f64) -> String {
    if v.abs() >= LARGE_VALUE_THRESHOLD {
        format!("{v:.*}", LARGE_VALUE_DECIMALS)
    } else {
        quantick_engine::display::display_f64(v)
    }
}

#[cfg(test)]
mod value_tests {
    use super::*;
    #[test]
    fn format_value_drops_decimals_only_at_price_scale() {
        assert_eq!(format_value(1234.5678), "1234.6");
        assert_eq!(format_value(0.12345), "0.12");
        assert_eq!(format_value(-9999.99), "-10000.0");
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn enlarged_strips_keep_explicit_priority_and_collapsed_floor() {
        for strip in [24.0, 48.0] {
            let heights = pane_heights(
                400.0,
                &[
                    PaneSizing::Auto,
                    PaneSizing::Manual(160.0),
                    PaneSizing::Collapsed,
                ],
                strip,
            );
            assert_eq!(heights[1], 160.0);
            assert_eq!(heights[2], strip);
            assert!(heights.iter().all(|h| *h >= strip));
            assert!(heights.iter().sum::<f32>() <= 280.0);
        }
    }
}
