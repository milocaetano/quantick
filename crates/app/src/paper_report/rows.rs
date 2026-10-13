//! One row of the ledger, painted.
//!
//! A day header, a group header, an open position, a closed trade, and the
//! "N more" row that reveals the next page. Each is a painter call and a hit
//! test, with no state of its own - what to draw arrives as an argument, and
//! what the pointer did leaves as a return value.

use std::collections::{BTreeMap, BTreeSet};

use eframe::egui;
use egui_phosphor::regular as icons;
use quantick_engine::Side;
use quantick_paper::pnl::SymbolPnl;
use quantick_paper::report::sessions::{
    SessionKey, SessionLabel, SessionRow, TradeRef, group_by_day_and_session, place_session_rows,
    source_word,
};
use quantick_sim::ClosedTrade;
use quantick_sim::history::SessionSource;
use rust_decimal::Decimal;

use super::sessions::{SessionHead, source_glyph};
use super::{
    DAY_HEADER_INSET_PX, DAY_HEADER_TEXT_X_PX, DETAIL_GAP_PX, DETAIL_RIGHT_PAD_PX,
    LEDGER_PAGE_TRADES, LEDGER_ROW_HEIGHT_PX, OpenRow, SIDE_RAIL_WIDTH_PX,
};
use crate::paper_calendar::CivilDate;
use crate::paper_chrome::{fmt_decimal, fmt_duration_ms, points_color, position_word};
use crate::theme;
use crate::timezone::TzOffset;

/// One virtualised ledger line.
pub(super) enum LedgerRow<'a> {
    /// A group caption and its count.
    Header(&'static str, usize),
    /// A civil day's caption: the date, how many of the rows under it
    /// closed on that day, what they netted, and whether it is folded
    /// shut. A ledger of bare clock times cannot answer "which session was
    /// that" — the day header is where the answer lives, it carries the
    /// day's result for free, and clicking it folds the day away.
    Day(CivilDate, usize, (Decimal, String), bool),
    /// A session's own row inside its day — every session a file holds.
    SessionHead(SessionHead),
    /// The rename or delete row standing where its session's row was.
    Inline,
    /// A closed trade from this session's simulator: selectable, and its
    /// round trip is on the current tape.
    Session(usize, &'a ClosedTrade, TradeMeta<'a>),
    /// A row loaded from an earlier session's journal. Selectable for its
    /// tags; its tape is not the one on screen, so it never navigates.
    Earlier(&'a str, &'a ClosedTrade, TradeMeta<'a>),
    /// This session's trades the filter hides — said, never dropped.
    HiddenLive(usize),
    /// The control that reveals the next page of saved history, carrying
    /// how many trades are still held back.
    More(usize),
}

/// What the session layer knows of one trade row.
#[derive(Clone, Copy)]
pub(super) struct TradeMeta<'a> {
    /// The session file it is in; `None` when no file under the trades
    /// folder took it.
    pub(super) session: Option<&'a SessionKey>,
    /// Which trade of that file it is.
    pub(super) trade_ref: TradeRef,
    pub(super) source: Option<SessionSource>,
    pub(super) tags: &'a [String],
    /// How its P&L is worded, bound to its own symbol.
    pub(super) pnl: SymbolPnl<'a>,
}

/// One trade before it is cut into rows.
pub(super) struct Item<'a> {
    pub(super) trade: &'a ClosedTrade,
    pub(super) symbol: &'a str,
    /// Its index among this session's closed trades; `None` for a saved row.
    pub(super) live: Option<usize>,
    pub(super) meta: TradeMeta<'a>,
}

/// How the list folds and shows sessions while rows are cut.
pub(super) struct GroupView<'s> {
    pub(super) collapsed_days: &'s BTreeSet<i64>,
    pub(super) collapsed_sessions: &'s BTreeSet<SessionKey>,
    /// The session F2 and Del act on.
    pub(super) focused: Option<&'s SessionKey>,
    /// The session whose row an inline rename or delete replaces.
    pub(super) editing: Option<&'s SessionKey>,
    pub(super) labels: &'s BTreeMap<SessionKey, SessionLabel>,
    /// Sessions a tab still journals to.
    pub(super) live: &'s BTreeSet<SessionKey>,
    pub(super) tz: TzOffset,
}

/// What one painted ledger row reported back.
#[derive(Default)]
pub(super) struct LedgerRowResponse {
    pub(super) clicked: bool,
    pub(super) navigate: bool,
    /// The row's own response, for its context menu.
    pub(super) response: Option<egui::Response>,
}

