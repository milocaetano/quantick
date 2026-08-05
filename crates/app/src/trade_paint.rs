//! Closed-trade paint: entry/exit marks and their connectors on the chart.
//!
//! Only this session's trades are painted — their fills happened on the
//! tape on screen, which is what makes the marks honest. Trades loaded from
//! earlier sessions stay in the ledger and the report; the chart has no
//! proof of their prints, so it does not draw them.
//!
//! The encoding is scannable for outcomes: marks and connectors take the
//! *outcome* colour (win/loss/scratch), while direction is carried by
//! shape — a filled triangle pointing the trade's way at the entry, a
//! diamond at the exit. Both anchor to the fill *price* (the honest datum)
//! on the bar that held the fill's venue time, and every mark wears a ring
//! in the canvas colour so it survives a same-coloured candle behind it.
//!
//! Switched off (the `closed trade marks` layer), nothing here draws;
//! nothing is forgotten either — the trades stay in the ledger and on disk.

use eframe::egui;
use quantick_engine::Side;
use quantick_sim::ClosedTrade;
use rust_decimal::prelude::ToPrimitive;

use crate::chart::PriceScale;
use crate::paper_trading::{
    fmt_decimal, fmt_duration_ms, fmt_signed_points, points_color, position_word,
};
use crate::theme;
use crate::timezone::TzOffset;

/// Entry triangle footprint (width × height), sized to read at 100% and
/// stay under one candle body.
const ENTRY_MARKER_W_PX: f32 = 11.0;
/// Entry triangle height (see `ENTRY_MARKER_W_PX`).
const ENTRY_MARKER_H_PX: f32 = 8.0;
/// Half-diagonal of the exit diamond — a 9 px mark, visibly smaller than
/// the entry triangle because an exit is a point event.
const EXIT_MARKER_RADIUS_PX: f32 = 4.5;
/// Gap between a mark's tip and the price pixel the fill owns.
const MARKER_GAP_PX: f32 = 3.0;
/// Ring around every mark, in the canvas colour, for legibility over a
/// same-coloured candle.
const MARKER_RING_PX: f32 = 1.0;
/// Connector dash length — distinct from the last-price line's 4/4 rhythm.
const CONNECTOR_DASH_PX: f32 = 2.0;
/// Connector gap (see `CONNECTOR_DASH_PX`).
const CONNECTOR_GAP_PX: f32 = 3.0;
/// Connector alpha: an annotation crossing candles, not a series.
const CONNECTOR_ALPHA: u8 = 115;
/// Emphasis width of a selected trade's connector.
const SELECTED_CONNECTOR_WIDTH_PX: f32 = 1.5;
/// The drawings' halo treatment, for the selected trade's marks.
const SELECTION_HALO_COLOR: egui::Color32 = egui::Color32::from_rgba_premultiplied(40, 40, 40, 40);
/// How far past a mark's edge the selection halo reaches.
const SELECTION_HALO_EXTRA_PX: f32 = 3.5;
/// At most this many trades paint, newest first; past it the paint is
/// noise (≈ one mark per 4 px on a 1600 px chart). The withheld count is
/// said out loud, never silently dropped.
const TRADE_PAINT_LIMIT: usize = 200;
/// How close (px) the pointer must be to a mark for its tooltip.
const HOVER_RADIUS_PX: f32 = 8.0;

/// The frame geometry the paint reads, handed over by the pane.
pub(crate) struct TradePaintFrame<'a> {
    pub painter: &'a egui::Painter,
    pub chart_rect: egui::Rect,
    pub scale: &'a PriceScale,
    /// The pane's resolved canvas colour — the marks' ring.
    pub background: egui::Color32,
    /// The pointer, for the hover tooltip; `None` paints no tooltip.
    pub pointer: Option<egui::Pos2>,
    pub tz: TzOffset,
}

/// One drawable mark, kept for the hover pass.
struct Mark<'a> {
    position: egui::Pos2,
    trade: &'a ClosedTrade,
}

