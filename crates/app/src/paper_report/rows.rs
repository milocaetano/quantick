//! One row of the ledger, painted.
//!
//! A day header, a group header, an open position, a closed trade, and the
//! "N more" row that reveals the next page. Each is a painter call and a hit
//! test, with no state of its own - what to draw arrives as an argument, and
//! what the pointer did leaves as a return value.

use eframe::egui;
use egui_phosphor::regular as icons;
use quantick_engine::Side;
use quantick_sim::ClosedTrade;
use rust_decimal::Decimal;

use super::sessions::source_glyph;
use super::{LEDGER_PAGE_TRADES, LEDGER_ROW_HEIGHT_PX, OpenRow, SIDE_RAIL_WIDTH_PX};
use crate::paper_calendar::CivilDate;
use crate::paper_chrome::{
    financial_text, fmt_decimal, fmt_duration_ms, points_color, position_word,
};
use crate::theme;
use crate::timezone::TzOffset;

pub(super) use quantick_paper::report::ledger_rows::{
    GroupView, Item, LedgerRow, TradeMeta, push_groups,
};

/// What one painted ledger row reported back.
#[derive(Default)]
pub(super) struct LedgerRowResponse {
    pub(super) clicked: bool,
    pub(super) navigate: bool,
    /// The row's own response, for its context menu.
    pub(super) response: Option<egui::Response>,
}

/// A group caption inside the ledger, before its measured trade rows.
pub(super) fn draw_group_header(ui: &mut egui::Ui, label: &str, count: usize) {
    let palette = theme::get(ui.ctx());
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(width, palette.layout(LEDGER_ROW_HEIGHT_PX)),
        egui::Sense::hover(),
    );
    if !ui.is_rect_visible(rect) {
        return;
    }
    ui.painter().text(
        egui::pos2(rect.left() + 2.0, rect.bottom() - 4.0),
        egui::Align2::LEFT_BOTTOM,
        format!("{label} · {count}"),
        palette.mono_font(10.0),
        palette.text_faint(),
    );
}

/// A folding civil-day caption followed by its complete count and net.
/// The measured height lets the date and calculation wrap independently.
pub(super) fn draw_day_header(
    ui: &mut egui::Ui,
    date: CivilDate,
    count: usize,
    net: &(Decimal, String),
    folded: bool,
) -> bool {
    let palette = theme::get(ui.ctx());
    let mut job = egui::text::LayoutJob::default();
    for (text, font_id, color) in [
        (
            format!(
                "{} ",
                if folded {
                    icons::CARET_RIGHT
                } else {
                    icons::CARET_DOWN
                }
            ),
            palette.body_font(10.0),
            palette.text_faint(),
        ),
        (
            format!("{}\n", date.long()),
            palette.mono_font(10.0),
            if folded {
                palette.text_faint()
            } else {
                palette.text_muted()
            },
        ),
    ] {
        job.append(
            &text,
            0.0,
            egui::TextFormat {
                font_id,
                color,
                ..Default::default()
            },
        );
    }
    let numbers = financial_text(
        &palette,
        &format!("{count} \u{00b7} {}", net.1),
        palette.mono_font(10.0),
        points_color(&palette, net.0),
    );
    for section in numbers.sections {
        job.append(&numbers.text[section.byte_range], 0.0, section.format);
    }
    job.break_on_newline = true;
    job.wrap.max_rows = usize::MAX;
    job.wrap.max_width = (ui.available_width() - 12.0).max(1.0);
    let galley = ui.painter().layout_job(job);
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), galley.size().y + 12.0),
        egui::Sense::click(),
    );
    if ui.is_rect_visible(rect) {
        ui.painter().rect_filled(
            rect,
            egui::Rounding::ZERO,
            if response.hovered() {
                palette.border()
            } else {
                palette.inset()
            },
        );
        ui.painter().galley(
            rect.min + egui::vec2(6.0, 6.0),
            galley,
            palette.text_primary(),
        );
    }
    response
        .on_hover_text(format!(
            "{} · {count} trade(s) closed · {} on the day - click to {}",
            date.iso(),
            net.1,
            if folded { "open it" } else { "fold it shut" },
        ))
        .clicked()
}

