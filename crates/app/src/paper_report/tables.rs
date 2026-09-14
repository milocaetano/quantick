//! Everything in the report window that is a number in a box.
//!
//! The three headline tiles, the trade list and the three grids. They share
//! one property worth the module: each takes a finished
//! [`PerformanceReport`] or [`ReportView`] and paints it, so none of them can
//! change a number - an auditor comparing the window against the journal
//! reads the report's arithmetic elsewhere and only the layout here.

use eframe::egui;
use egui_phosphor::regular as icons;
use quantick_sim::{PerformanceReport, SideReport, history};
use rust_decimal::Decimal;

use super::rows::elide_tail;
use super::{HEADLINE_FONT_PX, REPORT_LIST_MAX_H_PX, ReportView, TILE_GUTTER_PX, TILE_HEIGHT_PX};
use crate::paper_calendar::CivilDate;
use crate::paper_chrome::{
    caption, fmt_decimal, fmt_duration_ms, fmt_points, fmt_signed_points, points_color,
    position_word,
};
use crate::theme;

/// The three headline tiles: NET (the one coloured number in the window),
/// WIN RATE, PROFIT FACTOR — each with its denominator under it, so every
/// tile is a self-explaining fact rather than a floating statistic.
pub(super) fn draw_report_tiles(ui: &mut egui::Ui, report: &PerformanceReport) {
    let width = ((ui.available_width() - 2.0 * TILE_GUTTER_PX) / 3.0).max(80.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = TILE_GUTTER_PX;
        draw_tile(
            ui,
            width,
            "NET",
            fmt_signed_points(report.net_points),
            "pts",
            points_color(report.net_points),
            format!("{} trades", report.trades),
        );
        draw_tile(
            ui,
            width,
            "WIN RATE",
            report
                .win_rate_pct
                .map_or_else(|| "—".to_owned(), |rate| fmt_decimal(rate.round_dp(0))),
            "%",
            theme::TEXT_PRIMARY,
            format!(
                "{} W · {} L · {} scratch",
                report.wins, report.losses, report.scratches
            ),
        );
        draw_tile(
            ui,
            width,
            "PROFIT FACTOR",
            report
                .profit_factor
                .map_or_else(|| "—".to_owned(), fmt_points),
            "",
            theme::TEXT_PRIMARY,
            format!(
                "+{} / -{}",
                fmt_points(report.gross_profit),
                fmt_points(report.gross_loss)
            ),
        );
    });
}

/// One headline tile: caption, hero value with its unit, denominator line.
fn draw_tile(
    ui: &mut egui::Ui,
    width: f32,
    label: &str,
    value: String,
    unit: &str,
    value_color: egui::Color32,
    subline: String,
) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, TILE_HEIGHT_PX), egui::Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let painter = ui.painter();
    painter.rect_filled(rect, egui::Rounding::same(4.0), theme::INSET);
    painter.rect_stroke(
        rect,
        egui::Rounding::same(4.0),
        egui::Stroke::new(1.0_f32, theme::BORDER),
    );
    painter.text(
        rect.min + egui::vec2(12.0, 6.0),
        egui::Align2::LEFT_TOP,
        label,
        egui::FontId::monospace(10.0),
        theme::TEXT_FAINT,
    );
    let value_galley = painter.layout_no_wrap(
        value,
        egui::FontId::monospace(HEADLINE_FONT_PX),
        value_color,
    );
    let value_w = value_galley.size().x;
    let baseline = rect.top() + 38.0;
    painter.galley(
        egui::pos2(rect.left() + 12.0, baseline - value_galley.size().y),
        value_galley,
        value_color,
    );
    if !unit.is_empty() {
        painter.text(
            egui::pos2(rect.left() + 12.0 + value_w + 4.0, baseline - 2.0),
            egui::Align2::LEFT_BOTTOM,
            unit,
            egui::FontId::monospace(11.0),
            theme::TEXT_MUTED,
        );
    }
    painter.text(
        egui::pos2(rect.left() + 12.0, rect.bottom() - 6.0),
        egui::Align2::LEFT_BOTTOM,
        subline,
        egui::FontId::monospace(10.0),
        theme::TEXT_FAINT,
    );
}