/// Cut `items` (newest first) into rows: a header per civil day, then each
/// session's own row where [`place_session_rows`] gives it one, then the
/// trades. `inline_placed` carries the one inline editor across the lists
/// of one ledger.
pub(super) fn push_groups<'a>(
    rows: &mut Vec<LedgerRow<'a>>,
    items: &'a [Item<'a>],
    view: &GroupView<'_>,
    inline_placed: &mut bool,
) {
    let mut days = group_by_day_and_session(
        items
            .iter()
            .map(|item| (item.meta.session, item.meta.pnl, item.trade)),
        view.tz,
    );
    place_session_rows(&mut days, view.editing, view.collapsed_days, inline_placed);
    for day in days {
        let folded = view.collapsed_days.contains(&day.day.day_number());
        rows.push(LedgerRow::Day(
            day.day,
            day.count,
            (day.net, day.net_text),
            folded,
        ));
        // A folded day builds no trade rows at all — the header keeps the
        // date, the count and the net, so the day is summarised rather
        // than merely hidden, and the frame does not pay for what it does
        // not show.
        if folded {
            continue;
        }
        for session in day.sessions {
            let first = &items[session.items[0]];
            if session.row != SessionRow::Bare
                && let Some(key) = &session.key
            {
                if session.row == SessionRow::Inline {
                    rows.push(LedgerRow::Inline);
                } else {
                    rows.push(LedgerRow::SessionHead(SessionHead {
                        key: key.clone(),
                        label: view.labels.get(key).map_or_else(
                            || source_word(first.meta.source).to_owned(),
                            |label| label.text.clone(),
                        ),
                        source: first.meta.source,
                        count: session.count,
                        net: (session.net, session.net_text.clone()),
                        folded: view.collapsed_sessions.contains(key),
                        focused: view.focused == Some(key),
                        live: view.live.contains(key),
                    }));
                }
                if view.collapsed_sessions.contains(key) {
                    continue;
                }
            }
            rows.extend(session.items.iter().map(|&index| {
                let item = &items[index];
                match item.live {
                    Some(live) => LedgerRow::Session(live, item.trade, item.meta),
                    None => LedgerRow::Earlier(item.symbol, item.trade, item.meta),
                }
            }));
        }
    }
}