/// Paint the session's closed trades: connectors under marks, newest
/// trades first when the cap bites, the selected trade emphasized.
/// `slot_at_time` and `x_at_slot` are the pane's own mappings, so the
/// marks land exactly where the candles say that moment is.
pub(crate) fn draw(
    frame: &TradePaintFrame<'_>,
    trades: &[ClosedTrade],
    selected: Option<usize>,
    slot_at_time: impl Fn(i64) -> Option<usize>,
    x_at_slot: impl Fn(usize) -> f32,
) {
    let painter = frame.painter.with_clip_rect(frame.chart_rect);
    let mut marks: Vec<Mark<'_>> = Vec::new();
    let mut drawn = 0usize;
    let mut withheld = 0usize;
    for (index, trade) in trades.iter().enumerate().rev() {
        let Some(points) = endpoints(frame, trade, &slot_at_time, &x_at_slot) else {
            continue;
        };
        let (entry, exit) = points;
        let visible = entry.x.max(exit.x) >= frame.chart_rect.left()
            && entry.x.min(exit.x) <= frame.chart_rect.right();
        if !visible {
            continue;
        }
        if drawn >= TRADE_PAINT_LIMIT {
            withheld += 1;
            continue;
        }
        drawn += 1;
        let is_selected = selected == Some(index);
        // Win, loss or scratch — the one hue the whole round trip wears;
        // direction is carried by the entry mark's shape instead.
        let color = points_color(trade.pnl_points);
        let connector = if is_selected {
            egui::Stroke::new(SELECTED_CONNECTOR_WIDTH_PX, color)
        } else {
            egui::Stroke::new(
                1.0_f32,
                egui::Color32::from_rgba_unmultiplied(
                    color.r(),
                    color.g(),
                    color.b(),
                    CONNECTOR_ALPHA,
                ),
            )
        };
        painter.extend(egui::Shape::dashed_line(
            &[entry, exit],
            connector,
            CONNECTOR_DASH_PX,
            CONNECTOR_GAP_PX,
        ));
        draw_entry_mark(&painter, frame.background, entry, trade, color, is_selected);
        draw_exit_mark(&painter, frame.background, exit, color, is_selected);
        marks.push(Mark {
            position: entry,
            trade,
        });
        marks.push(Mark {
            position: exit,
            trade,
        });
    }
    if withheld > 0 {
        painter.text(
            frame.chart_rect.left_bottom() + egui::vec2(8.0, -24.0),
            egui::Align2::LEFT_BOTTOM,
            format!("trade paint: {drawn} of {} shown", drawn + withheld),
            egui::FontId::proportional(10.0),
            theme::TEXT_FAINT,
        );
    }
    if let Some(pointer) = frame.pointer
        && let Some(mark) = marks
            .iter()
            .filter(|mark| mark.position.distance(pointer) <= HOVER_RADIUS_PX)
            .min_by(|a, b| {
                a.position
                    .distance(pointer)
                    .total_cmp(&b.position.distance(pointer))
            })
    {
        draw_tooltip(frame, &painter, pointer, mark.trade);
    }
}

/// Screen endpoints of a round trip — entry and exit fills at their prices
/// on the bars that held their venue times. `None` while the chart has no
/// bar for those moments (a fresh tape after a reset, an empty pane).
fn endpoints(
    frame: &TradePaintFrame<'_>,
    trade: &ClosedTrade,
    slot_at_time: &impl Fn(i64) -> Option<usize>,
    x_at_slot: &impl Fn(usize) -> f32,
) -> Option<(egui::Pos2, egui::Pos2)> {
    let entry_slot = slot_at_time(trade.opened_ms)?;
    let exit_slot = slot_at_time(trade.closed_ms)?;
    let entry = egui::pos2(
        x_at_slot(entry_slot),
        frame
            .scale
            .y(trade.entry_price.to_f64().unwrap_or_default()),
    );
    let exit = egui::pos2(
        x_at_slot(exit_slot),
        frame.scale.y(trade.exit_price.to_f64().unwrap_or_default()),
    );
    Some((entry, exit))
}

