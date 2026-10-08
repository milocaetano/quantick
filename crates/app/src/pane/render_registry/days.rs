//! Where the display day turns over. Under the candles, a near-invisible
//! full-height rule. On the time strip, a tick at the boundary with the time
//! the old day's last bar closed left of it, and the date the new day opens
//! with its first bar's open time right of it (`18:24 | Tue 29 09:00`), plus
//! the date of the leftmost bar pinned to the strip's left edge. The time
//! labels carry a clock time only, so without it a chart spanning midnight
//! reads as one day, and nothing says when each session ran.
//!
//! The text and where it goes are `quantick_chart::day_turn`'s; this module
//! paints them.
use super::{Contribution, Package};
use crate::{pointer_compass, theme, viewport::Viewport};
pub(super) use day_turn::reserved_by;
use eframe::egui;
use quantick_chart::day_turn::{
    self, DAY_LABEL_GAP_PX, DayTurn, TurnWidths, day_text, ended_text, opened_suffix, pinned_text,
};
use quantick_civil::{CivilDate, TzOffset};
pub(super) const PACKAGE: Package = Package {
    layers: &[quantick_layers::ChartLayer::DaySeparator],
    contributions: &[
        Contribution::DayRule(day_rule),
        Contribution::DaySeparator(day_separator),
    ],
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
    let span = (p.history.left(), p.history.right());
    for (x, _) in day_turn::turn_xs(p.turns, p.viewport, span, p.total, p.candle_width) {
        p.painter.line_segment(
            [
                egui::pos2(x, p.history.top()),
                egui::pos2(x, p.history.bottom()),
            ],
            egui::Stroke::new(1.0_f32, theme::DAY_RULE),
        );
    }
}
pub(in crate::pane) struct DaySeparatorPass<'a> {
    pub painter: &'a egui::Painter,
    /// The candles' horizontal extent.
    pub history: egui::Rect,
    pub strip: egui::Rect,
    pub total: usize,
    pub candle_width: f32,
    pub viewport: &'a Viewport,
    pub turns: &'a [DayTurn],
    /// The display timezone the times are written in.
    pub tz: TzOffset,
    /// The date of the leftmost visible bar, pinned to the strip's left edge.
    pub first_visible: Option<CivilDate>,
    /// The pointer's chip outranks a label: it is where the trader is looking.
    pub claims: &'a pointer_compass::AxisClaims,
    /// Written here: the spans the labels took, which the time labels avoid.
    pub reserved: Vec<(f32, f32)>,
}
fn day_separator(p: &mut DaySeparatorPass<'_>) {
    let font = egui::FontId::monospace(crate::chart::TIME_LABEL_FONT_PX);
    let (painter, claims, tz) = (p.painter, p.claims, p.tz);
    let (left, right) = (p.history.left(), p.history.right());
    let y = p.strip.center().y;
    let sample = crate::chart::TimeLabelFormat::Full.sample();
    let chip_width = painter
        .layout_no_wrap(sample.to_owned(), font.clone(), egui::Color32::WHITE)
        .size()
        .x;
    // Monospace: every label is its length in this wide.
    let char_width = chip_width / sample.len() as f32;
    let claimed = |centre: f32, width: f32| {
        pointer_compass::claimed(centre, width, chip_width, claims.iter().copied())
    };
    let ticks = day_turn::turn_xs(p.turns, p.viewport, (left, right), p.total, p.candle_width);
    let widths: Vec<_> = ticks
        .iter()
        .map(|&(x, turn)| (x, TurnWidths::of(turn, char_width)))
        .collect();
    let pinned = p.first_visible.map(pinned_text);
    let plan = day_turn::plan_strip(
        left,
        right,
        pinned.as_ref().map(|text| text.len() as f32 * char_width),
        &widths,
        |start, width| !claimed(start + width / 2.0, width),
        |x| !claimed(x, 0.0),
    );
    let format = |color| egui::TextFormat::simple(font.clone(), color);
    let job = |text: String, color| egui::text::LayoutJob::single_section(text, format(color));
    // Lays out and writes one label at `start`, and reserves its span.
    let draw = |reserved: &mut Vec<(f32, f32)>, start: f32, job| {
        let galley = painter.layout_job(job);
        reserved.push((start, start + galley.size().x));
        let at = egui::pos2(start, y - galley.size().y / 2.0);
        painter.galley(at, galley, theme::TEXT_MUTED);
    };
    if let (Some(at), Some(text)) = (plan.pinned_at, pinned) {
        draw(&mut p.reserved, at, job(text, theme::TEXT_MUTED));
    }
    for (&(x, turn), tick) in ticks.iter().zip(&plan.ticks) {
        // Under the pointer's chip the chip wins, tick and labels alike.
        if !tick.drawn {
            continue;
        }
        painter.line_segment(
            [
                egui::pos2(x, p.strip.top()),
                egui::pos2(x, p.strip.bottom()),
            ],
            egui::Stroke::new(1.0_f32, theme::TEXT_MUTED),
        );
        // No time label is written across the tick either.
        p.reserved.push((x, x));
        let Some(labels) = tick.labels else {
            continue;
        };
        if let (Some(at), Some(ms)) = (labels.ended_at, turn.ended_ms) {
            draw(
                &mut p.reserved,
                at,
                job(ended_text(ms, tz), theme::TEXT_MUTED),
            );
        }
        // The date as bright as a date alone, its start time as muted as
        // the clock labels beside it.
        let mut dated = job(day_text(turn.date), theme::TEXT_PRIMARY);
        if let (true, Some(ms)) = (labels.with_opened, turn.opened_ms) {
            dated.append(&opened_suffix(ms, tz), 0.0, format(theme::TEXT_MUTED));
        }
        draw(&mut p.reserved, x + DAY_LABEL_GAP_PX, dated);
    }
}
