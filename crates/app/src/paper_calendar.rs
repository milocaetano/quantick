//! Calendar-shaped date filtering for the paper-trading surfaces.
//!
//! One date law for every trade surface. The ledger stamps a row, the
//! report highlights a day and the range filter cuts a window — all three
//! ask this module, so a trade can never sit under one date in the sidebar
//! and another in the calendar.
//!
//! Dates here are *civil* dates in the chart's display timezone, not UTC.
//! A trade printed at 21:30 UTC belongs to the previous day for everyone
//! west of Greenwich, and the ledger already renders its clock that way;
//! a UTC-dated calendar would highlight a day the trader never traded.
//!
//! The module is pure below its one `draw_month` entry point: civil-date
//! conversion, the day index and the selection state machine are plain
//! functions over plain values, which is what makes them testable without
//! a window.

use std::collections::BTreeMap;

use eframe::egui;
use egui_phosphor::regular as icons;
use quantick_sim::ClosedTrade;
use rust_decimal::Decimal;

use crate::paper_chrome::{fmt_signed_points, points_color};
use crate::theme;
use crate::timezone::TzOffset;

// The civil-date law itself moved into `quantick_paper::civil`, where the
// report cuts on it and the journal names its files by it. Re-exported here so
// the ledger, the report window and the harness hooks keep asking this module.
pub(crate) use quantick_paper::civil::{
    CivilDate, DateRange, WEEK_DAYS, fmt_offset_minute, parse_iso_date, weekday_abbr,
};

/// Rows a month grid always paints. Six is the worst case (a 31-day month
/// starting on a Sunday), and painting a fixed six keeps the calendar from
/// resizing the report window as the user pages through months.
const MONTH_GRID_ROWS: usize = 6;

/// Today's civil date on the display clock. The app layer may read a wall
/// clock — the engine and the pure modules may not — and this is the one
/// place it does so for a date: the calendar's fallback when no saved
/// trade names a month to open on.
pub(crate) fn today(tz: TzOffset) -> CivilDate {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| i64::try_from(elapsed.as_millis()).unwrap_or(0))
        .unwrap_or(0);
    CivilDate::from_ms(now_ms, tz)
}

/// `January`…`December`, for the calendar's own header.
fn month_name(month: i64) -> &'static str {
    const NAMES: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    usize::try_from(month - 1)
        .ok()
        .and_then(|index| NAMES.get(index))
        .copied()
        .unwrap_or("???")
}

// ----------------------------------------------------------------------
// The day index
// ----------------------------------------------------------------------

/// What one civil day holds — the calendar cell's whole story.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct DayStat {
    pub(crate) trades: usize,
    pub(crate) wins: usize,
    pub(crate) net: Decimal,
}

/// Which civil days hold trades, and what each one did. Built once per
/// history load and per timezone — never per frame: a calendar repainting
/// at 60 fps must not walk months of history to decide a cell's colour.
#[derive(Debug, Clone, Default)]
pub(crate) struct DayIndex {
    days: BTreeMap<i64, DayStat>,
}

impl DayIndex {
    /// Index every trade by the civil day it *closed* on. Closing time is
    /// the one the ledger, the equity curve and the period filter already
    /// agree on; indexing by open time would put a trade on a day whose
    /// P&L it did not produce.
    /// The index does not keep `tz`: which timezone it was cut with is the
    /// caller's cache key (`report_days_key`), because the caller is what
    /// decides when to rebuild.
    pub(crate) fn build<'a>(trades: impl Iterator<Item = &'a ClosedTrade>, tz: TzOffset) -> Self {
        let mut days: BTreeMap<i64, DayStat> = BTreeMap::new();
        for trade in trades {
            let day = CivilDate::from_ms(trade.closed_ms, tz).day_number();
            let stat = days.entry(day).or_default();
            stat.trades += 1;
            if trade.pnl_points > Decimal::ZERO {
                stat.wins += 1;
            }
            stat.net = stat.net.saturating_add(trade.pnl_points);
        }
        Self { days }
    }

    /// What that day holds, or `None` when it holds nothing.
    pub(crate) fn stat(&self, date: CivilDate) -> Option<DayStat> {
        self.days.get(&date.day_number()).copied()
    }

    /// Oldest day holding a trade.
    pub(crate) fn first(&self) -> Option<CivilDate> {
        self.days
            .keys()
            .next()
            .map(|day| CivilDate::from_day_number(*day))
    }

    /// Newest day holding a trade — where the calendar opens.
    pub(crate) fn last(&self) -> Option<CivilDate> {
        self.days
            .keys()
            .next_back()
            .map(|day| CivilDate::from_day_number(*day))
    }

    /// How many days hold at least one trade.
    pub(crate) fn len(&self) -> usize {
        self.days.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.days.is_empty()
    }
}