/// The trade list's columns: caption and width in pixels, in paint order.
/// One table, so the header row and every trade row can only ever agree
/// about where a column begins.
const TRADE_LIST_COLUMNS: [(&str, f32); 11] = [
    ("#", 38.0),
    ("DATE", 70.0),
    ("TIME", 58.0),
    ("SYMBOL", 66.0),
    ("SIDE", 56.0),
    ("ENTRY → EXIT", 122.0),
    ("HELD", 52.0),
    ("EXIT", 74.0),
    ("PTS", 52.0),
    ("EQUITY", 60.0),
    // Not a hover: under Source "Both" a practice trade and a real one
    // are otherwise the same row, and a replay result readable as a real
    // one is the worst thing this window could do.
    ("SOURCE", 58.0),
];

/// One trade-list row's height. Tight on purpose: this is a table to scan,
/// not a list to browse.
const REPORT_LIST_ROW_H_PX: f32 = 17.0;

/// Left padding inside a trade-list cell.
const TRADE_LIST_CELL_PAD_PX: f32 = 4.0;

/// The trade list's full width — the sum of its columns.
fn trade_list_width() -> f32 {
    TRADE_LIST_COLUMNS.iter().map(|(_, width)| width).sum()
}

/// Paint one row of cells on the shared column grid, each cut to its own
/// column rather than allowed to run into the next one.
fn paint_list_row(
    painter: &egui::Painter,
    rect: egui::Rect,
    glyph_w: f32,
    cells: &[(String, egui::Color32)],
) {
    let font = egui::FontId::monospace(10.0);
    let mut x = rect.left();
    for ((text, color), (_, width)) in cells.iter().zip(TRADE_LIST_COLUMNS) {
        let budget = ((width - 2.0 * TRADE_LIST_CELL_PAD_PX) / glyph_w)
            .floor()
            .max(0.0) as usize;
        painter.text(
            egui::pos2(x + TRADE_LIST_CELL_PAD_PX, rect.center().y),
            egui::Align2::LEFT_CENTER,
            elide_tail(text, budget),
            font.clone(),
            *color,
        );
        x += width;
    }
}

