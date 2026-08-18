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

use crate::theme;
use crate::timezone::TzOffset;

/// Milliseconds in a civil day. Civil days here are exactly 24 h: the
/// display timezone is a fixed offset (the workspace has no DST table),
/// so there is no shorter or longer day to model — and inventing one
/// would be a guess, not a fact.
pub(crate) const DAY_MS: i64 = 86_400_000;

/// Days in one calendar week.
const WEEK_DAYS: i64 = 7;

/// Rows a month grid always paints. Six is the worst case (a 31-day month
/// starting on a Sunday), and painting a fixed six keeps the calendar from
/// resizing the report window as the user pages through months.
const MONTH_GRID_ROWS: usize = 6;

// ----------------------------------------------------------------------
// Civil dates
// ----------------------------------------------------------------------

/// Civil UTC date-time from epoch milliseconds: `(year, month, day, hour,
/// minute, second)`. Civil-from-days per Howard Hinnant's algorithm; no
/// clock, no chrono.
pub(crate) fn civil_utc(timestamp_ms: i64) -> (i64, i64, i64, i64, i64, i64) {
    let seconds = timestamp_ms.div_euclid(1000);
    let days = seconds.div_euclid(86_400);
    let time_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    (
        year,
        month,
        day,
        time_of_day / 3600,
        (time_of_day % 3600) / 60,
        time_of_day % 60,
    )
}

/// `(year, month, day)` from a day number counted from 1970-01-01 —
/// Hinnant's `civil_from_days`.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let mp = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// The exact inverse of [`civil_from_days`] — Hinnant's `days_from_civil`.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = year - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month_offset = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * month_offset + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// One civil date in the display timezone: the unit the ledger stamps, the
/// calendar paints and the range filter cuts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct CivilDate {
    /// Day number counted from 1970-01-01. Kept as the single field so
    /// ordering, differences and range tests are plain integer work, and
    /// an impossible date (month 13, day 32) cannot be constructed.
    day_number: i64,
}

impl CivilDate {
    /// The civil date a venue timestamp falls on, in the display timezone.
    pub(crate) fn from_ms(timestamp_ms: i64, tz: TzOffset) -> Self {
        let local = timestamp_ms.saturating_add(tz.offset_ms());
        Self {
            day_number: local.div_euclid(DAY_MS),
        }
    }

    /// The date `year-month-day`, normalising out-of-range months and days
    /// the way the civil algorithm does (month 13 is January of the next
    /// year) — the calendar's month paging relies on it.
    pub(crate) fn from_ymd(year: i64, month: i64, day: i64) -> Self {
        Self {
            day_number: days_from_civil(year, month, day),
        }
    }

    /// `(year, month, day)`.
    pub(crate) fn ymd(self) -> (i64, i64, i64) {
        civil_from_days(self.day_number)
    }

    /// Day number from 1970-01-01 — the sort key and the day index's key.
    pub(crate) fn day_number(self) -> i64 {
        self.day_number
    }

    /// This date `count` days later (negative walks back).
    pub(crate) fn offset_days(self, count: i64) -> Self {
        Self {
            day_number: self.day_number.saturating_add(count),
        }
    }

    /// First venue timestamp that belongs to this civil date, inclusive.
    pub(crate) fn start_ms(self, tz: TzOffset) -> i64 {
        self.day_number
            .saturating_mul(DAY_MS)
            .saturating_sub(tz.offset_ms())
    }

    /// First venue timestamp *after* this civil date — the exclusive end,
    /// so a trade printed at 23:59:59.999 local is inside and the next
    /// day's 00:00:00.000 is not.
    pub(crate) fn end_ms(self, tz: TzOffset) -> i64 {
        self.offset_days(1).start_ms(tz)
    }

    /// `2026-08-17` — the unambiguous stamp every surface prints.
    pub(crate) fn iso(self) -> String {
        let (year, month, day) = self.ymd();
        format!("{year:04}-{month:02}-{day:02}")
    }

    /// `17 Aug` — the compact stamp for a row that already sits under a
    /// year-qualified day header. Short on purpose: the characters it does
    /// not spend are characters the exit reason beside it gets to keep.
    pub(crate) fn short(self) -> String {
        let (_, month, day) = self.ymd();
        format!("{day:02} {}", month_abbr(month))
    }

