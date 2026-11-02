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

use eframe::egui;
use egui_phosphor::regular as icons;

use crate::paper_chrome::{fmt_signed_points, points_color};
use crate::theme;
use crate::timezone::TzOffset;

// The civil-date law itself moved into `quantick-civil`, where the
// report cuts on it and the journal names its files by it. Re-exported here so
// the ledger, the report window and the harness hooks keep asking this module.
#[cfg(feature = "scenario-harness")]
pub(crate) use quantick_civil::parse_iso_date;
pub(crate) use quantick_civil::{CivilDate, DateRange, WEEK_DAYS, fmt_offset_minute, weekday_abbr};

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

#[cfg(any(feature = "scenario-harness", test))]
pub(crate) use quantick_paper::report::calendar::parse_selection;
pub(crate) use quantick_paper::report::calendar::{
    CalendarState, DayIndex, DaySelection, month_name,
};

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
    let palette = theme::get(ui.ctx());
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
                .color(palette.text_primary())
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
                        palette.text_faint(),
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
    let palette = theme::get(ui.ctx());
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
            theme::active_tint(palette.accent()),
        );
    } else if let Some(stat) = stat {
        // Tinted by the day's outcome: a month's shape is readable before
        // a single number is. A day that netted exactly zero is neither —
        // the same verdict `points_color` gives it everywhere else, and
        // painting a scratch green would be the optimistic lie.
        let base = points_color(&palette, stat.net);
        ui.painter().rect_filled(
            body,
            egui::Rounding::same(3.0),
            egui::Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), DAY_TINT_ALPHA),
        );
    } else if response.hovered() {
        ui.painter()
            .rect_filled(body, egui::Rounding::same(3.0), palette.control());
    }
    if picked || stat.is_some() {
        ui.painter().rect_stroke(
            body,
            egui::Rounding::same(3.0),
            egui::Stroke::new(
                1.0_f32,
                if picked {
                    palette.accent()
                } else {
                    palette.border()
                },
            ),
        );
    }

    let (_, _, day) = date.ymd();
    let ink = match (in_month, stat.is_some()) {
        (true, true) => palette.text_primary(),
        (true, false) => palette.text_muted(),
        (false, _) => palette.text_faint(),
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
            palette.text_faint(),
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