/// A filled triangle pointing the trade's way, its apex one gap off the
/// entry price: under the level for a long, above it for a short.
fn draw_entry_mark(
    painter: &egui::Painter,
    background: egui::Color32,
    at: egui::Pos2,
    trade: &ClosedTrade,
    color: egui::Color32,
    selected: bool,
) {
    let long = trade.side == Side::Buy;
    let apex_y = if long {
        at.y + MARKER_GAP_PX
    } else {
        at.y - MARKER_GAP_PX
    };
    let base_y = if long {
        apex_y + ENTRY_MARKER_H_PX
    } else {
        apex_y - ENTRY_MARKER_H_PX
    };
    let half = ENTRY_MARKER_W_PX / 2.0;
    let points = vec![
        egui::pos2(at.x, apex_y),
        egui::pos2(at.x - half, base_y),
        egui::pos2(at.x + half, base_y),
    ];
    if selected {
        painter.add(egui::Shape::convex_polygon(
            points.clone(),
            SELECTION_HALO_COLOR,
            egui::Stroke::new(SELECTION_HALO_EXTRA_PX, SELECTION_HALO_COLOR),
        ));
    }
    painter.add(egui::Shape::convex_polygon(
        points,
        color,
        egui::Stroke::new(MARKER_RING_PX, background),
    ));
}

/// A diamond centred exactly on the exit price — a point event; its
/// direction is already told by the entry.
fn draw_exit_mark(
    painter: &egui::Painter,
    background: egui::Color32,
    at: egui::Pos2,
    color: egui::Color32,
    selected: bool,
) {
    let r = EXIT_MARKER_RADIUS_PX;
    let points = vec![
        egui::pos2(at.x, at.y - r),
        egui::pos2(at.x + r, at.y),
        egui::pos2(at.x, at.y + r),
        egui::pos2(at.x - r, at.y),
    ];
    if selected {
        painter.add(egui::Shape::convex_polygon(
            points.clone(),
            SELECTION_HALO_COLOR,
            egui::Stroke::new(SELECTION_HALO_EXTRA_PX, SELECTION_HALO_COLOR),
        ));
    }
    painter.add(egui::Shape::convex_polygon(
        points,
        color,
        egui::Stroke::new(MARKER_RING_PX, background),
    ));
}

