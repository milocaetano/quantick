//! One row of the ledger, painted.
//!
//! A day header, a group header, an open position, a closed trade, and the
//! "N more" row that reveals the next page. Each is a painter call and a hit
//! test, with no state of its own - what to draw arrives as an argument, and
//! what the pointer did leaves as a return value.

use std::sync::Arc;

use eframe::egui;
use egui_phosphor::regular as icons;
use quantick_engine::Side;
use quantick_paper::report::fit::{count_and_net, fit_head, fit_line};
use quantick_sim::ClosedTrade;
use rust_decimal::Decimal;

use super::sessions::source_glyph;
use super::{
    DAY_HEADER_INSET_PX, DAY_HEADER_TEXT_X_PX, DETAIL_GAP_PX, DETAIL_RIGHT_PAD_PX,
    LEDGER_PAGE_TRADES, LEDGER_ROW_HEIGHT_PX, OpenRow, SIDE_RAIL_WIDTH_PX,
};
use crate::paper_calendar::CivilDate;
use crate::paper_chrome::{fmt_decimal, fmt_duration_ms, points_color, position_word};
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

/// A group caption inside the virtualised list, sharing the fixed row
/// height so `show_rows` stays honest about where every row is.
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

/// A civil day's caption: a fold caret and the date on the left, the
/// day's trade count and net on the right, tinted by the result. The row
/// is the ledger's answer to "which day am I looking at" while scrolling
/// back through months, and clicking it folds that day to this one line.
/// Returns whether it was clicked.
pub(super) fn draw_day_header(
    ui: &mut egui::Ui,
    date: CivilDate,
    count: usize,
    net: &(Decimal, String),
    folded: bool,
) -> bool {
    let palette = theme::get(ui.ctx());
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(width, palette.layout(LEDGER_ROW_HEIGHT_PX)),
        egui::Sense::click(),
    );
    if !ui.is_rect_visible(rect) {
        return response.clicked();
    }
    let painter = ui.painter();
    let band = egui::Rect::from_min_max(
        egui::pos2(rect.left(), rect.top() + DAY_HEADER_INSET_PX),
        rect.right_bottom(),
    );
    painter.rect_filled(
        band,
        egui::Rounding::ZERO,
        if response.hovered() {
            palette.border()
        } else {
            palette.inset()
        },
    );
    painter.line_segment(
        [
            egui::pos2(rect.left(), band.top()),
            egui::pos2(rect.right(), band.top()),
        ],
        egui::Stroke::new(1.0_f32, palette.border()),
    );
    // The caret is the affordance: a header that folds must look like it
    // folds, or the click is a secret.
    painter.text(
        egui::pos2(rect.left() + 6.0, band.center().y),
        egui::Align2::LEFT_CENTER,
        if folded {
            icons::CARET_RIGHT
        } else {
            icons::CARET_DOWN
        },
        palette.body_font(10.0),
        palette.text_faint(),
    );
    // A narrow panel shortens the date first, then drops the points
    // qualifier; the count and the money net always stay whole.
    let rights = count_and_net(count, &net.1);
    let date_color = if folded {
        palette.text_faint()
    } else {
        palette.text_muted()
    };
    let ends = [
        rect.left() + palette.layout(DAY_HEADER_TEXT_X_PX),
        rect.right() - 6.0,
        band.center().y,
    ];
    paint_fitted(
        painter,
        palette.mono_font(10.0),
        ends,
        &[&date.long(), &date.short()],
        &[&rights[0], &rights[1]],
        [date_color, points_color(&palette, net.0)],
    );
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

