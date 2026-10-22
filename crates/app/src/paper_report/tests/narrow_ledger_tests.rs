//! The trades ledger docked at its narrowest: every line keeps its result
//! whole and no two pieces of text paint over each other.

use eframe::egui;
use quantick_engine::Side;
use quantick_paper::pnl::PnlReader;
use quantick_paper::report::readout::LedgerTotals;
use quantick_paper::report::sessions::{SessionKey, TradeRef};
use quantick_sim::history::SessionSource;
use quantick_sim::{ClosedTrade, ExitReason};
use rust_decimal::Decimal;

use super::super::ledger::{LedgerPage, draw_totals_strip};
use super::super::rows::{TradeMeta, draw_day_header, draw_ledger_row};
use super::super::sessions::{SessionHead, draw_session_head};
use crate::paper_calendar::CivilDate;
use crate::timezone::TzOffset;

/// The docked trades panel's width in the trader's screenshot.
const NARROW_PX: f32 = 300.0;
const WIDE_PX: f32 = 640.0;
const DAY_NET: &str = "+115.20 BRL · +205.5 pts/ct";
/// 2026-10-21 10:58:16 UTC.
const CLOSED_MS: i64 = 1_792_580_296_000;

fn short_four() -> ClosedTrade {
    ClosedTrade {
        side: Side::Sell,
        quantity: Decimal::from(4),
        entry_price: Decimal::new(2_119_805, 1),
        exit_price: Decimal::from(211_855),
        opened_ms: CLOSED_MS - 450_000,
        closed_ms: CLOSED_MS,
        pnl_points: Decimal::new(1_255, 1),
        exit_reason: ExitReason::TakeProfit,
        entry_agg_id: None,
        exit_agg_id: None,
        mae_points: None,
        mfe_points: None,
    }
}

/// Paints `draw` into a frameless panel `width` wide and returns every
/// non-empty piece of text it painted, with where it painted it.
fn painted_text(width: f32, mut draw: impl FnMut(&mut egui::Ui)) -> Vec<(String, egui::Rect)> {
    let ctx = egui::Context::default();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(width, 400.0),
        )),
        ..Default::default()
    };
    let output = ctx.run(input, |ctx| {
        egui::CentralPanel::default()
            .frame(egui::Frame::none())
            .show(ctx, |ui| draw(ui));
    });
    let mut texts = Vec::new();
    for clipped in &output.shapes {
        collect_text(&clipped.shape, &mut texts);
    }
    texts
}

fn collect_text(shape: &egui::Shape, out: &mut Vec<(String, egui::Rect)>) {
    match shape {
        egui::Shape::Text(text) if !text.galley.text().is_empty() => {
            out.push((text.galley.text().to_owned(), text.visual_bounding_rect()));
        }
        egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| collect_text(shape, out)),
        _ => {}
    }
}

/// No two painted texts share a pixel, and all of them stay in the panel.
fn assert_apart(texts: &[(String, egui::Rect)], width: f32) {
    for (i, (a, a_rect)) in texts.iter().enumerate() {
        assert!(
            a_rect.right() <= width + 0.5,
            "{a:?} runs out of the panel at {a_rect:?}"
        );
        for (b, b_rect) in &texts[i + 1..] {
            let overlap = a_rect.intersect(*b_rect);
            assert!(
                overlap.width() <= 0.0 || overlap.height() <= 0.0,
                "{a:?} at {a_rect:?} paints over {b:?} at {b_rect:?}"
            );
        }
    }
}

fn has(texts: &[(String, egui::Rect)], wanted: &str) -> bool {
    texts.iter().any(|(text, _)| text == wanted)
}

fn trade_row(width: f32, trade: &ClosedTrade) -> Vec<(String, egui::Rect)> {
    let tags = ["breakout".to_owned(), "absorption-at-vwap".to_owned()];
    let meta = TradeMeta {
        session: None,
        trade_ref: TradeRef::default(),
        source: Some(SessionSource::Replay),
        tags: &tags,
        pnl: PnlReader::plain().of("WINZ26"),
    };
    painted_text(width, |ui| {
        draw_ledger_row(
            ui,
            trade,
            meta,
            Some("WINZ26"),
            false,
            false,
            TzOffset::new(0),
        );
    })
}

/// The trader's screenshot: side, round trip, instrument and points ran
/// into one another. Narrow, the instrument goes and the points stay.
#[test]
fn a_trade_row_at_the_docked_width_keeps_its_result_and_overlaps_nothing() {
    let trade = short_four();
    let points = PnlReader::plain().of("WINZ26").closed(&trade);

    let narrow = trade_row(NARROW_PX, &trade);
    assert_apart(&narrow, NARROW_PX);
    assert!(has(&narrow, "SHORT 4"), "the side is never cut: {narrow:?}");
    assert!(
        has(&narrow, &points),
        "the points are never cut: {narrow:?}"
    );

    let wide = trade_row(WIDE_PX, &trade);
    assert_apart(&wide, WIDE_PX);
    assert!(
        has(&wide, "WINZ26"),
        "a wide row still names the instrument: {wide:?}"
    );
    assert!(
        has(&wide, "211980.5 → 211855"),
        "and its whole route: {wide:?}"
    );
}

#[test]
fn a_day_header_at_the_docked_width_keeps_its_count_and_money() {
    let date = CivilDate::from_ms(CLOSED_MS, TzOffset::new(0));
    let net = (Decimal::new(11_520, 2), DAY_NET.to_owned());
    let paint = |width| painted_text(width, |ui| _ = draw_day_header(ui, date, 11, &net, false));

    let narrow = paint(NARROW_PX - 60.0);
    assert_apart(&narrow, NARROW_PX - 60.0);
    assert!(
        has(&narrow, "11 · +115.20 BRL"),
        "the count and money net stay: {narrow:?}"
    );

    let docked = paint(NARROW_PX);
    assert_apart(&docked, NARROW_PX);

    let wide = paint(WIDE_PX);
    assert!(
        has(&wide, &date.long()),
        "a wide header keeps the long date: {wide:?}"
    );
    assert!(
        has(&wide, &format!("11 · {DAY_NET}")),
        "and the whole net: {wide:?}"
    );
}

#[test]
fn a_session_row_at_the_docked_width_elides_its_name_not_its_net() {
    let head = SessionHead {
        key: SessionKey::parse("WINZ26/2026-10-21-replay.csv").unwrap(),
        label: "morning replay of the opening drive".to_owned(),
        source: Some(SessionSource::Replay),
        count: 11,
        net: (Decimal::new(11_520, 2), DAY_NET.to_owned()),
        folded: false,
        focused: false,
        live: false,
    };
    let texts = painted_text(NARROW_PX, |ui| _ = draw_session_head(ui, &head));
    assert_apart(&texts, NARROW_PX);
    assert!(
        texts.iter().any(|(text, _)| text.ends_with('…')),
        "the name gives way, visibly: {texts:?}"
    );
}

#[test]
fn the_totals_strip_at_the_docked_width_keeps_its_net() {
    let mut totals = LedgerTotals::default();
    totals.trades = 76;
    totals.wins = 67;
    let page = LedgerPage {
        shown: 50,
        remaining: 26,
    };
    let net = "+5056.20 BRL · +9112.5 pts/ct";
    let texts = painted_text(NARROW_PX, |ui| {
        draw_totals_strip(ui, (net.to_owned(), Decimal::ONE), &totals, page, 50);
    });
    assert_apart(&texts, NARROW_PX);
    assert!(
        has(&texts, net) || has(&texts, "+5056.20 BRL"),
        "the net is never cut: {texts:?}"
    );
}