/// A group caption inside the virtualised list, sharing the fixed row
/// height so `show_rows` stays honest about where every row is.
pub(super) fn draw_group_header(ui: &mut egui::Ui, label: &str, count: usize) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(width, LEDGER_ROW_HEIGHT_PX),
        egui::Sense::hover(),
    );
    if !ui.is_rect_visible(rect) {
        return;
    }
    ui.painter().text(
        egui::pos2(rect.left() + 2.0, rect.bottom() - 4.0),
        egui::Align2::LEFT_BOTTOM,
        format!("{label} · {count}"),
        egui::FontId::monospace(10.0),
        theme::TEXT_FAINT,
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
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(width, LEDGER_ROW_HEIGHT_PX),
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
            theme::BORDER
        } else {
            theme::INSET
        },
    );
    painter.line_segment(
        [
            egui::pos2(rect.left(), band.top()),
            egui::pos2(rect.right(), band.top()),
        ],
        egui::Stroke::new(1.0_f32, theme::BORDER),
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
        egui::FontId::proportional(10.0),
        theme::TEXT_FAINT,
    );
    painter.text(
        egui::pos2(rect.left() + DAY_HEADER_TEXT_X_PX, band.center().y),
        egui::Align2::LEFT_CENTER,
        date.long(),
        egui::FontId::monospace(10.0),
        if folded {
            theme::TEXT_FAINT
        } else {
            theme::TEXT_MUTED
        },
    );
    painter.text(
        egui::pos2(rect.right() - 6.0, band.center().y),
        egui::Align2::RIGHT_CENTER,
        format!("{count} · {}", net.1),
        egui::FontId::monospace(10.0),
        points_color(net.0),
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
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(width, LEDGER_ROW_HEIGHT_PX),
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
            theme::BORDER
        } else {
            theme::CONTROL
        },
    );
    ui.painter().text(
        body.center(),
        egui::Align2::CENTER_CENTER,
        format!("{}  show older · {remaining} more saved", icons::CARET_DOWN),
        egui::FontId::monospace(10.0),
        theme::TEXT_MUTED,
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
    let summary = &open.summary;
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(width, LEDGER_ROW_HEIGHT_PX),
        egui::Sense::hover(),
    );
    if !ui.is_rect_visible(rect) {
        return;
    }
    let painter = ui.painter();
    painter.rect_filled(rect, egui::Rounding::ZERO, theme::INSET);
    for y in [rect.top(), rect.bottom()] {
        painter.line_segment(
            [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
            egui::Stroke::new(1.0_f32, theme::BORDER),
        );
    }
    painter.rect_filled(
        egui::Rect::from_min_max(
            rect.left_top(),
            egui::pos2(rect.left() + SIDE_RAIL_WIDTH_PX, rect.bottom()),
        ),
        egui::Rounding::ZERO,
        theme::side_color(summary.side),
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
    let font = egui::FontId::monospace(11.0);
    let x = rect.left() + SIDE_RAIL_WIDTH_PX + 6.0;
    let y1 = rect.top() + 9.0;
    let y2 = rect.top() + 24.0;
    let color = theme::side_color(lines.side);
    let head = painter.layout_no_wrap(lines.head, font.clone(), color);
    let head_w = head.size().x;
    painter.galley(egui::pos2(x, y1 - head.size().y / 2.0), head, color);
    painter.text(
        egui::pos2(x + head_w + 8.0, y1),
        egui::Align2::LEFT_CENTER,
        lines.route,
        font.clone(),
        theme::TEXT_MUTED,
    );
    let mut head_right = rect.right() - DETAIL_RIGHT_PAD_PX;
    if let Some((points, text)) = lines.points {
        let galley = painter.layout_no_wrap(text, font.clone(), points_color(points));
        let width = galley.size().x;
        painter.galley(
            egui::pos2(head_right - width, y1 - galley.size().y / 2.0),
            galley,
            points_color(points),
        );
        head_right -= width + DETAIL_GAP_PX;
    }
    if let Some(tag) = lines.tag {
        let galley = painter.layout_no_wrap(tag, font.clone(), theme::TEXT_FAINT);
        let tag_w = galley.size().x;
        painter.galley(
            egui::pos2(head_right - tag_w, y1 - galley.size().y / 2.0),
            galley,
            theme::TEXT_FAINT,
        );
        head_right -= tag_w + 4.0;
    }
    if let Some((glyph, color)) = lines.marker {
        painter.text(
            egui::pos2(head_right, y1),
            egui::Align2::RIGHT_CENTER,
            glyph,
            egui::FontId::proportional(10.0),
            color,
        );
    }
    // The detail line and its stamp share one row from opposite ends.
    let detail_font = egui::FontId::monospace(10.0);
    let mut detail_limit = rect.right() - DETAIL_RIGHT_PAD_PX - x;
    if let Some(stamp) = lines.stamp {
        let galley = painter.layout_no_wrap(stamp, detail_font.clone(), theme::TEXT_FAINT);
        let stamp_w = galley.size().x;
        painter.galley(
            egui::pos2(
                rect.right() - DETAIL_RIGHT_PAD_PX - stamp_w,
                y2 - galley.size().y / 2.0,
            ),
            galley,
            theme::TEXT_FAINT,
        );
        detail_limit -= stamp_w + DETAIL_GAP_PX;
    }
    // Monospace, so one measured glyph gives the budget for all of them.
    let glyph_w = painter
        .layout_no_wrap("0".to_owned(), detail_font.clone(), theme::TEXT_FAINT)
        .size()
        .x
        .max(1.0);
    let budget = (detail_limit / glyph_w).floor().max(0.0) as usize;
    painter.text(
        egui::pos2(x, y2),
        egui::Align2::LEFT_CENTER,
        elide_tail(&lines.detail, budget),
        detail_font,
        theme::TEXT_FAINT,
    );
}

/// Cut `text` to `budget` characters, marking the cut with `…` so a
/// shortened exit reason can never be read as a complete one. Below the
/// ellipsis plus one character there is nothing honest left to say, so the
/// text is dropped entirely rather than reduced to a lone `…`.
pub(super) fn elide_tail(text: &str, budget: usize) -> String {
    if text.chars().count() <= budget {
        return text.to_owned();
    }
    if budget < 2 {
        return String::new();
    }
    let mut out: String = text.chars().take(budget - 1).collect();
    out.push('…');
    out
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
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(width, LEDGER_ROW_HEIGHT_PX),
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
            theme::active_tint(theme::ACCENT),
        );
    } else if response.hovered() {
        ui.painter()
            .rect_filled(rect, egui::Rounding::ZERO, theme::BORDER);
    }
    ui.painter().rect_filled(
        egui::Rect::from_min_max(
            rect.left_top(),
            egui::pos2(rect.left() + SIDE_RAIL_WIDTH_PX, rect.bottom()),
        ),
        egui::Rounding::ZERO,
        theme::side_color(trade.side),
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
            marker: source_glyph(meta.source),
        },
    );
    let mut navigate = false;
    if session && response.hovered() {
        let nav_rect = egui::Rect::from_center_size(
            egui::pos2(rect.right() - 14.0, rect.top() + 24.0),
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
            egui::FontId::proportional(11.0),
            if nav.hovered() {
                theme::TEXT_PRIMARY
            } else {
                theme::TEXT_MUTED
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