/// The "show older" control at the foot of the saved history: it names how
/// many trades are still held back, so the end of the list is never
/// mistaken for the end of the history.
pub(super) fn draw_more_row(ui: &mut egui::Ui, remaining: usize) -> bool {
    let palette = theme::get(ui.ctx());
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(width, palette.layout(LEDGER_ROW_HEIGHT_PX)),
        egui::Sense::click(),
    );
    if !ui.is_rect_visible(rect) {
        return response.clicked();
    }
    let body = rect.shrink2(egui::vec2(6.0, 4.0));
    ui.painter().rect_filled(
        body,
        egui::Rounding::same(4.0),
        if response.hovered() {
            palette.border()
        } else {
            palette.control()
        },
    );
    ui.painter().text(
        body.center(),
        egui::Align2::CENTER_CENTER,
        format!("{}  show older · {remaining} more saved", icons::CARET_DOWN),
        palette.mono_font(10.0),
        palette.text_muted(),
    );
    response
        .on_hover_text(format!(
            "reveal the next {LEDGER_PAGE_TRADES} saved trade(s) - {remaining} still held back"
        ))
        .clicked()
}

/// The pinned open-position row uses the current mark as its provisional exit.
pub(super) fn draw_open_row(ui: &mut egui::Ui, open: &OpenRow, symbol: Option<&str>) {
    let summary = &open.summary;
    let exit = open.mark_price.map_or_else(|| "…".to_owned(), fmt_decimal);
    let held = open
        .held_ms
        .map_or_else(String::new, |ms| format!("open {}", fmt_duration_ms(ms)));
    let job = trade_text(
        ui,
        summary.side,
        (
            &format!(
                "{} {}",
                position_word(summary.side),
                fmt_decimal(summary.quantity)
            ),
            &format!("{} → {}", fmt_decimal(summary.avg_price), exit),
        ),
        summary.open_points.zip(open.pnl.as_deref()),
        &format!("{held} · at the last print"),
        symbol,
        None,
    );
    paint_trade(ui, job, summary.side, false, egui::Sense::hover());
}

/// Lay out the complete facts before allocating the row. Wrapped prices and
/// formulas retain the same font size and give the next row their real height.
fn trade_text(
    ui: &egui::Ui,
    side: Side,
    (head, route): (&str, &str),
    points: Option<(Decimal, &str)>,
    detail: &str,
    symbol: Option<&str>,
    marker: Option<(&str, egui::Color32)>,
) -> egui::text::LayoutJob {
    let palette = theme::get(ui.ctx());
    let font = palette.mono_font(11.0);
    let mut job = egui::text::LayoutJob::default();
    let mut append = |text: &str, color| {
        job.append(
            text,
            0.0,
            egui::TextFormat {
                font_id: font.clone(),
                color,
                ..Default::default()
            },
        )
    };
    append(
        head,
        if palette.resolved.dark {
            palette.side_color(side)
        } else {
            palette.text_primary()
        },
    );
    append(
        &format!("  {route}"),
        if palette.resolved.dark {
            palette.text_muted()
        } else {
            palette.text_primary()
        },
    );
    if let Some(symbol) = symbol {
        append(
            &format!("  {symbol}"),
            if palette.resolved.dark {
                palette.text_faint()
            } else {
                palette.text_muted()
            },
        );
    }
    if let Some((glyph, color)) = marker {
        append(&format!("  {glyph}"), color);
    }
    if let Some((value, text)) = points {
        let numeric = financial_text(
            &palette,
            &format!("\n{text}"),
            font.clone(),
            points_color(&palette, value),
        );
        for section in numeric.sections {
            job.append(&numeric.text[section.byte_range], 0.0, section.format);
        }
    }
    job.append(
        &format!("\n{detail}"),
        0.0,
        egui::TextFormat {
            font_id: palette.mono_font(10.0),
            color: if palette.resolved.dark {
                palette.text_faint()
            } else {
                palette.text_muted()
            },
            ..Default::default()
        },
    );
    job.break_on_newline = true;
    job.wrap.max_rows = usize::MAX;
    job.wrap.max_width = (ui.available_width() - SIDE_RAIL_WIDTH_PX - 12.0).max(1.0);
    job
}