/// The hover tooltip, in the ledger's own vocabulary so the two surfaces
/// never disagree about a trade.
fn draw_tooltip(
    frame: &TradePaintFrame<'_>,
    painter: &egui::Painter,
    pointer: egui::Pos2,
    trade: &ClosedTrade,
) {
    let font = egui::FontId::monospace(11.0);
    let caption = egui::FontId::monospace(10.0);
    let head = format!(
        "{} {} · {} → {} · {} pts",
        position_word(trade.side),
        fmt_decimal(trade.quantity),
        fmt_decimal(trade.entry_price),
        fmt_decimal(trade.exit_price),
        fmt_signed_points(trade.pnl_points),
    );
    let detail = format!(
        "{} · {} · {}",
        trade.exit_reason.as_str().replace('_', " "),
        crate::app::fmt_time(trade.closed_ms, frame.tz),
        fmt_duration_ms(trade.closed_ms.saturating_sub(trade.opened_ms)),
    );
    let head_galley = painter.layout_no_wrap(head, font, theme::TEXT_PRIMARY);
    let detail_galley = painter.layout_no_wrap(detail, caption, theme::TEXT_MUTED);
    let head_height = head_galley.size().y;
    let width = head_galley.size().x.max(detail_galley.size().x) + 16.0;
    let height = head_height + detail_galley.size().y + 14.0;
    let mut origin = pointer + egui::vec2(12.0, -height - 8.0);
    origin.x = origin
        .x
        .min(frame.chart_rect.right() - width)
        .max(frame.chart_rect.left());
    origin.y = origin.y.max(frame.chart_rect.top());
    let rect = egui::Rect::from_min_size(origin, egui::vec2(width, height));
    painter.rect_filled(rect, egui::Rounding::same(4.0), theme::TAG_BG);
    painter.galley(
        rect.min + egui::vec2(8.0, 4.0),
        head_galley,
        theme::TEXT_PRIMARY,
    );
    painter.galley(
        rect.min + egui::vec2(8.0, head_height + 8.0),
        detail_galley,
        theme::TEXT_MUTED,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use quantick_sim::ExitReason;
    use rust_decimal::Decimal;

    fn trade(open_s: i64, close_s: i64, pnl: i64) -> ClosedTrade {
        ClosedTrade {
            side: Side::Buy,
            quantity: Decimal::ONE,
            entry_price: Decimal::from(100),
            exit_price: Decimal::from(100 + pnl),
            opened_ms: open_s * 1000,
            closed_ms: close_s * 1000,
            pnl_points: Decimal::from(pnl),
            exit_reason: ExitReason::TakeProfit,
            entry_agg_id: Some(1),
            exit_agg_id: Some(2),
            mae_points: Some(Decimal::ZERO),
            mfe_points: Some(Decimal::from(pnl.max(0))),
        }
    }

    /// Run one paint pass against a real context; returns the shape count
    /// and every text galley painted.
    fn painted(trades: &[ClosedTrade], pointer: Option<egui::Pos2>) -> (usize, Vec<String>) {
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 400.0));
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ctx| {
                let painter = ctx.layer_painter(egui::LayerId::background());
                let scale = PriceScale::from_range(80.0, 120.0, 0.0, 400.0);
                let frame = TradePaintFrame {
                    painter: &painter,
                    chart_rect: screen,
                    scale: &scale,
                    background: egui::Color32::BLACK,
                    pointer,
                    tz: TzOffset::default(),
                };
                draw(
                    &frame,
                    trades,
                    None,
                    |ms| usize::try_from(ms / 1000).ok(),
                    |slot| ((slot % 100) as f32) * 4.0,
                );
            },
        );
        let mut texts = Vec::new();
        let count = output.shapes.len();
        for shape in output.shapes {
            if let egui::epaint::Shape::Text(text) = shape.shape {
                texts.push(text.galley.text().to_owned());
            }
        }
        (count, texts)
    }

    #[test]
    fn a_round_trip_paints_marks_and_nothing_paints_empty() {
        let (empty, _) = painted(&[], None);
        let (one, texts) = painted(&[trade(10, 20, 5)], None);
        assert!(
            one > empty,
            "a closed trade adds its marks: {one} vs {empty}"
        );
        assert!(texts.is_empty(), "no cap note under the limit: {texts:?}");
    }

    #[test]
    fn the_cap_discloses_what_it_withheld() {
        let trades: Vec<ClosedTrade> = (0..260).map(|index| trade(index, index + 1, 1)).collect();
        let (_, texts) = painted(&trades, None);
        assert!(
            texts
                .iter()
                .any(|text| text == "trade paint: 200 of 260 shown"),
            "withheld marks are counted out loud: {texts:?}"
        );
    }

    #[test]
    fn a_hovered_mark_speaks_the_ledgers_words() {
        // Entry: slot 10 → x 40; price 100 → y 200 on this scale.
        let (_, texts) = painted(&[trade(10, 20, 5)], Some(egui::pos2(41.0, 202.0)));
        let tooltip = texts.join(" ");
        assert!(
            tooltip.contains("LONG 1 · 100 → 105 · +5 pts"),
            "the tooltip heads with the ledger row's words: {texts:?}"
        );
        assert!(
            tooltip.contains("take profit"),
            "and tells the exit reason: {texts:?}"
        );
    }
}
