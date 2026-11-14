// The `paper_report.rs` unit tests, moved out of the file so a session
// opening the report no longer reads 1,305 lines of tests it did not ask
// for.
//
// They stay a child module of `crate::paper_report` rather than moving to an
// integration test: a child sees its ancestor's private items, so the move
// widens no visibility in production code, and the `use super::*` below is
// the line the module already had inline.

use eframe::egui;
use quantick_engine::Side;
use quantick_sim::{ExitReason, history};
use rust_decimal::Decimal;

// The items the split gave owners; `super::*` still carries the state
// itself, the environment types and the module's constants.
use super::*;
use crate::paper_calendar::CivilDate;
use crate::paper_trading::PaperTrading;
use quantick_paper::pnl::PnlReader;
use quantick_paper::report::ledger_rows::trade_detail as ledger_detail;

mod ledger_keys_tests;
mod narrow_ledger_tests;
mod scroll_tests;

/// A closed trade that netted `pnl` points, closing at `closed_ms`.
fn trade_at(closed_ms: i64, pnl: i64) -> ClosedTrade {
    ClosedTrade {
        side: Side::Buy,
        quantity: Decimal::ONE,
        entry_price: Decimal::from(100),
        exit_price: Decimal::from(100 + pnl),
        opened_ms: closed_ms - 1000,
        closed_ms,
        pnl_points: Decimal::from(pnl),
        exit_reason: ExitReason::Manual,
        entry_agg_id: None,
        exit_agg_id: None,
        mae_points: None,
        mfe_points: None,
    }
}

fn row(symbol: &str, source: Option<history::SessionSource>, trade: ClosedTrade) -> HistoryRow {
    HistoryRow {
        symbol: symbol.to_owned(),
        source,
        trade_ref: history::TradeRefs::default().next(&trade),
        trade,
        session: None,
        tags: Vec::new(),
    }
}

/// The ledger lists the chart's instrument, one the trader names, or
/// all of them — and the picker's label always says which.
#[test]
fn the_ledger_lists_the_chart_a_named_market_or_all_of_them() {
    assert_eq!(LedgerScope::Chart.folder("BTCUSDT"), Some("BTCUSDT"));
    assert_eq!(
        LedgerScope::Symbol("WINV26".to_owned()).folder("BTCUSDT"),
        Some("WINV26"),
        "a named market does not follow the chart"
    );
    assert_eq!(LedgerScope::All.folder("BTCUSDT"), None, "the whole folder");
    assert_eq!(LedgerScope::Chart.label("BTCUSDT"), "This chart · BTCUSDT");
    assert_eq!(
        LedgerScope::Chart.label(""),
        "This chart",
        "before a feed settles there is no market to name"
    );
    assert_eq!(
        LedgerScope::Symbol("WINV26".to_owned()).label("BTCUSDT"),
        "WINV26"
    );
    assert_eq!(LedgerScope::All.label("BTCUSDT"), "All symbols");
}

/// Everything a trader must be able to read off one ledger row: which
/// market, when on the clock, how long, and why it ended — plus the
/// date, which rides the row's right-hand stamp rather than the detail
/// line so the two can only ever collide where the elision shows.
#[test]
fn a_ledger_row_names_its_market_its_clock_its_age_and_its_ending() {
    let utc = TzOffset::new(0);
    let mut trade = trade_at(1_773_666_068_000, -25);
    trade.opened_ms = trade.closed_ms - 246_000;
    trade.exit_reason = quantick_sim::ExitReason::TakeProfit;
    // The detail line spends every character it has on the reason:
    // the instrument rides the head line and the date the right-hand
    // stamp, precisely so "take profit" is never cut to "take prof…".
    assert_eq!(
        ledger_detail(&trade, utc),
        "13:01:08 · 4m 06s · take profit"
    );
    assert_eq!(
        CivilDate::from_ms(trade.closed_ms, utc).short(),
        "16 Mar",
        "the stamp opposite the detail carries the date"
    );
    assert_eq!(
        CivilDate::from_ms(trade.closed_ms, utc).long(),
        "Mon 16 Mar 2026",
        "and the day header above it carries the year"
    );
    // The display timezone moves the clock and the stamp together.
    assert_eq!(
        ledger_detail(&trade, TzOffset::new(-180)),
        "10:01:08 · 4m 06s · take profit"
    );
    assert_eq!(
        CivilDate::from_ms(trade.closed_ms, TzOffset::new(-180)).short(),
        "16 Mar"
    );
}

/// A refusal the report raises reaches the window's one toast.
///
/// The typed-period field answers a value it cannot read with an
/// acknowledgement - "a typed `2d` must never do nothing quietly". When
/// the report lived on the host that was one call to `show_toast`; now
/// it is an outbox, a `ReportResponse` and a host that has to forward
/// it, which is three places for the message to die. It died in the
/// first draft of exactly that seam, and no test in the suite noticed,
/// so this is the test that would have.
///
/// Asserted with the window *shut*, which is the harder half: the
/// message is raised, the trader closes the window, and the refusal
/// they earned must still arrive rather than leaving with the thing
/// that raised it.
#[test]
fn a_refusal_the_report_raises_reaches_the_windows_one_toast() {
    let mut paper = PaperTrading::new();
    paper
        .report_state_mut()
        .show_toast("SIM: could not read `2x` as a period".to_owned());

    let ctx = egui::Context::default();
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        paper.draw_report_window(ctx, TzOffset::new(0));
    });

    assert_eq!(
        paper.take_toast().as_deref(),
        Some("SIM: could not read `2x` as a period"),
        "the report's refusal must reach the window's acknowledgement lane"
    );
    assert_eq!(
        paper.take_toast(),
        None,
        "and once only - the outbox is a slot that is handed over, not a copy"
    );
}