// ----------------------------------------------------------------------
// The selection
// ----------------------------------------------------------------------

/// The calendar's click state machine. A first click picks a day and
/// filters to it immediately — a trader asking "what happened on the 12th"
/// should not have to click twice. A second click turns the pick into a
/// span; clicking the picked day again clears, so the calendar is its own
/// undo.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum DaySelection {
    /// Nothing picked — the anchor-relative period pills are in charge.
    #[default]
    None,
    /// One day picked, and filtering to it; a second click extends.
    Anchor(CivilDate),
    /// A closed span.
    Range(DateRange),
}

impl DaySelection {
    /// Apply a click on `day`.
    pub(crate) fn click(self, day: CivilDate) -> Self {
        match self {
            // Clicking the one picked day again is the deselect gesture.
            Self::Anchor(anchor) if anchor == day => Self::None,
            Self::Anchor(anchor) => Self::Range(DateRange::new(anchor, day)),
            // A closed span restarts rather than growing: extending an
            // existing range on click would make the two ends impossible
            // to tell apart, and "start over" is what a second thought is.
            Self::None | Self::Range(_) => Self::Anchor(day),
        }
    }

    /// The span this selection filters on; `None` means "not filtering".
    pub(crate) fn range(self) -> Option<DateRange> {
        match self {
            Self::None => None,
            Self::Anchor(day) => Some(DateRange {
                start: day,
                end: day,
            }),
            Self::Range(range) => Some(range),
        }
    }

    /// Whether `day` sits inside the selection.
    pub(crate) fn contains(self, day: CivilDate) -> bool {
        self.range()
            .is_some_and(|range| day >= range.start && day <= range.end)
    }
}

/// Everything the calendar widget remembers between frames: which month is
/// on screen, what is picked, and whether the panel is expanded at all.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct CalendarState {
    /// Whether the month grid is expanded. Collapsed by default — the
    /// report's existing layout must not shrink for a trader who never
    /// asked for a calendar.
    pub(crate) open: bool,
    /// First day of the month on screen; `None` until the first draw picks
    /// the newest day that holds a trade.
    pub(crate) month: Option<CivilDate>,
    pub(crate) selection: DaySelection,
}

/// Read a harness hook's calendar spec: `1` opens the grid with nothing
/// picked, `YYYY-MM-DD` opens it on one day, and `YYYY-MM-DD..YYYY-MM-DD`
/// on a span. A spec that is none of those is refused rather than guessed
/// — a typo must reach no calendar, never the wrong month.
pub(crate) fn parse_selection(spec: &str) -> Option<DaySelection> {
    let spec = spec.trim();
    if spec == "1" {
        return Some(DaySelection::None);
    }
    match spec.split_once("..") {
        Some((start, end)) => Some(DaySelection::Range(DateRange::new(
            parse_iso_date(start)?,
            parse_iso_date(end)?,
        ))),
        None => Some(DaySelection::Anchor(parse_iso_date(spec)?)),
    }
}

/// What a drawn calendar reports back to its host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CalendarAction {
    /// The selection changed — the view must be recut.
    SelectionChanged,
}

