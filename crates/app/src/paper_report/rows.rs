//! One row of the ledger, painted.
//!
//! A day header, a group header, an open position, a closed trade, and the
//! "N more" row that reveals the next page. Each is a painter call and a hit
//! test, with no state of its own - what to draw arrives as an argument, and
//! what the pointer did leaves as a return value.

use eframe::egui;
use egui_phosphor::regular as icons;
use quantick_appearance::FontRole;
use quantick_appearance::paper::{DAY_EDGE_PX, RowKind, RowLook, ledger_row};
use quantick_engine::Side;
use quantick_sim::ClosedTrade;
use rust_decimal::Decimal;

use super::sessions::source_glyph;
use super::{LEDGER_PAGE_TRADES, LEDGER_ROW_HEIGHT_PX, OpenRow, SIDE_RAIL_WIDTH_PX};
use crate::paper_calendar::CivilDate;
use crate::paper_chrome::{
    financial_text, fmt_decimal, fmt_duration_ms, points_color, position_word,
};
use crate::theme::{self, BOLD_STRIKE_PX};
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

/// A folding civil-day band: a bold caret and date over its complete count
/// and net, on the control fill with a rule down its left edge
/// (`paper::RowKind::Day`). The measured height lets both lines wrap.
pub(super) fn draw_day_header(
    ui: &mut egui::Ui,
    date: CivilDate,
    count: usize,
    net: &(Decimal, String),
    folded: bool,
) -> bool {
    let palette = theme::get(ui.ctx());
    let look = ledger_row(palette.resolved.dark, RowKind::Day);
    let ink = *palette.get(look.ink);
    let mut job = egui::text::LayoutJob::default();
    for (text, font_id) in [
        (format!("{} ", caret(folded)), palette.font(FontRole::Body)),
        (format!("{}\n", date.long()), palette.mono_font(10.0)),
    ] {
        job.append(
            &text,
            0.0,
            egui::TextFormat {
                font_id,
                color: ink,
                valign: egui::Align::Center,
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
    job.wrap.max_width = (ui.available_width() - 12.0 - DAY_EDGE_PX).max(1.0);
    let galley = ui.painter().layout_job(job);
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), galley.size().y + 12.0),
        egui::Sense::click(),
    );
    if ui.is_rect_visible(rect) {
        paint_look(ui, rect, &look, response.hovered(), DAY_EDGE_PX);
        let at = rect.min + egui::vec2(6.0 + DAY_EDGE_PX, 6.0);
        // egui bundles one weight: the band struck twice reads bold.
        let strike = at + egui::vec2(BOLD_STRIKE_PX, 0.0);
        ui.painter().galley(strike, galley.clone(), ink);
        ui.painter().galley(at, galley, ink);
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

/// A ledger row's `look` over `rect`: its fill (the hover fill while `lit`),
/// its left rule `edge_px` wide, and a lit plate's outline.
pub(super) fn paint_look(ui: &egui::Ui, rect: egui::Rect, look: &RowLook, lit: bool, edge_px: f32) {
    let (palette, painter) = (theme::get(ui.ctx()), ui.painter());
    let fill = if lit {
        look.hover_fill.or(look.fill)
    } else {
        look.fill
    };
    let rounding = if look.edge.is_some() { 0.0 } else { 3.0 };
    for (token, area) in [
        (fill, rect),
        (look.edge, rect.with_max_x(rect.left() + edge_px)),
    ] {
        if let Some(token) = token {
            painter.rect_filled(area, rounding, *palette.get(token));
        }
    }
    if let Some(edge) = look.hover_edge.filter(|_| lit) {
        let stroke = egui::Stroke::new(1.0_f32, *palette.get(edge));
        painter.rect_stroke(rect.shrink(0.5), rounding, stroke);
    }
}

/// The fold caret a day or session row leads with.
pub(super) fn caret(folded: bool) -> &'static str {
    if folded {
        icons::CARET_RIGHT
    } else {
        icons::CARET_DOWN
    }
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
            palette.hover_fill()
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
    let look = ledger_row(
        palette.resolved.dark,
        RowKind::Trade {
            long: side == Side::Buy,
        },
    );
    let galley = ui.painter().layout_job(job);
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), galley.size().y + 12.0),
        sense,
    );
    if ui.is_rect_visible(rect) {
        if selected {
            let tint = theme::active_tint(palette.accent());
            ui.painter().rect_filled(rect, egui::Rounding::ZERO, tint);
        }
        paint_look(
            ui,
            rect,
            &look,
            response.hovered() && !selected,
            SIDE_RAIL_WIDTH_PX,
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
    // An accent text action, not a pill: the row is the thing, this a link.
    let navigate = session
        && ui
            .link(egui::RichText::new(format!("{} Center on chart", icons::ARROW_UP_RIGHT)).small())
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