/// The trades behind the curve: every trade the filters kept, in closing
/// order, each naming its date, instrument, side, round trip, result and
/// why it ended. A performance screen without this is a shape with no
/// story — "I see the chart and the number, but not which trades those
/// were" is exactly the gap it closes.
///
/// Virtualised: the list holds whatever the filters kept — an "All" window
/// can be thousands of trades — and only the rows on screen are ever laid
/// out. Returns whether the section's collapse control was clicked.
pub(super) fn draw_trade_list(ui: &mut egui::Ui, view: &ReportView, open: bool) -> bool {
    let mut toggled = false;
    ui.horizontal(|ui| {
        ui.label(caption("TRADES BEHIND THIS CURVE"));
        ui.label(
            egui::RichText::new(format!("{}", view.rows.len()))
                .color(theme::TEXT_MUTED)
                .small(),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let icon = if open {
                icons::CARET_UP
            } else {
                icons::CARET_DOWN
            };
            if ui
                .small_button(icon)
                .on_hover_text(if open {
                    "collapse the trade list"
                } else {
                    "list every trade in this window"
                })
                .clicked()
            {
                toggled = true;
            }
        });
    });
    if !open {
        return toggled;
    }
    let width = trade_list_width();
    let glyph_w = ui
        .painter()
        .layout_no_wrap(
            "0".to_owned(),
            egui::FontId::monospace(10.0),
            theme::TEXT_MUTED,
        )
        .size()
        .x
        .max(1.0);
    // Wide by nature — ten columns of facts — and long by nature too. It
    // scrolls both ways inside a bounded strip rather than pushing the
    // window wider, clipping a number in half, or growing until the metric
    // grids beneath it are out of reach. Row zero is the header, so it
    // rides the same column grid and the same horizontal scroll.
    egui::ScrollArea::both()
        .id_salt("paper_report_trade_list")
        .max_height(REPORT_LIST_MAX_H_PX)
        // Vertically it shrinks to its content: a five-trade window must
        // not reserve the whole strip and push the grids off the screen.
        .auto_shrink([false, true])
        .show_rows(
            ui,
            REPORT_LIST_ROW_H_PX,
            view.rows.len() + 1,
            |ui, range| {
                for index in range {
                    let (rect, response) = ui.allocate_exact_size(
                        egui::vec2(width, REPORT_LIST_ROW_H_PX),
                        egui::Sense::hover(),
                    );
                    if !ui.is_rect_visible(rect) {
                        continue;
                    }
                    if index == 0 {
                        let cells: Vec<(String, egui::Color32)> = TRADE_LIST_COLUMNS
                            .iter()
                            .map(|(label, _)| ((*label).to_owned(), theme::TEXT_FAINT))
                            .collect();
                        paint_list_row(ui.painter(), rect, glyph_w, &cells);
                        ui.painter().line_segment(
                            [
                                egui::pos2(rect.left(), rect.bottom()),
                                egui::pos2(rect.right(), rect.bottom()),
                            ],
                            egui::Stroke::new(1.0_f32, theme::BORDER),
                        );
                        continue;
                    }
                    let ordinal = index - 1;
                    let row = &view.rows[ordinal];
                    let trade = &row.trade;
                    // The running total was cut with the view, so a row
                    // deep in the list costs no more than the first.
                    let equity = view
                        .equity
                        .points
                        .get(index)
                        .copied()
                        .unwrap_or(Decimal::ZERO);
                    if response.hovered() {
                        ui.painter()
                            .rect_filled(rect, egui::Rounding::ZERO, theme::BORDER);
                    } else if ordinal % 2 == 1 {
                        ui.painter()
                            .rect_filled(rect, egui::Rounding::ZERO, theme::INSET);
                    }
                    let muted = theme::TEXT_MUTED;
                    let cells = [
                        (format!("{}", ordinal + 1), theme::TEXT_FAINT),
                        (
                            CivilDate::from_ms(trade.closed_ms, view.tz).iso(),
                            theme::TEXT_PRIMARY,
                        ),
                        (crate::plot_area::fmt_time(trade.closed_ms, view.tz), muted),
                        (row.symbol.clone(), theme::TEXT_PRIMARY),
                        (
                            format!(
                                "{} {}",
                                position_word(trade.side),
                                fmt_decimal(trade.quantity)
                            ),
                            theme::side_color(trade.side),
                        ),
                        (
                            format!(
                                "{} → {}",
                                fmt_decimal(trade.entry_price),
                                fmt_decimal(trade.exit_price)
                            ),
                            muted,
                        ),
                        (
                            fmt_duration_ms(trade.closed_ms.saturating_sub(trade.opened_ms)),
                            muted,
                        ),
                        (trade.exit_reason.as_str().replace('_', " "), muted),
                        (
                            fmt_signed_points(trade.pnl_points),
                            points_color(trade.pnl_points),
                        ),
                        (fmt_signed_points(equity), points_color(equity)),
                        match row.source {
                            Some(history::SessionSource::Live) => {
                                ("live".to_owned(), theme::TEXT_FAINT)
                            }
                            Some(history::SessionSource::Replay) => {
                                ("replay".to_owned(), theme::WARN)
                            }
                            // Unrecorded is not "live": a file from before
                            // the source line existed says so with a mark
                            // that reads as absence, never as a fact.
                            None => ("—".to_owned(), theme::TEXT_FAINT),
                        },
                    ];
                    paint_list_row(ui.painter(), rect, glyph_w, &cells);
                    // Every fact the columns cannot hold whole, on hover -
                    // including the session source, which is never guessed.
                    // Spelled with words, not an arrow: a hover card is
                    // drawn in the proportional UI font, which has no glyph
                    // for → and paints a tofu box. The arrow survives in the
                    // painted row above, which is monospace.
                    response.on_hover_text(format!(
                        "#{} · {} {} · {} to {} · {} pts · {} · held {} · {}",
                        ordinal + 1,
                        position_word(trade.side),
                        fmt_decimal(trade.quantity),
                        fmt_decimal(trade.entry_price),
                        fmt_decimal(trade.exit_price),
                        fmt_signed_points(trade.pnl_points),
                        trade.exit_reason.as_str().replace('_', " "),
                        fmt_duration_ms(trade.closed_ms.saturating_sub(trade.opened_ms)),
                        match row.source {
                            Some(source) => format!("{} session", source.as_str()),
                            None => "saved before quantick recorded a session source".to_owned(),
                        },
                    ));
                }
            },
        );
    ui.add_space(6.0);
    toggled
}