/// The month grid: a header with month paging, a weekday rule, and six
/// rows of day cells. Days holding trades are tinted by their net and
/// carry their trade count; days holding none are inert text, so "which
/// days did I trade" is answered by colour before it is read.
pub(crate) fn draw_month(
    ui: &mut egui::Ui,
    index: &DayIndex,
    state: &mut CalendarState,
    cell: egui::Vec2,
    fallback: CivilDate,
) -> Option<CalendarAction> {
    // `fallback` comes from the caller because this module is deliberately
    // clock-free. Without it an empty index fell back to epoch zero and
    // opened the grid on January 1970 — a month nobody asked about.
    let anchor = state
        .month
        .or_else(|| index.last().map(CivilDate::month_start))
        .unwrap_or_else(|| fallback.month_start());
    state.month = Some(anchor);
    let (year, month, _) = anchor.ymd();
    let mut action = None;

    ui.horizontal(|ui| {
        if ui
            .small_button(icons::CARET_LEFT)
            .on_hover_text("previous month")
            .clicked()
        {
            state.month = Some(anchor.offset_months(-1));
        }
        ui.label(
            egui::RichText::new(format!("{} {year:04}", month_name(month)))
                .color(theme::TEXT_PRIMARY)
                .monospace(),
        );
        if ui
            .small_button(icons::CARET_RIGHT)
            .on_hover_text("next month")
            .clicked()
        {
            state.month = Some(anchor.offset_months(1));
        }
        ui.separator();
        if let Some(newest) = index.last()
            && ui
                .small_button(icons::CLOCK_COUNTER_CLOCKWISE)
                .on_hover_text("jump to the newest day that holds a trade")
                .clicked()
        {
            state.month = Some(newest.month_start());
        }
        // No clear button here: the row above the grid already carries one,
        // and two identical controls a hand-width apart invite the wrong
        // click — the same reason the Source filter does not spell its
        // third option "All".
    });

    // The grid starts on the Monday on or before the 1st, so every month
    // paints the same seven columns under the same seven labels.
    let first_cell = anchor.offset_days(-anchor.weekday());
    let grid_width = cell.x * WEEK_DAYS as f32;
    ui.allocate_ui(
        egui::vec2(grid_width, cell.y * (MONTH_GRID_ROWS as f32 + 1.0)),
        |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
            ui.horizontal(|ui| {
                for weekday in 0..WEEK_DAYS {
                    let (rect, _) = ui.allocate_exact_size(
                        egui::vec2(cell.x, cell.y * 0.7),
                        egui::Sense::hover(),
                    );
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        weekday_abbr(weekday),
                        egui::FontId::monospace(9.0),
                        theme::TEXT_FAINT,
                    );
                }
            });
            for row in 0..MONTH_GRID_ROWS {
                ui.horizontal(|ui| {
                    for column in 0..WEEK_DAYS {
                        let date = first_cell.offset_days(row as i64 * WEEK_DAYS + column);
                        if draw_day_cell(ui, date, anchor, index, state, cell) {
                            state.selection = state.selection.click(date);
                            action = Some(CalendarAction::SelectionChanged);
                        }
                    }
                });
            }
        },
    );
    action
}

