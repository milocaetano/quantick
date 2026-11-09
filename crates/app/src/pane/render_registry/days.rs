//! Where the display day turns over: a near-invisible full-height rule under
//! the candles. The time strip names the new day itself, as TradingView does
//! — the day number alone where it turns (`quantick_chart::time_marks`).
//!
//! Where the turns fall is `quantick_chart::day_turn`'s; this module paints
//! the rule.
use super::{Contribution, Package};
use crate::{theme, viewport::Viewport};
use eframe::egui;
use quantick_chart::day_turn::{self, DayTurn};
pub(super) const PACKAGE: Package = Package {
    layers: &[quantick_layers::ChartLayer::DaySeparator],
    contributions: &[Contribution::DayRule(day_rule)],
};
/// The rule across the candles, painted under them with the grid.
pub(in crate::pane) struct DayRulePass<'a> {
    /// Clipped to the candles' extent: the rule spans its height.
    pub painter: &'a egui::Painter,
    pub history: egui::Rect,
    pub total: usize,
    pub candle_width: f32,
    pub viewport: &'a Viewport,
    pub turns: &'a [DayTurn],
}
fn day_rule(p: &mut DayRulePass<'_>) {
    let palette = theme::get(p.painter.ctx());
    let span = (p.history.left(), p.history.right());
    let stroke = egui::Stroke::new(1.0_f32, palette.day_rule());
    for (x, _) in day_turn::turn_xs(p.turns, p.viewport, span, p.total, p.candle_width) {
        p.painter.vline(x, p.history.y_range(), stroke);
    }
}
