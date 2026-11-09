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

use super::super::ledger::draw_totals_strip;
use super::super::rows::{TradeMeta, draw_day_header, draw_ledger_row};
use super::super::sessions::{SessionHead, draw_session_head};
use crate::paper_calendar::CivilDate;
use crate::timezone::TzOffset;
use quantick_paper::report::ledger_rows::LedgerPage;

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
    painted_text_with_theme(width, None, &mut draw)
}

fn painted_text_with_theme(
    width: f32,
    source: Option<&str>,
    mut draw: impl FnMut(&mut egui::Ui),
) -> Vec<(String, egui::Rect)> {
    let ctx = egui::Context::default();
    if let Some(source) = source {
        let mut appearance = quantick_appearance::Appearance::default();
        appearance.apply(source).unwrap();
        crate::theme::sync(&ctx, &appearance);
    }
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
                overlap.width() <= 0.0 || overlap.height() <= 0.0 || struck(a, a_rect, b, b_rect),
                "{a:?} at {a_rect:?} paints over {b:?} at {b_rect:?}"
            );
        }
    }
}

/// A bold word is the same text painted again under a pixel to its right.
fn struck(a: &str, a_rect: &egui::Rect, b: &str, b_rect: &egui::Rect) -> bool {
    a == b
        && (a_rect.size() - b_rect.size()).length() < 0.01
        && (a_rect.min - b_rect.min).length() < 1.0
}

fn has(texts: &[(String, egui::Rect)], wanted: &str) -> bool {
    texts.iter().any(|(text, _)| text.contains(wanted))
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
        has(&narrow, "+115.20 BRL"),
        "the count and money net stay: {narrow:?}"
    );

    let docked = paint(NARROW_PX);
    assert_apart(&docked, NARROW_PX);

    let wide = paint(WIDE_PX);
    assert!(
        has(&wide, &date.long()),
        "a wide header keeps the long date: {wide:?}"
    );
    assert!(has(&wide, DAY_NET), "and the whole net: {wide:?}");
}

#[test]
fn a_session_row_at_the_docked_width_wraps_its_complete_name_and_net() {
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
        has(&texts, &head.label) && has(&texts, DAY_NET),
        "the name and calculation stay complete: {texts:?}"
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

#[test]
fn enlarged_light_trade_rows_keep_primary_text_apart() {
    let source = format!(
        "{}\n[typography.monospace]\nsize=24.0",
        quantick_appearance::LIGHT
    );
    assert_eq!(
        quantick_appearance::ResolvedTheme::parse(&source)
            .unwrap()
            .typography
            .get(quantick_appearance::FontRole::Monospace)
            .size,
        24.0
    );
    let trade = short_four();
    let tags = [];
    let meta = TradeMeta {
        session: None,
        trade_ref: TradeRef::default(),
        source: Some(SessionSource::Replay),
        tags: &tags,
        pnl: PnlReader::plain().of("WINZ26"),
    };
    for width in [240.0, 300.0, 640.0] {
        let texts = painted_text_with_theme(width, Some(&source), |ui| {
            draw_ledger_row(
                ui,
                &trade,
                meta,
                Some("WINZ26"),
                false,
                false,
                TzOffset::new(0),
            );
        });
        assert_apart(&texts, width);
    }
}

#[test]
fn paper_values_and_actions_fit_narrow_panels_report_tiles() {
    for source in [None, Some(quantick_appearance::LIGHT)] {
        for width in [320.0, 400.0, 520.0] {
            let mut state = super::super::ReportState {
                report: Some(super::super::LoadedHistory {
                    rows: vec![super::row(
                        "LONG-INSTRUMENT-USDT",
                        None,
                        super::trade_at(CLOSED_MS, 123456789),
                    )],
                    files: 1,
                    unreadable_files: 0,
                    problem_rows: 0,
                    sessions: Default::default(),
                }),
                ..Default::default()
            };
            state.ensure_report_view(TzOffset::new(0), PnlReader::plain());
            let view = state.report_view.as_ref().unwrap();
            let texts = painted_text_with_theme(width, source, |ui| {
                super::super::tables::draw_report_tiles(ui, view);
            });
            assert_apart(&texts, width);
            assert!(texts.iter().any(|(text, _)| text == &view.net_text.0));
            for label in ["NET", "WIN RATE", "PROFIT FACTOR"] {
                assert!(
                    texts.iter().any(|(text, _)| text == label),
                    "missing {label}: {texts:?}"
                );
            }
        }
    }
}

#[test]
fn measured_report_warm_frames_only_format_visible_rows() {
    use super::super::tables::{FORMATTED_ROWS, draw_trade_list};
    for count in [100, 1000] {
        let ctx = egui::Context::default();
        let mut state = super::super::ReportState {
            report: Some(super::super::LoadedHistory {
                rows: (0..count)
                    .map(|i| {
                        super::row(
                            "LONG-INSTRUMENT-USDT",
                            None,
                            super::trade_at(CLOSED_MS + i, i),
                        )
                    })
                    .collect(),
                files: 1,
                unreadable_files: 0,
                problem_rows: 0,
                sessions: Default::default(),
            }),
            ..Default::default()
        };
        state.ensure_report_view(TzOffset::new(0), PnlReader::plain());
        for frame in 0..4 {
            FORMATTED_ROWS.with(|value| value.set(0));
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(520.0, 400.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        draw_trade_list(
                            ui,
                            state.report_view.as_ref().unwrap(),
                            true,
                            &mut state.report_table_layout,
                        );
                    });
                },
            );
            let visited = FORMATTED_ROWS.with(std::cell::Cell::get);
            if frame == 0 {
                assert!(visited >= count as usize);
            } else {
                assert!(visited < 25, "warm frame formats {visited} of {count}");
            }
        }
    }
}