/// The metric grid: one metric per row, the explanation on hover, honest
/// blanks (`—`) where a ratio has no denominator. Every value here has a
/// structurally fixed sign, so none of them is coloured.
pub(super) fn draw_report_grid(ui: &mut egui::Ui, report: &PerformanceReport) {
    let blank = || "—".to_owned();
    let pts = |value: Decimal| format!("{} pts", fmt_points(value));
    let opt_pts = |value: Option<Decimal>| {
        value.map_or_else(blank, |value| format!("{} pts", fmt_points(value)))
    };
    let opt_plain = |value: Option<Decimal>| value.map_or_else(blank, fmt_points);
    let duration = |value: Option<i64>| value.map_or_else(blank, fmt_duration_ms);
    egui::Grid::new("paper_report_grid")
        .num_columns(2)
        .spacing([16.0, 4.0])
        .show(ui, |ui| {
            let mut row = |label: &str, value: String, explain: &str| {
                ui.label(egui::RichText::new(label).color(theme::TEXT_MUTED))
                    .on_hover_text(explain.to_owned());
                ui.label(value);
                ui.end_row();
            };
            row(
                "trades",
                format!(
                    "{} ({} long / {} short)",
                    report.trades, report.long_trades, report.short_trades
                ),
                "closed round trips under the current filter",
            );
            row(
                "max drawdown",
                pts(report.max_drawdown_points),
                "deepest drop of realized equity below its running peak, in closing order",
            );
            row(
                "max run-up",
                pts(report.max_runup_points),
                "highest rise of realized equity above its running trough — the drawdown's mirror",
            );
            row(
                "recovery factor",
                opt_plain(report.recovery_factor),
                "net profit divided by the max drawdown; blank while nothing drew down",
            );
            row(
                "gross profit / loss",
                format!(
                    "{} / {} pts",
                    fmt_points(report.gross_profit),
                    fmt_points(report.gross_loss)
                ),
                "winners' points and losers' magnitude, before netting",
            );
            row(
                "avg win / loss",
                format!(
                    "{} / {} pts",
                    opt_plain(report.avg_win),
                    opt_plain(report.avg_loss),
                ),
                "mean winner and mean loser magnitude",
            );
            row(
                "payoff ratio",
                opt_plain(report.payoff_ratio),
                "average win divided by average loss — how much a winner pays for a loser",
            );
            row(
                "expectancy",
                report
                    .expectancy_points
                    .map_or_else(blank, |value| format!("{} pts/trade", fmt_points(value))),
                "net profit per trade: what one average trade pays",
            );
            row(
                "std deviation",
                opt_pts(report.stddev_points),
                "sample standard deviation of trade points (N−1); blank below two trades",
            );
            row(
                "longest streaks",
                format!(
                    "{} wins / {} losses",
                    report.max_consecutive_wins, report.max_consecutive_losses
                ),
                "longest consecutive runs, in closing order; a scratch breaks both",
            );
            row(
                "avg / median duration",
                format!(
                    "{} / {}",
                    duration(report.avg_duration_ms),
                    duration(report.median_duration_ms),
                ),
                "trade lifetimes in venue time",
            );
            row(
                "winner vs loser duration",
                format!(
                    "{} / {}",
                    duration(report.avg_win_duration_ms),
                    duration(report.avg_loss_duration_ms),
                ),
                "how long winners run against how long losers are held",
            );
            row(
                "largest win / loss",
                format!(
                    "{} / {} pts",
                    fmt_points(report.largest_win),
                    fmt_points(report.largest_loss)
                ),
                "best single trade and worst single trade magnitude",
            );
            row(
                "avg winner MAE",
                report.avg_winner_mae_points.map_or_else(blank, |value| {
                    format!(
                        "{} pts (over {} of {})",
                        fmt_points(value),
                        report.winners_with_mae,
                        report.wins
                    )
                }),
                "how far the average winner first ran against you; only trades that \
                 recorded an excursion count, and the denominator says how many did",
            );
            row(
                "avg loser MFE",
                report.avg_loser_mfe_points.map_or_else(blank, |value| {
                    format!(
                        "{} pts (over {} of {})",
                        fmt_points(value),
                        report.losers_with_mfe,
                        report.losses
                    )
                }),
                "how far the average loser was in profit before it lost; same disclosure",
            );
        });
}