/// One day cell; returns whether it was clicked. A cell for a day with no
/// trades is still clickable — picking an empty day is a legitimate
/// question, and the report answers it by saying the day is empty rather
/// than by refusing the click.
fn draw_day_cell(
    ui: &mut egui::Ui,
    date: CivilDate,
    month_anchor: CivilDate,
    index: &DayIndex,
    state: &CalendarState,
    cell: egui::Vec2,
) -> bool {
    let (rect, response) = ui.allocate_exact_size(cell, egui::Sense::click());
    if !ui.is_rect_visible(rect) {
        return response.clicked();
    }
    let in_month = date.same_month(month_anchor);
    let stat = index.stat(date);
    let picked = state.selection.contains(date);
    let body = rect.shrink(1.0);

    if picked {
        ui.painter().rect_filled(
            body,
            egui::Rounding::same(3.0),
            theme::active_tint(theme::ACCENT),
        );
    } else if let Some(stat) = stat {
        // Tinted by the day's outcome: a month's shape is readable before
        // a single number is. A day that netted exactly zero is neither —
        // the same verdict `points_color` gives it everywhere else, and
        // painting a scratch green would be the optimistic lie.
        let base = points_color(stat.net);
        ui.painter().rect_filled(
            body,
            egui::Rounding::same(3.0),
            egui::Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), DAY_TINT_ALPHA),
        );
    } else if response.hovered() {
        ui.painter()
            .rect_filled(body, egui::Rounding::same(3.0), theme::CONTROL);
    }
    if picked || stat.is_some() {
        ui.painter().rect_stroke(
            body,
            egui::Rounding::same(3.0),
            egui::Stroke::new(1.0_f32, if picked { theme::ACCENT } else { theme::BORDER }),
        );
    }

    let (_, _, day) = date.ymd();
    let ink = match (in_month, stat.is_some()) {
        (true, true) => theme::TEXT_PRIMARY,
        (true, false) => theme::TEXT_MUTED,
        (false, _) => theme::TEXT_FAINT,
    };
    ui.painter().text(
        egui::pos2(body.center().x, body.top() + DAY_NUMBER_BASELINE_PX),
        egui::Align2::CENTER_CENTER,
        format!("{day}"),
        egui::FontId::monospace(11.0),
        ink,
    );
    if let Some(stat) = stat {
        ui.painter().text(
            egui::pos2(body.center().x, body.bottom() - DAY_COUNT_BASELINE_PX),
            egui::Align2::CENTER_CENTER,
            format!("{}", stat.trades),
            egui::FontId::monospace(8.0),
            theme::TEXT_FAINT,
        );
    }
    // Built only for the cell under the pointer. Formatting all forty-two
    // every frame was ~2500 allocations a second on the thread that paints
    // the chart, for text at most one of them will ever show.
    let response = response.on_hover_ui(|ui| {
        ui.label(match stat {
            // The same signed-points spelling the ledger and the tiles use
            // — a day cannot net "+12.3456789" here and "+12.35" there.
            Some(stat) => format!(
                "{} · {} trade(s) · {} pts · {} win",
                date.iso(),
                stat.trades,
                fmt_signed_points(stat.net),
                stat.wins,
            ),
            None => format!("{} · no trades", date.iso()),
        });
    });
    response.clicked()
}

/// How strongly a day with trades is washed with its outcome colour.
const DAY_TINT_ALPHA: u8 = 46;
/// Where the day number sits inside its cell.
const DAY_NUMBER_BASELINE_PX: f32 = 11.0;
/// Where the trade count sits, measured up from the cell's bottom.
const DAY_COUNT_BASELINE_PX: f32 = 7.0;

#[cfg(test)]
mod tests {
    use super::*;

    fn utc() -> TzOffset {
        TzOffset::new(0)
    }

    /// The user's own timezone — every off-by-one this module could have
    /// shows up here first.
    fn sao_paulo() -> TzOffset {
        TzOffset::new(-180)
    }

    fn trade(closed_ms: i64, pnl: i64) -> ClosedTrade {
        ClosedTrade {
            side: quantick_engine::Side::Buy,
            quantity: Decimal::ONE,
            entry_price: Decimal::from(100),
            exit_price: Decimal::from(100) + Decimal::from(pnl),
            opened_ms: closed_ms - 1_000,
            closed_ms,
            pnl_points: Decimal::from(pnl),
            exit_reason: quantick_sim::ExitReason::Manual,
            entry_agg_id: None,
            exit_agg_id: None,
            mae_points: None,
            mfe_points: None,
        }
    }

    #[test]
    fn the_day_index_counts_by_the_closing_day_in_the_display_timezone() {
        let day = CivilDate::from_ymd(2026, 8, 17);
        let trades = [
            trade(day.start_ms(sao_paulo()) + 3_600_000, 10),
            trade(day.end_ms(sao_paulo()) - 1, -4),
            // 00:30 local on the next day — a different cell.
            trade(day.end_ms(sao_paulo()) + 1_800_000, 7),
        ];
        let index = DayIndex::build(trades.iter(), sao_paulo());
        assert_eq!(index.len(), 2);
        let stat = index.stat(day).expect("the 17th holds trades");
        assert_eq!(stat.trades, 2);
        assert_eq!(stat.wins, 1);
        assert_eq!(stat.net, Decimal::from(6));
        assert_eq!(index.first(), Some(day));
        assert_eq!(index.last(), Some(day.offset_days(1)));
        assert_eq!(
            index.stat(day.offset_days(-1)),
            None,
            "a quiet day is empty"
        );
    }

