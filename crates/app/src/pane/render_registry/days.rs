//! Where the display day turns over: a faint rule through the candles, a tick
//! on the time strip and the date the new day opens. The time labels carry a
//! clock time only, so without it a chart spanning midnight reads as one day.
use super::super::{SEAM_LABEL_INSET_PX, SEAM_LABEL_PT};
use super::{Contribution, Package};
use crate::{theme, viewport::Viewport};
use eframe::egui;
use quantick_civil::{CivilDate, weekday_abbr};
/// How far the tick reaches down into the time strip from its top rule.
const DAY_TICK_PX: f32 = 5.0;
pub(super) const PACKAGE: Package = Package {
    layers: &[quantick_layers::ChartLayer::DaySeparator],
    contributions: &[Contribution::DaySeparator(day_separator)],
};
pub(in crate::pane) struct DaySeparatorPass<'a> {
    pub painter: &'a egui::Painter,
    pub pane: egui::Rect,
    pub strip: egui::Rect,
    pub total: usize,
    pub candle_width: f32,
    pub viewport: &'a Viewport,
    /// Slots whose bar opens a new display day, from `quantick_civil::day_starts`.
    pub starts: &'a [(usize, CivilDate)],
}
fn day_separator(p: &mut DaySeparatorPass<'_>) {
    let DaySeparatorPass {
        painter,
        pane,
        strip,
        total,
        candle_width,
        viewport,
        starts,
    } = *p;
    // Right edge of the last date written: zoomed out, midnights sit closer
    // than a date is wide, and the rule and tick still mark every one.
    let mut written_right = f32::NEG_INFINITY;
    for &(slot, date) in starts {
        // The left edge of the first bar of the day, like the seam: the rule
        // separates two bars rather than crossing one.
        let x = viewport.x_center(slot, pane.right(), total) - candle_width / 2.0;
        if x < pane.left() || x > pane.right() {
            continue; // off-screen
        }
        painter.line_segment(
            [egui::pos2(x, pane.top()), egui::pos2(x, pane.bottom())],
            egui::Stroke::new(1.0_f32, theme::SEAM_LINE),
        );
        painter.line_segment(
            [
                egui::pos2(x, strip.top()),
                egui::pos2(x, strip.top() + DAY_TICK_PX),
            ],
            egui::Stroke::new(1.0_f32, theme::TEXT_MUTED),
        );
        let galley = painter.layout_no_wrap(
            format!("{} {}", weekday_abbr(date.weekday()), date.short()),
            egui::FontId::proportional(SEAM_LABEL_PT),
            theme::SEAM_LABEL,
        );
        let width = galley.size().x;
        if let Some(left) = label_left(x, width, pane.left(), pane.right(), written_right) {
            painter.galley(
                egui::pos2(left, pane.top() + SEAM_LABEL_INSET_PX),
                galley,
                theme::SEAM_LABEL,
            );
            written_right = left + width;
        }
    }
}
/// Where a date `width` wide starts beside the rule at `x`: right of it, or
/// left of it when the pane ends first, never past either pane edge nor over
/// the date written before it (whose right edge is `written_right`).
fn label_left(x: f32, width: f32, left: f32, right: f32, written_right: f32) -> Option<f32> {
    [x + SEAM_LABEL_INSET_PX, x - SEAM_LABEL_INSET_PX - width]
        .into_iter()
        .find(|&start| {
            start >= left.max(written_right + SEAM_LABEL_INSET_PX) && start + width <= right
        })
}
#[cfg(test)]
mod days_tests {
    use super::*;

    #[test]
    fn a_date_sits_right_of_its_rule_when_the_pane_has_room() {
        let start = label_left(100.0, 60.0, 0.0, 500.0, f32::NEG_INFINITY);
        assert_eq!(start, Some(100.0 + SEAM_LABEL_INSET_PX));
    }

    #[test]
    fn a_date_near_the_right_edge_flips_left_of_its_rule() {
        let start = label_left(480.0, 60.0, 0.0, 500.0, f32::NEG_INFINITY);
        assert_eq!(start, Some(480.0 - SEAM_LABEL_INSET_PX - 60.0));
    }

    #[test]
    fn a_date_that_fits_neither_side_is_not_written() {
        assert_eq!(label_left(30.0, 60.0, 0.0, 80.0, f32::NEG_INFINITY), None);
    }

    #[test]
    fn a_date_never_overlaps_the_one_written_before_it() {
        // The previous rule at 100 wrote its date to 164; a rule 12 px later
        // has no room on either side, one past it does.
        assert_eq!(label_left(112.0, 60.0, 0.0, 500.0, 164.0), None);
        assert!(label_left(200.0, 60.0, 0.0, 500.0, 164.0).is_some());
    }
}