    /// `Mon 17 Aug 2026` — the ledger's day header.
    pub(crate) fn long(self) -> String {
        let (year, month, day) = self.ymd();
        format!(
            "{} {day:02} {} {year:04}",
            weekday_abbr(self.weekday()),
            month_abbr(month)
        )
    }

    /// Weekday, 0 = Monday … 6 = Sunday. 1970-01-01 was a Thursday, which
    /// is why the epoch day number is shifted by three before the modulo.
    pub(crate) fn weekday(self) -> i64 {
        (self.day_number + 3).rem_euclid(WEEK_DAYS)
    }

    /// The first day of this date's month — where a month grid starts.
    pub(crate) fn month_start(self) -> Self {
        let (year, month, _) = self.ymd();
        Self::from_ymd(year, month, 1)
    }

    /// The month `count` months later (negative walks back), clamped to
    /// the first of the month: paging never lands on the 31st of a month
    /// that has 30 days.
    pub(crate) fn offset_months(self, count: i64) -> Self {
        let (year, month, _) = self.ymd();
        let zero_based = (year * 12 + month - 1).saturating_add(count);
        Self::from_ymd(zero_based.div_euclid(12), zero_based.rem_euclid(12) + 1, 1)
    }

    /// Whether this date shares a month with `other`.
    pub(crate) fn same_month(self, other: Self) -> bool {
        let (year, month, _) = self.ymd();
        let (other_year, other_month, _) = other.ymd();
        (year, month) == (other_year, other_month)
    }
}

/// `Jan`…`Dec`; anything outside 1..=12 is a bug upstream, and `???` says
/// so rather than panicking inside a paint.
fn month_abbr(month: i64) -> &'static str {
    const NAMES: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    usize::try_from(month - 1)
        .ok()
        .and_then(|index| NAMES.get(index))
        .copied()
        .unwrap_or("???")
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

/// `Mon`…`Sun` from [`CivilDate::weekday`].
fn weekday_abbr(weekday: i64) -> &'static str {
    const NAMES: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
    usize::try_from(weekday)
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
            .map(|day| CivilDate { day_number: *day })
    }

    /// Newest day holding a trade — where the calendar opens.
    pub(crate) fn last(&self) -> Option<CivilDate> {
        self.days
            .keys()
            .next_back()
            .map(|day| CivilDate { day_number: *day })
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

/// An inclusive span of civil days.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DateRange {
    pub(crate) start: CivilDate,
    pub(crate) end: CivilDate,
}

impl DateRange {
    /// The span between two days, in either click order.
    pub(crate) fn new(a: CivilDate, b: CivilDate) -> Self {
        if a <= b {
            Self { start: a, end: b }
        } else {
            Self { start: b, end: a }
        }
    }

    /// Whether a venue timestamp falls inside — start inclusive, end
    /// inclusive to the last millisecond of its civil day.
    pub(crate) fn contains_ms(self, timestamp_ms: i64, tz: TzOffset) -> bool {
        timestamp_ms >= self.start.start_ms(tz) && timestamp_ms < self.end.end_ms(tz)
    }

    /// How many civil days the span covers, both ends counted.
    pub(crate) fn days(self) -> i64 {
        self.end.day_number() - self.start.day_number() + 1
    }

    /// `2026-08-17` for a single day, `2026-08-12 to 2026-08-17` for a
    /// span — what the report's support line reads out loud.
    ///
    /// Spelled with a word, not an arrow: this string is rendered in the
    /// proportional UI font, whose fallback has no `→` and draws a tofu
    /// box instead. The arrow survives where the text is monospace (the
    /// trade list's `ENTRY → EXIT`, a ledger row's round trip); here it
    /// would be a missing glyph in the one label naming the filter.
    pub(crate) fn label(self) -> String {
        if self.start == self.end {
            self.start.iso()
        } else {
            format!("{} to {}", self.start.iso(), self.end.iso())
        }
    }
}

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