/// The pinned open-position row: sunken, live open points on the right,
/// the current mark standing in for the exit.
pub(super) fn draw_open_row(ui: &mut egui::Ui, open: &OpenRow, symbol: Option<&str>) {
    let palette = theme::get(ui.ctx());
    let summary = &open.summary;
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(width, palette.layout(LEDGER_ROW_HEIGHT_PX)),
        egui::Sense::hover(),
    );
    if !ui.is_rect_visible(rect) {
        return;
    }
    let painter = ui.painter();
    painter.rect_filled(rect, egui::Rounding::ZERO, palette.inset());
    for y in [rect.top(), rect.bottom()] {
        painter.line_segment(
            [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
            egui::Stroke::new(1.0_f32, palette.border()),
        );
    }
    painter.rect_filled(
        egui::Rect::from_min_max(
            rect.left_top(),
            egui::pos2(rect.left() + SIDE_RAIL_WIDTH_PX, rect.bottom()),
        ),
        egui::Rounding::ZERO,
        palette.side_color(summary.side),
    );
    let exit = open.mark_price.map_or_else(|| "…".to_owned(), fmt_decimal);
    let held = open
        .held_ms
        .map_or_else(String::new, |ms| format!("open {}", fmt_duration_ms(ms)));
    draw_row_lines(
        painter,
        rect,
        RowLines {
            side: summary.side,
            head: format!(
                "{} {}",
                position_word(summary.side),
                fmt_decimal(summary.quantity)
            ),
            route: format!("{} → {}", fmt_decimal(summary.avg_price), exit),
            points: summary.open_points.zip(open.pnl.clone()),
            detail: format!("{held} · at the last print"),
            // No stamp: the open position closes at a time nobody knows
            // yet, and a date on a trade still running would be a guess.
            stamp: None,
            tag: symbol.map(str::to_owned),
            marker: None,
        },
    );
}

