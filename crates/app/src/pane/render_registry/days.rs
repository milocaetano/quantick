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
        painter.text(
            egui::pos2(x + SEAM_LABEL_INSET_PX, pane.top() + SEAM_LABEL_INSET_PX),
            egui::Align2::LEFT_TOP,
            format!("{} {}", weekday_abbr(date.weekday()), date.short()),
            egui::FontId::proportional(SEAM_LABEL_PT),
            theme::SEAM_LABEL,
        );
    }
}
