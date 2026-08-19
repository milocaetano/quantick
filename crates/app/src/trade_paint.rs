//! Closed-trade paint: entry/exit marks and their connectors on the chart.
//!
//! Only this session's trades are painted — their fills happened on the
//! tape on screen, which is what makes the marks honest. Trades loaded from
//! earlier sessions stay in the ledger and the report; the chart has no
//! proof of their prints, so it does not draw them.
//!
//! The same test runs per trade, against the bars the pane holds *now*: a
//! fill whose instant is older than the oldest bar, or newer than the newest
//! print, paints nothing at all. A replay seek keeps the round trips (they
//! happened) and rebuilds the bars under them, so without this every earlier
//! trade would be clamped onto whichever bar sits at the edge and accumulate
//! there — marks at the start of the day for fills the tape has not reached.
//! How many were left off is said in the corner, beside the cap's own count:
//! an empty chart under a switched-on layer must not read as a lost ledger.
//! That line says "bars", not "tape": the canvas has a surface *called* the
//! tape, and a trader reading the corner must not think the count is about
//! that lane.
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
    let mut off_tape = 0usize;
    for (index, trade) in trades.iter().enumerate().rev() {
        let Some(points) = endpoints(frame, trade, &slot_at_time, &x_at_slot) else {
            off_tape += 1;
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
        draw_entry_mark(
            &painter,
            frame.background,
            entry,
            trade,
            color,
            is_selected,
            frame.scale.is_inverted(),
        );
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
    // Both ways a round trip can fail to reach the screen are said out loud
    // — the cap, and the tape not covering the fill. An empty chart with the
    // layer switched *on* is otherwise indistinguishable from the layer being
    // off, from a bug, or from the trades having been lost; after a replay
    // seek that is every trade of the session at once. Nothing is allocated
    // on a frame with nothing to report.
    if withheld > 0 || off_tape > 0 {
        let mut note = String::from("trade paint: ");
        if withheld > 0 {
            use std::fmt::Write as _;
            let _ = write!(note, "{drawn} of {} shown", drawn + withheld);
        }
        if off_tape > 0 {
            if withheld > 0 {
                note.push_str(" · ");
            }
            use std::fmt::Write as _;
            let _ = write!(note, "{off_tape} off the bars on screen");
        }
        painter.text(
            frame.chart_rect.left_bottom() + egui::vec2(8.0, -24.0),
            egui::Align2::LEFT_BOTTOM,
            note,
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
/// on the bars that held their venue times. `None` unless the series covers
/// *both* moments: an empty pane, a fresh tape after a reset, or a round
/// trip the bars on screen have not reached (or have already dropped).
///
/// Both ends or neither. One endpoint covered and the other clamped would
/// draw a connector running to a bar the fill has nothing to do with, which
/// is the lie this whole rule exists to refuse; the trade stays in the
/// ledger, where nothing about it was ever lost.
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
/// entry price: on the price side below the level for a long, above it for a
/// short. The glyph mirrors with the chart (`inverted`): this triangle is the
/// only carrier of trade direction — the hue says win or loss — and a fixed
/// screen offset would read every long as a short the moment the chart turns
/// over.
#[allow(clippy::too_many_arguments)]
fn draw_entry_mark(
    painter: &egui::Painter,
    background: egui::Color32,
    at: egui::Pos2,
    trade: &ClosedTrade,
    color: egui::Color32,
    selected: bool,
    inverted: bool,
) {
    let long = trade.side == Side::Buy;
    // +1 points the apex toward higher screen y; the product mirrors the
    // whole glyph when the chart is upside down.
    let side_sign = if long { 1.0 } else { -1.0 };
    let orientation = if inverted { -1.0 } else { 1.0 };
    let sign = side_sign * orientation;
    let apex_y = at.y + sign * MARKER_GAP_PX;
    let base_y = apex_y + sign * ENTRY_MARKER_H_PX;
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

    /// What one paint pass put on screen.
    struct Painted {
        /// Every shape emitted, marks and connectors alike.
        shapes: usize,
        /// Text galleys: the cap notice and the hover tooltip.
        texts: Vec<String>,
        /// The convex polygons — the entry triangles and exit diamonds — as
        /// their vertices, in paint order.
        polygons: Vec<Vec<egui::Pos2>>,
    }

    /// Run one paint pass against a real context, the pane's time→slot
    /// mapping covering the whole tape.
    fn painted(trades: &[ClosedTrade], pointer: Option<egui::Pos2>) -> Painted {
        painted_over(trades, pointer, |ms| usize::try_from(ms / 1000).ok())
    }

    /// Run one paint pass with `slot_at_time` standing in for the pane's
    /// own lookup — `None` is how a pane says its bars do not reach that
    /// instant, which is the whole subject of the tests below.
    fn painted_over(
        trades: &[ClosedTrade],
        pointer: Option<egui::Pos2>,
        slot_at_time: impl Fn(i64) -> Option<usize>,
    ) -> Painted {
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
                draw(&frame, trades, None, &slot_at_time, |slot| {
                    ((slot % 100) as f32) * 4.0
                });
            },
        );
        let mut texts = Vec::new();
        let mut polygons = Vec::new();
        let shapes = output.shapes.len();
        for shape in output.shapes {
            match shape.shape {
                egui::epaint::Shape::Text(text) => texts.push(text.galley.text().to_owned()),
                egui::epaint::Shape::Path(path) => polygons.push(path.points.to_vec()),
                _ => {}
            }
        }
        Painted {
            shapes,
            texts,
            polygons,
        }
    }

    /// A mapping that answers only for the window `[from, to]` seconds — a
    /// pane whose bars start and end there, in the tests' own units.
    fn tape(from: i64, to: i64) -> impl Fn(i64) -> Option<usize> {
        move |ms: i64| {
            (from * 1_000..=to * 1_000)
                .contains(&ms)
                .then(|| usize::try_from(ms / 1_000).ok())
                .flatten()
        }
    }

    #[test]
    fn a_round_trip_paints_marks_and_nothing_paints_empty() {
        let empty = painted(&[], None);
        let one = painted(&[trade(10, 20, 5)], None);
        assert!(
            one.shapes > empty.shapes,
            "a closed trade adds its marks: {} vs {}",
            one.shapes,
            empty.shapes
        );
        assert!(
            one.texts.is_empty(),
            "no cap note under the limit: {:?}",
            one.texts
        );
    }

    #[test]
    fn the_cap_discloses_what_it_withheld() {
        let trades: Vec<ClosedTrade> = (0..260).map(|index| trade(index, index + 1, 1)).collect();
        let out = painted(&trades, None);
        assert!(
            out.texts
                .iter()
                .any(|text| text == "trade paint: 200 of 260 shown"),
            "withheld marks are counted out loud: {:?}",
            out.texts
        );
    }

    #[test]
    fn a_hovered_mark_speaks_the_ledgers_words() {
        // Entry: slot 10 → x 40; price 100 → y 200 on this scale.
        let out = painted(&[trade(10, 20, 5)], Some(egui::pos2(41.0, 202.0)));
        let tooltip = out.texts.join(" ");
        assert!(
            tooltip.contains("LONG 1 · 100 → 105 · +5 pts"),
            "the tooltip heads with the ledger row's words: {:?}",
            out.texts
        );
        assert!(
            tooltip.contains("take profit"),
            "and tells the exit reason: {:?}",
            out.texts
        );
    }

    /// The covered trade's geometry, pinned: the rule below removes marks
    /// the tape cannot prove, and this is the half that must not move.
    #[test]
    fn a_covered_round_trip_lands_on_its_own_bars() {
        // Entry slot 15 → x 60, price 100 → y 200, apex one gap under it
        // because a long points down. Exit slot 25 → x 100, price 105 → y
        // 150, diamond centred there.
        let out = painted_over(&[trade(15, 25, 5)], None, tape(10, 30));
        assert_eq!(out.polygons.len(), 2, "one triangle and one diamond");
        let apex = out.polygons[0][0];
        assert!(
            (apex.x - 60.0).abs() < 0.01 && (apex.y - 203.0).abs() < 0.01,
            "the entry apex sits on the bar holding the fill: {apex:?}"
        );
        let top = out.polygons[1][0];
        assert!(
            (top.x - 100.0).abs() < 0.01 && (top.y - 145.5).abs() < 0.01,
            "and the exit diamond on its own: {top:?}"
        );
    }

    /// The rule itself: a fill the bars on screen do not reach paints no
    /// mark — not a triangle, not a diamond, not half a round trip — and
    /// says so instead of leaving an empty chart to explain itself.
    #[test]
    fn a_trade_the_tape_does_not_cover_paints_nothing() {
        for (name, round_trip) in [
            ("older than the oldest bar", trade(2, 5, 5)),
            ("newer than the newest print", trade(40, 50, 5)),
            ("entry off the tape, exit on it", trade(5, 20, 5)),
            ("entry on the tape, exit past it", trade(20, 40, 5)),
        ] {
            let out = painted_over(&[round_trip], None, tape(10, 30));
            assert!(out.polygons.is_empty(), "{name}: a mark was painted");
            assert_eq!(
                out.texts,
                vec!["trade paint: 1 off the bars on screen".to_owned()],
                "{name}: the chart did not say what it left off"
            );
        }
    }

    /// One end covered is not half a trade — a connector to a bar the fill
    /// has nothing to do with is the lie the rule exists to refuse — and an
    /// unpainted mark answers no pointer.
    #[test]
    fn an_off_tape_trade_cannot_be_hovered_either() {
        // The pixel the entry mark would have occupied had it been clamped
        // onto the tape's first bar (slot 10 → x 40, price 100 → y 200).
        let out = painted_over(
            &[trade(2, 5, 5)],
            Some(egui::pos2(41.0, 202.0)),
            tape(10, 30),
        );
        let text = out.texts.join(" ");
        assert!(
            !text.contains("LONG"),
            "an unpainted mark still answered the pointer: {:?}",
            out.texts
        );
    }

    /// The cap counts paint, not ledger rows: trades the tape does not
    /// cover were never candidates, so they are reported as what they are
    /// rather than as "withheld".
    #[test]
    fn the_cap_counts_only_what_the_tape_covers() {
        let trades: Vec<ClosedTrade> = (0..260).map(|index| trade(index, index + 1, 1)).collect();
        // The rebuilt tape holds the last hundred round trips; the other
        // hundred and sixty are off it.
        let out = painted_over(&trades, None, tape(160, 400));
        assert_eq!(
            out.texts,
            vec!["trade paint: 160 off the bars on screen".to_owned()],
            "a hundred marks fit under the cap of 200, and the rest are named"
        );
        assert_eq!(
            out.polygons.len(),
            200,
            "two marks each for the hundred the tape proves"
        );
    }

    /// Both counts at once: the cap bit *and* the tape fell short.
    #[test]
    fn the_notice_carries_the_cap_and_the_tape_together() {
        let trades: Vec<ClosedTrade> = (0..500).map(|index| trade(index, index + 1, 1)).collect();
        let out = painted_over(&trades, None, tape(100, 500));
        assert_eq!(
            out.texts,
            vec!["trade paint: 200 of 400 shown · 100 off the bars on screen".to_owned()],
            "the corner reports both reasons a round trip is not on screen"
        );
    }
}