/// Long and short, side by side, with the core metrics in each column.
pub(super) fn draw_side_grid(ui: &mut egui::Ui, report: &PerformanceReport) {
    ui.add_space(8.0);
    ui.label(caption("LONG VS SHORT"));
    fn opt_plain(value: Option<Decimal>) -> String {
        value.map_or_else(|| "—".to_owned(), fmt_points)
    }
    /// One side-by-side row's value, computed per column.
    type SideValue = Box<dyn Fn(&SideReport) -> String>;
    let side_rows: [(&str, SideValue); 7] = [
        ("trades", Box::new(|side| side.trades.to_string())),
        (
            "net",
            Box::new(|side| format!("{} pts", fmt_signed_points(side.net_points))),
        ),
        (
            "win rate",
            Box::new(|side| {
                side.win_rate_pct
                    .map_or_else(|| "—".to_owned(), |rate| format!("{}%", fmt_points(rate)))
            }),
        ),
        (
            "profit factor",
            Box::new(|side| opt_plain(side.profit_factor)),
        ),
        ("avg win", Box::new(|side| opt_plain(side.avg_win))),
        ("avg loss", Box::new(|side| opt_plain(side.avg_loss))),
        (
            "expectancy",
            Box::new(|side| opt_plain(side.expectancy_points)),
        ),
    ];
    egui::Grid::new("paper_report_sides")
        .num_columns(3)
        .spacing([16.0, 4.0])
        .show(ui, |ui| {
            ui.label("");
            ui.label(egui::RichText::new("LONG").color(theme::BUY).small());
            ui.label(egui::RichText::new("SHORT").color(theme::SELL).small());
            ui.end_row();
            for (label, value_of) in &side_rows {
                ui.label(egui::RichText::new(*label).color(theme::TEXT_MUTED));
                ui.label(value_of(&report.long));
                ui.label(value_of(&report.short));
                ui.end_row();
            }
        });
}

/// How trades that left one way performed — count and net per exit reason.
pub(super) fn draw_exit_reason_grid(ui: &mut egui::Ui, report: &PerformanceReport) {
    if report.by_exit_reason.is_empty() {
        return;
    }
    ui.add_space(8.0);
    ui.label(caption("BY EXIT REASON"));
    egui::Grid::new("paper_report_reasons")
        .num_columns(3)
        .spacing([16.0, 4.0])
        .show(ui, |ui| {
            for reason in &report.by_exit_reason {
                ui.label(
                    egui::RichText::new(reason.reason.as_str().replace('_', " "))
                        .color(theme::TEXT_MUTED),
                );
                ui.label(format!("{} trade(s)", reason.trades));
                ui.label(format!("{} pts", fmt_signed_points(reason.net_points)));
                ui.end_row();
            }
        });
}