    #[test]
    fn the_same_trades_land_on_different_days_under_a_different_timezone() {
        let boundary = CivilDate::from_ymd(2026, 8, 18).start_ms(utc()) + 60 * 60_000;
        let trades = [trade(boundary, 5)];
        assert_eq!(
            DayIndex::build(trades.iter(), utc())
                .first()
                .map(CivilDate::iso),
            Some("2026-08-18".to_owned())
        );
        assert_eq!(
            DayIndex::build(trades.iter(), sao_paulo())
                .first()
                .map(CivilDate::iso),
            Some("2026-08-17".to_owned()),
            "the index must be rebuilt when the display timezone moves"
        );
    }

    #[test]
    fn one_click_picks_a_day_and_filters_to_it() {
        let day = CivilDate::from_ymd(2026, 8, 12);
        let selection = DaySelection::None.click(day);
        assert_eq!(selection, DaySelection::Anchor(day));
        let range = selection.range().expect("one day is still a filter");
        assert_eq!(range.days(), 1);
        assert_eq!(range.label(), "2026-08-12");
    }

    #[test]
    fn a_second_click_makes_a_range_in_either_direction() {
        let earlier = CivilDate::from_ymd(2026, 8, 12);
        let later = CivilDate::from_ymd(2026, 8, 17);
        let forwards = DaySelection::None.click(earlier).click(later);
        let backwards = DaySelection::None.click(later).click(earlier);
        assert_eq!(forwards, backwards, "click order must not change the span");
        let range = forwards.range().expect("a closed span");
        assert_eq!(range.label(), "2026-08-12 to 2026-08-17");
        assert_eq!(range.days(), 6, "both ends counted");
    }

    #[test]
    fn clicking_the_picked_day_again_clears_the_filter() {
        let day = CivilDate::from_ymd(2026, 8, 12);
        assert_eq!(DaySelection::None.click(day).click(day), DaySelection::None);
        assert!(DaySelection::None.click(day).click(day).range().is_none());
    }

    #[test]
    fn clicking_inside_a_closed_range_starts_a_new_pick() {
        let start = CivilDate::from_ymd(2026, 8, 12);
        let end = CivilDate::from_ymd(2026, 8, 17);
        let middle = CivilDate::from_ymd(2026, 8, 14);
        let restarted = DaySelection::None.click(start).click(end).click(middle);
        assert_eq!(restarted, DaySelection::Anchor(middle));
    }

    #[test]
    fn selection_membership_matches_the_range_it_reports() {
        let selection = DaySelection::None
            .click(CivilDate::from_ymd(2026, 8, 12))
            .click(CivilDate::from_ymd(2026, 8, 17));
        assert!(selection.contains(CivilDate::from_ymd(2026, 8, 12)));
        assert!(selection.contains(CivilDate::from_ymd(2026, 8, 14)));
        assert!(selection.contains(CivilDate::from_ymd(2026, 8, 17)));
        assert!(!selection.contains(CivilDate::from_ymd(2026, 8, 11)));
        assert!(!selection.contains(CivilDate::from_ymd(2026, 8, 18)));
    }

    #[test]
    fn a_hook_spec_reaches_a_day_a_span_or_nothing_at_all() {
        assert_eq!(parse_selection("1"), Some(DaySelection::None));
        assert_eq!(
            parse_selection("2026-08-12"),
            Some(DaySelection::Anchor(CivilDate::from_ymd(2026, 8, 12)))
        );
        assert_eq!(
            parse_selection(" 2026-08-12..2026-08-17 "),
            Some(DaySelection::Range(DateRange::new(
                CivilDate::from_ymd(2026, 8, 12),
                CivilDate::from_ymd(2026, 8, 17),
            )))
        );
        // Written backwards it still names the same span.
        assert_eq!(
            parse_selection("2026-08-17..2026-08-12"),
            parse_selection("2026-08-12..2026-08-17")
        );
        for refused in [
            "",
            "0",
            "2026-08",
            "2026-08-12-01",
            "2026-13-01",
            "2026-02-30",
            "2026/08/12",
            "yesterday",
            "2026-08-12..",
            "..2026-08-12",
        ] {
            assert_eq!(
                parse_selection(refused),
                None,
                "{refused:?} must be refused"
            );
        }
    }
}