#[test]
fn paper_report_filter_input_recuts_before_dependent_paint() {
    let ctx = egui::Context::default();
    let mut report = ReportState::default();
    report.update(Intent::Open("X".to_owned()));
    report.update(Intent::LoadedReport(LoadedReport {
        history: LoadedHistory {
            rows: vec![
                row("X", Some(history::SessionSource::Live), trade_at(1000, 5)),
                row(
                    "X",
                    Some(history::SessionSource::Replay),
                    trade_at(2000, 19),
                ),
            ],
            files: 2,
            unreadable_files: 0,
            problem_rows: 0,
            sessions: Default::default(),
        },
        symbols: vec!["X".to_owned()],
    }));
    let env = ReportEnv {
        symbol: "X",
        dir: std::path::Path::new("."),
        session_journal_paths: &[],
        session_trades: &[],
        session_trade_at: &[],
        session_trade_sources: &[],
        open: None,
        pnl: PnlReader::plain(),
    };
    let frame = |report: &mut ReportState, events| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1600.0, 1000.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                report.draw_window(ctx, TzOffset::new(0), &env);
            },
        )
    };
    let mut output = frame(&mut report, vec![]);
    for _ in 0..3 {
        output = frame(&mut report, vec![]);
    }
    let pick = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.text() == "Replay" => {
                Some(text.pos + text.galley.rect.center().to_vec2())
            }
            _ => None,
        })
        .expect("the real Source pill is painted");
    assert_eq!(
        report.snapshot().unwrap().report.net_points,
        Decimal::from(5)
    );
    for pressed in [true, false] {
        output = frame(
            &mut report,
            vec![
                egui::Event::PointerMoved(pick),
                egui::Event::PointerButton {
                    pos: pick,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
    assert_eq!(report.snapshot().unwrap().source, SourceFilter::Replay);
    assert_eq!(
        report.snapshot().unwrap().report.net_points,
        Decimal::from(19)
    );
    let net = &report.report_view().as_ref().unwrap().net_text.0;
    assert!(
        output.shapes.iter().any(|shape| matches!(&shape.shape,
        egui::Shape::Text(text) if text.galley.text() == net)),
        "the same release frame paints the newly cut total"
    );
}

#[test]
fn paper_report_immediate_journal_and_export_paths_use_nested_model() {
    let dir = crate::scratch::ScratchDir::new("paper-report-immediate-export");
    let mut paper = PaperTrading::new();
    paper.redirect_history_dir(dir.path().to_path_buf());
    paper.set_symbol("X");
    let saved = history::write_header("X", history::SessionSource::Live)
        + "1000,2000,long,1,100,105,5,manual,,,,\n";
    dir.write("X/saved.csv", &saved);
    {
        let (report, env) = paper.report_parts();
        report.open(&env);
    }
    let print = |id, price| quantick_engine::Trade {
        agg_id: id,
        timestamp_ms: 10_000 + id as i64 * 1000,
        price: Decimal::from(price),
        quantity: Decimal::ONE,
        side: Side::Buy,
    };
    paper.seed(&print(0, 100));
    paper.market(Side::Buy);
    paper.on_trade(&print(1, 100));
    paper.close_position();
    paper.on_trade(&print(2, 107));
    assert!(paper.position_summary().is_none());
    assert_eq!(
        paper.report_state().report().as_ref().unwrap().rows.len(),
        2,
        "the immediate account event refreshes an open report without drawing it"
    );
    assert!(
        paper.report_state().saved_rows_loaded().is_none(),
        "the ledger has never painted"
    );
    let _ = paper.take_toast();
    paper.account_mut().start_export();
    assert_eq!(
        paper.report_state().saved_rows_loaded().unwrap().len(),
        1,
        "export loaded saved history and excluded the live session's own journal"
    );
    let mut note = None;
    for _ in 0..500 {
        paper.account_mut().poll_export();
        note = paper.take_toast();
        if note.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let note = note.expect("the existing export worker returns its receipt");
    assert!(
        note.contains("2 trade"),
        "export includes both halves: {note}"
    );
    let exported = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(Result::ok)
        .find(|entry| entry.file_name().to_string_lossy().starts_with("export-"))
        .expect("export file");
    let text = std::fs::read_to_string(exported.path()).unwrap();
    assert!(text.contains("2000"), "saved close is present");
    assert!(
        text.contains("12000"),
        "current close is present exactly once"
    );
    assert_eq!(
        text.lines().filter(|line| line.contains("12000")).count(),
        1
    );
}