/// Read a `YYYY-MM-DD` date, refusing anything that is not one. The
/// parse must round-trip: `2026-02-30` normalises to March 2nd inside the
/// civil algorithm, and silently answering a question nobody asked is
/// exactly the guess this refuses to make.
pub(crate) fn parse_iso_date(text: &str) -> Option<CivilDate> {
    let mut parts = text.trim().split('-');
    let year: i64 = parts.next()?.parse().ok()?;
    let month: i64 = parts.next()?.parse().ok()?;
    let day: i64 = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    let date = CivilDate::from_ymd(year, month, day);
    (date.ymd() == (year, month, day)).then_some(date)
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
        let base = crate::paper_trading::points_color(stat.net);
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
                crate::paper_trading::fmt_signed_points(stat.net),
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
    fn civil_days_round_trip_through_the_epoch_and_back() {
        for day in [-100_000_i64, -1, 0, 1, 19_952, 100_000] {
            let (year, month, date) = civil_from_days(day);
            assert_eq!(
                days_from_civil(year, month, date),
                day,
                "{year:04}-{month:02}-{date:02} must come back to day {day}"
            );
        }
    }

    #[test]
    fn a_date_knows_its_own_midnight_bounds() {
        let date = CivilDate::from_ymd(2026, 8, 17);
        assert_eq!(date.iso(), "2026-08-17");
        assert_eq!(date.end_ms(utc()) - date.start_ms(utc()), DAY_MS);
        // The last millisecond of the day is inside; the next is not.
        let range = DateRange {
            start: date,
            end: date,
        };
        assert!(range.contains_ms(date.end_ms(utc()) - 1, utc()));
        assert!(!range.contains_ms(date.end_ms(utc()), utc()));
        assert!(range.contains_ms(date.start_ms(utc()), utc()));
        assert!(!range.contains_ms(date.start_ms(utc()) - 1, utc()));
    }

    #[test]
    fn a_late_utc_print_belongs_to_the_previous_day_in_sao_paulo() {
        // 2026-08-18T01:30:00Z is 2026-08-17T22:30 local at UTC-03:00.
        let timestamp = CivilDate::from_ymd(2026, 8, 18).start_ms(utc()) + 90 * 60_000;
        assert_eq!(CivilDate::from_ms(timestamp, utc()).iso(), "2026-08-18");
        assert_eq!(
            CivilDate::from_ms(timestamp, sao_paulo()).iso(),
            "2026-08-17",
            "the calendar must highlight the day the trader saw on the clock"
        );
    }

    #[test]
    fn month_paging_never_lands_on_a_day_the_month_does_not_have() {
        let january_31 = CivilDate::from_ymd(2026, 1, 31);
        assert_eq!(january_31.offset_months(1).iso(), "2026-02-01");
        assert_eq!(january_31.offset_months(-1).iso(), "2025-12-01");
        assert_eq!(january_31.offset_months(12).iso(), "2027-01-01");
        assert_eq!(january_31.offset_months(-13).iso(), "2024-12-01");
    }

    #[test]
    fn weekdays_start_on_monday() {
        // 2026-08-17 is a Monday.
        assert_eq!(CivilDate::from_ymd(2026, 8, 17).weekday(), 0);
        assert_eq!(CivilDate::from_ymd(2026, 8, 23).weekday(), 6);
        assert_eq!(CivilDate::from_ymd(1970, 1, 1).weekday(), 3, "a Thursday");
        assert_eq!(CivilDate::from_ymd(1969, 12, 28).weekday(), 6, "a Sunday");
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
    fn a_range_contains_every_millisecond_of_both_end_days() {
        let range = DateRange::new(
            CivilDate::from_ymd(2026, 8, 12),
            CivilDate::from_ymd(2026, 8, 17),
        );
        assert!(range.contains_ms(range.start.start_ms(sao_paulo()), sao_paulo()));
        assert!(range.contains_ms(range.end.end_ms(sao_paulo()) - 1, sao_paulo()));
        assert!(!range.contains_ms(range.start.start_ms(sao_paulo()) - 1, sao_paulo()));
        assert!(!range.contains_ms(range.end.end_ms(sao_paulo()), sao_paulo()));
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

    #[test]
    fn dates_print_the_way_every_surface_reads_them() {
        let date = CivilDate::from_ymd(2026, 8, 17);
        assert_eq!(date.iso(), "2026-08-17");
        assert_eq!(date.short(), "17 Aug");
        assert_eq!(date.long(), "Mon 17 Aug 2026");
    }
}