fn paint_trade(
    ui: &mut egui::Ui,
    job: egui::text::LayoutJob,
    side: Side,
    selected: bool,
    sense: egui::Sense,
) -> egui::Response {
    let palette = theme::get(ui.ctx());
    let galley = ui.painter().layout_job(job);
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), galley.size().y + 12.0),
        sense,
    );
    if ui.is_rect_visible(rect) {
        if selected || response.hovered() {
            ui.painter().rect_filled(
                rect,
                egui::Rounding::ZERO,
                if selected {
                    theme::active_tint(palette.accent())
                } else {
                    palette.border()
                },
            );
        }
        ui.painter().rect_filled(
            egui::Rect::from_min_size(rect.min, egui::vec2(SIDE_RAIL_WIDTH_PX, rect.height())),
            egui::Rounding::ZERO,
            palette.side_color(side),
        );
        ui.painter().galley(
            rect.min + egui::vec2(SIDE_RAIL_WIDTH_PX + 6.0, 6.0),
            galley,
            palette.text_primary(),
        );
    }
    response
}

/// How wide `text` paints in `font` - the measure the ledger's fitting
/// decisions in [`quantick_paper::report::fit`] are made with.
pub(super) fn text_width(painter: &egui::Painter, text: &str, font: &egui::FontId) -> f32 {
    let palette = theme::get(painter.ctx());
    painter
        .layout_no_wrap(text.to_owned(), font.clone(), palette.text_faint())
        .size()
        .x
}

/// One closed trade in the ledger. Every row selects on click (the tag
/// strip edits the selected one) and carries its tags on the detail line;
/// session rows also carry a separate jump-to-chart action. Rows from
/// earlier sessions never navigate — their tape is not the one on screen,
/// so the chart has nothing honest to point at.
pub(super) fn draw_ledger_row(
    ui: &mut egui::Ui,
    trade: &ClosedTrade,
    meta: TradeMeta<'_>,
    symbol: Option<&str>,
    selected: bool,
    session: bool,
    tz: TzOffset,
) -> LedgerRowResponse {
    let palette = theme::get(ui.ctx());
    let mut detail = ledger_detail(trade, tz);
    detail.push_str(&format!(
        " · {}",
        CivilDate::from_ms(trade.closed_ms, tz).short()
    ));
    if !meta.tags.is_empty() {
        detail.push_str(&format!(" · #{}", meta.tags.join(" #")));
    }
    let pnl = meta.pnl.closed(trade);
    let job = trade_text(
        ui,
        trade.side,
        (
            &format!(
                "{} {}",
                position_word(trade.side),
                fmt_decimal(trade.quantity)
            ),
            &format!(
                "{} → {}",
                fmt_decimal(trade.entry_price),
                fmt_decimal(trade.exit_price)
            ),
        ),
        Some((trade.pnl_points, &pnl)),
        &detail,
        symbol,
        source_glyph(&palette, meta.source),
    );
    let response = paint_trade(ui, job, trade.side, selected, egui::Sense::click());
    // Navigation gets its own row; it never covers the date or financial text.
    let navigate = session
        && ui
            .small_button(format!("{} Center on chart", icons::ARROW_UP_RIGHT))
            .clicked();
    LedgerRowResponse {
        clicked: response.clicked() && !navigate,
        navigate,
        response: Some(response),
    }
}

/// The detail line under a ledger row: the closing clock, how long the
/// trade was held, and why it ended. The instrument rides the head line
/// (see `RowLines::tag`) and the date the right-hand stamp, so this line
/// spends every character it has on the reason. Pure, so a test can assert
/// exactly what a trader will read.
pub(super) fn ledger_detail(trade: &ClosedTrade, tz: TzOffset) -> String {
    format!(
        "{} · {} · {}",
        crate::plot_area::fmt_time(trade.closed_ms, tz),
        fmt_duration_ms(trade.closed_ms.saturating_sub(trade.opened_ms)),
        trade.exit_reason.as_str().replace('_', " "),
    )
}