/// The two text lines every ledger row shares.
struct RowLines {
    side: Side,
    head: String,
    route: String,
    points: Option<(Decimal, String)>,
    detail: String,
    /// A stamp pinned to the right end of the detail line — the trade's
    /// date. It gets the space no other field wants (under the points),
    /// and anchoring it opposite the detail means the two can only ever
    /// collide in the middle, where the elision is visible.
    stamp: Option<String>,
    /// The instrument, riding the empty stretch of the head line between
    /// the round trip and the points. It sat on the detail line first, and
    /// the exit reason paid for it in elided characters — "stop …" is not
    /// a fact, and the room to say "stop loss" was free one line up.
    tag: Option<String>,
    /// The session's source glyph, left of the instrument: amber play for a
    /// replay, a faint `?` for an unrecorded file, nothing for live.
    marker: Option<(&'static str, egui::Color32)>,
}

fn draw_row_lines(painter: &egui::Painter, rect: egui::Rect, lines: RowLines) {
    let painter = &painter.with_clip_rect(rect);
    let palette = theme::get(painter.ctx());
    let font = palette.mono_font(11.0);
    let x = rect.left() + SIDE_RAIL_WIDTH_PX + 6.0;
    let y1 = rect.top() + palette.layout(9.0);
    let y2 = rect.top() + palette.layout(24.0);
    let color = palette.side_color(lines.side);
    // The right end is laid out first, right to left - P&L, tag, source
    // marker - each with the gap to its left. Only the tag may drop; the
    // route gets what is left.
    let piece = |text, font, color, gap| (painter.layout_no_wrap(text, font, color), color, gap);
    let points = (lines.points).map(|(points, text)| {
        piece(
            quantick_paper::report::fit::elide_to_width(
                &text,
                (rect.right()
                    - x
                    - DETAIL_RIGHT_PAD_PX
                    - 2.0 * DETAIL_GAP_PX
                    - palette.layout(12.0))
                .max(0.0),
                |text| text_width(painter, text, &font),
            ),
            font.clone(),
            points_color(&palette, points),
            DETAIL_GAP_PX,
        )
    });
    let tag = (lines.tag).map(|tag| piece(tag, font.clone(), palette.text_faint(), 4.0));
    let glyph_font = palette.body_font(10.0);
    let marker = (lines.marker)
        .map(|(glyph, color)| piece(glyph.to_owned(), glyph_font, color, DETAIL_GAP_PX));
    let span = |piece: &Option<(Arc<egui::Galley>, _, f32)>| {
        piece
            .as_ref()
            .map_or(0.0, |(galley, _, gap)| galley.size().x + gap)
    };
    let mut right = rect.right() - DETAIL_RIGHT_PAD_PX;
    let fixed = span(&points) + span(&marker);
    let head = quantick_paper::report::fit::elide_to_width(
        &lines.head,
        (right - x - fixed + DETAIL_GAP_PX).max(0.0),
        |text| text_width(painter, text, &font),
    );
    let head = painter.layout_no_wrap(head, font.clone(), color);
    let route_x = x + head.size().x + 8.0;
    painter.galley(egui::pos2(x, y1 - head.size().y / 2.0), head, color);
    let fit = fit_head(right - route_x, fixed, span(&tag), &lines.route, |text| {
        text_width(painter, text, &font)
    });
    let route_pos = egui::pos2(route_x, y1);
    painter.text(
        route_pos,
        egui::Align2::LEFT_CENTER,
        fit.route,
        font,
        palette.text_muted(),
    );
    for (galley, color, gap) in [points, tag.filter(|_| fit.tag), marker]
        .into_iter()
        .flatten()
    {
        let size = galley.size();
        painter.galley(egui::pos2(right - size.x, y1 - size.y / 2.0), galley, color);
        right -= size.x + gap;
    }
    // The detail line and its stamp share one row from opposite ends.
    let ends = [x, rect.right() - DETAIL_RIGHT_PAD_PX, y2];
    let stamp = lines.stamp.as_deref().unwrap_or_default();
    let font = palette.mono_font(10.0);
    paint_fitted(
        painter,
        font,
        ends,
        &[&lines.detail],
        &[stamp],
        [palette.text_faint(); 2],
    );
}

/// Paints a line fitted by [`fit_line`]: the left form from `x0`, the right
/// one ending at `x1`, both centred on `y`.
pub(super) fn paint_fitted(
    painter: &egui::Painter,
    font: egui::FontId,
    [x0, x1, y]: [f32; 3],
    lefts: &[&str],
    rights: &[&str],
    [left_color, right_color]: [egui::Color32; 2],
) {
    let measure = |text: &str| text_width(painter, text, &font);
    let fit = fit_line(x1 - x0, DETAIL_GAP_PX, lefts, rights, measure);
    let (left, right) = (egui::pos2(x0, y), egui::pos2(x1, y));
    painter.text(
        left,
        egui::Align2::LEFT_CENTER,
        fit.left,
        font.clone(),
        left_color,
    );
    painter.text(
        right,
        egui::Align2::RIGHT_CENTER,
        fit.right,
        font,
        right_color,
    );
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
/// session rows also reveal a jump-to-chart control on hover. Rows from
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
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(width, palette.layout(LEDGER_ROW_HEIGHT_PX)),
        egui::Sense::click(),
    );
    if !ui.is_rect_visible(rect) {
        return LedgerRowResponse {
            response: Some(response),
            ..LedgerRowResponse::default()
        };
    }
    if selected {
        ui.painter().rect_filled(
            rect,
            egui::Rounding::ZERO,
            theme::active_tint(palette.accent()),
        );
    } else if response.hovered() {
        ui.painter()
            .rect_filled(rect, egui::Rounding::ZERO, palette.border());
    }
    ui.painter().rect_filled(
        egui::Rect::from_min_max(
            rect.left_top(),
            egui::pos2(rect.left() + SIDE_RAIL_WIDTH_PX, rect.bottom()),
        ),
        egui::Rounding::ZERO,
        palette.side_color(trade.side),
    );
    let mut detail = ledger_detail(trade, tz);
    if !meta.tags.is_empty() {
        detail.push_str(" · #");
        detail.push_str(&meta.tags.join(" #"));
    }
    draw_row_lines(
        ui.painter(),
        rect,
        RowLines {
            side: trade.side,
            head: format!(
                "{} {}",
                position_word(trade.side),
                fmt_decimal(trade.quantity)
            ),
            route: format!(
                "{} → {}",
                fmt_decimal(trade.entry_price),
                fmt_decimal(trade.exit_price)
            ),
            points: Some((trade.pnl_points, meta.pnl.closed(trade))),
            detail,
            // Compact: the day header directly above carries the year.
            stamp: Some(CivilDate::from_ms(trade.closed_ms, tz).short()),
            tag: symbol.map(str::to_owned),
            marker: source_glyph(&palette, meta.source),
        },
    );
    let mut navigate = false;
    if session && response.hovered() {
        let nav_rect = egui::Rect::from_center_size(
            egui::pos2(rect.right() - 14.0, rect.top() + palette.layout(24.0)),
            egui::vec2(16.0, 14.0),
        );
        let nav = ui
            .interact(
                nav_rect,
                ui.id()
                    .with(("ledger_nav", trade.opened_ms, trade.closed_ms)),
                egui::Sense::click(),
            )
            .on_hover_text("center the chart on this trade");
        ui.painter().text(
            nav_rect.center(),
            egui::Align2::CENTER_CENTER,
            icons::ARROW_UP_RIGHT,
            palette.body_font(11.0),
            if nav.hovered() {
                palette.text_primary()
            } else {
                palette.text_muted()
            },
        );
        navigate = nav.clicked();
    }
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
