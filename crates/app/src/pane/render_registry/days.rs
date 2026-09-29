//! Where the display day turns over, written on the time strip only: a tick
//! across the strip at the boundary and the date the new day opens, plus the
//! date of the leftmost bar pinned to the strip's left edge. The time labels
//! carry a clock time only, so without it a chart spanning midnight reads as
//! one day. Nothing is drawn over the candles.
use super::{Contribution, Package};
use crate::{pointer_compass, theme, viewport::Viewport};
use eframe::egui;
use quantick_civil::{CivilDate, weekday_abbr};
/// Space between the tick and the date it opens, and between two dates.
const DAY_LABEL_GAP_PX: f32 = 3.0;
pub(super) const PACKAGE: Package = Package {
    layers: &[quantick_layers::ChartLayer::DaySeparator],
    contributions: &[Contribution::DaySeparator(day_separator)],
};
pub(in crate::pane) struct DaySeparatorPass<'a> {
    pub painter: &'a egui::Painter,
    /// The candles' horizontal extent; the strip's own height.
    pub history: egui::Rect,
    pub strip: egui::Rect,
    pub total: usize,
    pub candle_width: f32,
    pub viewport: &'a Viewport,
    /// Slots whose bar opens a new display day, from `quantick_civil::day_starts`.
    pub starts: &'a [(usize, CivilDate)],
    /// The date of the leftmost visible bar, pinned to the strip's left edge.
    pub first_visible: Option<CivilDate>,
    /// The pointer's chip outranks a date: it is where the trader is looking.
    pub claims: &'a pointer_compass::AxisClaims,
    /// Written here: the spans the dates took, which the time labels avoid.
    pub reserved: Vec<(f32, f32)>,
}
fn day_separator(p: &mut DaySeparatorPass<'_>) {
    let font = egui::FontId::monospace(crate::chart::TIME_LABEL_FONT_PX);
    let (left, right) = (p.history.left(), p.history.right());
    let y = p.strip.center().y;
    let chip_width = p
        .painter
        .layout_no_wrap(
            crate::chart::TimeLabelFormat::Full.sample().to_owned(),
            font.clone(),
            theme::TEXT_MUTED,
        )
        .size()
        .x;
    let mut written_right = f32::NEG_INFINITY;
    // `limit` is where the next tick stands, or the strip's end: a date is
    // never crossed by the tick after it.
    let mut write = |p: &mut DaySeparatorPass<'_>, start: f32, limit: f32, text: String, color| {
        let galley = p.painter.layout_no_wrap(text, font.clone(), color);
        let width = galley.size().x;
        let fits = start >= written_right + DAY_LABEL_GAP_PX && start + width <= limit;
        let centre = start + width / 2.0;
        if !fits || pointer_compass::claimed(centre, width, chip_width, p.claims.iter().copied()) {
            return;
        }
        p.painter
            .galley(egui::pos2(start, y - galley.size().y / 2.0), galley, color);
        p.reserved.push((start, start + width));
        written_right = start + width;
    };
    // Pinned first, so a boundary date close to the left edge wins by
    // pushing it out rather than being pushed itself.
    let ticks: Vec<(f32, CivilDate)> = p
        .starts
        .iter()
        .map(|&(slot, date)| {
            // The left edge of the first bar of the day: between the two
            // days' bars rather than on either.
            let x = p.viewport.x_center(slot, right, p.total) - p.candle_width / 2.0;
            (x, date)
        })
        .filter(|&(x, _)| x >= left && x <= right)
        .collect();
    let limit_before = |index: usize| {
        ticks
            .get(index)
            .map_or(right, |&(x, _)| x - DAY_LABEL_GAP_PX)
    };
    if let Some(date) = p.first_visible {
        let text = format!("{} {}", weekday_abbr(date.weekday()), date.short());
        let limit = limit_before(0);
        write(p, left + DAY_LABEL_GAP_PX, limit, text, theme::TEXT_MUTED);
    }
    for (index, &(x, date)) in ticks.iter().enumerate() {
        p.painter.line_segment(
            [
                egui::pos2(x, p.strip.top()),
                egui::pos2(x, p.strip.bottom()),
            ],
            egui::Stroke::new(1.0_f32, theme::TEXT_MUTED),
        );
        // No time label is written across the tick either.
        p.reserved.push((x, x));
        let limit = limit_before(index + 1);
        write(
            p,
            x + DAY_LABEL_GAP_PX,
            limit,
            day_text(date),
            theme::TEXT_PRIMARY,
        );
    }
}
/// `Tue 29`; the month joins it on the first of the month, where the month
/// is what changed.
fn day_text(date: CivilDate) -> String {
    let (_, _, day) = date.ymd();
    if day == 1 {
        format!("{} {}", weekday_abbr(date.weekday()), date.short())
    } else {
        format!("{} {day}", weekday_abbr(date.weekday()))
    }
}
/// Whether a time label spanning `from..to` would touch a date's span.
pub(super) fn reserved_by(from: f32, to: f32, reserved: &[(f32, f32)]) -> bool {
    reserved
        .iter()
        .any(|&(start, end)| from < end + DAY_LABEL_GAP_PX && to > start - DAY_LABEL_GAP_PX)
}
#[cfg(test)]
mod days_tests {
    use super::*;

    #[test]
    fn a_day_reads_weekday_and_date_and_names_the_month_when_it_turns() {
        assert_eq!(day_text(CivilDate::from_ymd(2026, 9, 29)), "Tue 29");
        assert_eq!(day_text(CivilDate::from_ymd(2026, 10, 1)), "Thu 01 Oct");
    }

    #[test]
    fn a_time_label_stands_aside_only_where_a_date_is_written() {
        let reserved = [(100.0, 140.0)];
        assert!(reserved_by(120.0, 150.0, &reserved), "overlapping");
        assert!(reserved_by(141.0, 170.0, &reserved), "inside the gap");
        assert!(!reserved_by(150.0, 190.0, &reserved), "clear of it");
        assert!(!reserved_by(40.0, 90.0, &reserved), "clear before it");
        assert!(!reserved_by(120.0, 150.0, &[]), "no dates, no gaps");
    }
}
