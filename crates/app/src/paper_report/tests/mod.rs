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
use super::ledger::{LedgerPage, LedgerTotals};
use super::rows::{LedgerRow, elide_tail, ledger_detail, push_by_day};
use super::window::ReportPeriod;
use super::*;
use crate::paper_calendar::{CivilDate, DateRange, DaySelection};
use crate::paper_trading::PaperTrading;

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
        trade,
    }
}

#[test]
fn the_report_opens_on_real_and_keeps_replay_out_until_asked() {
    let utc = TzOffset::new(0);
    let day = 86_400_000_i64;
    let mut paper = PaperTrading::new();
    assert_eq!(
        paper.report_state().report_source,
        SourceFilter::Real,
        "practice runs never inflate the real track record unasked"
    );
    paper.report_state_mut().report = Some(LoadedHistory {
        rows: vec![
            row("X", Some(history::SessionSource::Live), trade_at(day, 5)),
            row("X", None, trade_at(2 * day, 3)),
            row(
                "X",
                Some(history::SessionSource::Replay),
                trade_at(3 * day, 100),
            ),
        ],
        files: 3,
        unreadable_files: 0,
        problem_rows: 0,
    });
    paper.report_state_mut().ensure_report_view(utc);
    let view = paper
        .report_state()
        .report_view
        .as_ref()
        .expect("view built");
    assert_eq!(view.rows.len(), 2, "live + unrecorded-legacy count as real");
    assert_eq!(
        view.hidden_by_source, 1,
        "the replay trade sits behind the filter"
    );
    assert_eq!(
        view.anchor_ms,
        Some(2 * day),
        "the anchor comes from the filtered scope, not the replay trade"
    );
    assert_eq!(view.report.net_points, Decimal::from(8));

    paper.report_state_mut().report_source = SourceFilter::Replay;
    paper.report_state_mut().ensure_report_view(utc);
    let view = paper.report_state().report_view.as_ref().expect("rebuilt");
    assert_eq!(view.rows.len(), 1, "the practice run, alone");
    assert_eq!(view.report.net_points, Decimal::from(100));
    assert_eq!(view.hidden_by_source, 2);

    paper.report_state_mut().report_source = SourceFilter::All;
    paper.report_state_mut().ensure_report_view(utc);
    let view = paper.report_state().report_view.as_ref().expect("rebuilt");
    assert_eq!(view.rows.len(), 3, "All mixes on purpose");
    assert_eq!(view.hidden_by_source, 0);
}

/// The report answers as data, not only as pixels: the snapshot names
/// the window in force and hands back the very rows the tiles, the
/// curve and the list were computed from. An operator that cannot see
/// the screen reads this.
#[test]
fn the_report_reports_itself_as_data() {
    let tz = TzOffset::new(-180);
    let day = CivilDate::from_ymd(2026, 8, 17);
    let mut paper = PaperTrading::new();
    assert!(
        paper.report_state().snapshot().is_none(),
        "nothing loaded, nothing to report"
    );
    paper.report_state_mut().report = Some(LoadedHistory {
        rows: vec![
            row("WINV26", None, trade_at(day.start_ms(tz) + 60_000, 5)),
            row(
                "WINV26",
                Some(history::SessionSource::Replay),
                trade_at(day.start_ms(tz) + 120_000, 9),
            ),
            row(
                "WINV26",
                None,
                trade_at(day.offset_days(1).start_ms(tz) + 60_000, -3),
            ),
        ],
        files: 1,
        unreadable_files: 0,
        problem_rows: 0,
    });
    paper.report_state_mut().report_symbol = Some("WINV26".to_owned());

    // The pills in force, no dates picked.
    paper.report_state_mut().report_period = ReportPeriod::All;
    paper.report_state_mut().ensure_report_view(tz);
    let snapshot = paper.report_state().snapshot().expect("a snapshot");
    assert_eq!(snapshot.symbol, Some("WINV26"));
    assert_eq!(snapshot.source, SourceFilter::Real);
    assert_eq!(snapshot.window, ReportWindow::Period(ReportPeriod::All));
    assert_eq!(snapshot.rows.len(), 2, "the practice run is filtered out");
    assert_eq!(snapshot.hidden_by_source, 1);
    assert_eq!(snapshot.report.net_points, Decimal::from(2));
    assert_eq!(snapshot.window.label(), "everything saved");

    // The named action a script would call, then the same read-back.
    paper
        .report_state_mut()
        .pick_report_dates(DaySelection::None.click(day));
    paper.report_state_mut().ensure_report_view(tz);
    let snapshot = paper.report_state().snapshot().expect("a snapshot");
    assert_eq!(
        snapshot.window,
        ReportWindow::Dates(DateRange {
            start: day,
            end: day
        }),
        "a range and a period are never both in force"
    );
    assert_eq!(snapshot.rows.len(), 1);
    assert_eq!(snapshot.rows[0].symbol, "WINV26");
    assert_eq!(snapshot.hidden_outside, 1, "the next day sits outside");
    assert_eq!(snapshot.window.label(), day.iso(), "and it names itself");
    // The numbers the tiles show come from the very rows handed back.
    let net: Decimal = snapshot
        .rows
        .iter()
        .map(|row| row.trade.pnl_points)
        .sum::<Decimal>();
    assert_eq!(snapshot.report.net_points, net);
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

/// Folding is per civil day and reversible, and the "fold everything"
/// control reports honestly whether it has anything left to fold.
#[test]
fn days_fold_shut_one_at_a_time_or_all_at_once() {
    let tz = TzOffset::new(-180);
    let day = CivilDate::from_ymd(2026, 8, 17);
    let mut paper = PaperTrading::new();
    paper.report_state_mut().ledger_tz = tz;
    paper.report_state_mut().history_cache = Some(LoadedHistory {
        rows: vec![
            row("X", None, trade_at(day.start_ms(tz) + 60_000, 5)),
            row("X", None, trade_at(day.end_ms(tz) - 1, -2)),
            row("X", None, trade_at(day.offset_days(1).start_ms(tz), 7)),
        ],
        files: 1,
        unreadable_files: 0,
        problem_rows: 0,
    });
    assert!(
        !{
            let (state, env) = paper.report_parts();
            state.all_days_collapsed(&env)
        },
        "nothing is folded to begin with"
    );

    paper.report_state_mut().set_day_collapsed(day, true);
    assert!(
        paper
            .report_state()
            .collapsed_days
            .contains(&day.day_number())
    );
    assert!(
        !{
            let (state, env) = paper.report_parts();
            state.all_days_collapsed(&env)
        },
        "the next day is still open, so the control still offers to fold"
    );

    {
        let (state, env) = paper.report_parts();
        state.toggle_all_days(false, &env)
    };
    assert!(
        {
            let (state, env) = paper.report_parts();
            state.all_days_collapsed(&env)
        },
        "both days shut"
    );
    assert_eq!(paper.report_state().collapsed_days.len(), 2);

    {
        let (state, env) = paper.report_parts();
        state.toggle_all_days(true, &env)
    };
    assert!(paper.report_state().collapsed_days.is_empty());
    assert!(!{
        let (state, env) = paper.report_parts();
        state.all_days_collapsed(&env)
    });

    // An empty ledger has nothing folded *and* nothing to fold: the
    // control must not claim everything is already shut.
    let mut empty = PaperTrading::new();
    assert!(!{
        let (state, env) = empty.report_parts();
        state.all_days_collapsed(&env)
    });
}

/// The rows the ledger builds each frame are bounded by the revealed
/// page, and so is every other per-frame pass it makes. A trader with
/// a year of sessions must pay the same per-frame cost as one with a
/// week — which means the totals strip cannot walk the history either.
#[test]
fn the_ledger_builds_a_bounded_number_of_rows_however_deep_the_history() {
    let tz = TzOffset::new(-180);
    let day = CivilDate::from_ymd(2026, 8, 17);
    // Five thousand saved trades over fifty days.
    let saved: Vec<ClosedTrade> = (0..5_000)
        .map(|index| {
            trade_at(
                day.offset_days(-(index / 100)).start_ms(tz) + (index % 100) * 60_000,
                1,
            )
        })
        .collect();
    let items: Vec<(&str, &ClosedTrade)> = saved.iter().map(|trade| ("WINV26", trade)).collect();

    let page = LedgerPage::of(items.len(), 1);
    assert_eq!(page.shown, LEDGER_PAGE_TRADES);
    assert_eq!(page.remaining, 4_950, "and the control says so out loud");
    let mut rows = Vec::new();
    push_by_day(
        &mut rows,
        &items[..page.shown],
        tz,
        &std::collections::BTreeSet::new(),
        |item| item.1,
        |item| LedgerRow::Earlier(item.0, item.1),
    );
    // Fifty trades plus at most one day caption each — nowhere near
    // the five thousand rows the pre-paging ledger would have built.
    assert!(
        rows.len() <= 2 * LEDGER_PAGE_TRADES,
        "{} rows for a page of {LEDGER_PAGE_TRADES}",
        rows.len()
    );
    // Revealing pages grows the list by one page at a time, never all
    // at once.
    let deeper = LedgerPage::of(items.len(), 2);
    assert_eq!(deeper.shown - page.shown, LEDGER_PAGE_TRADES);

    // And the strip under the list is summed with the load, not on the
    // frame: the totals are a stored value, so the frame reads them
    // rather than walking five thousand trades to print one line.
    let totals = LedgerTotals::of(saved.iter());
    assert_eq!(totals.trades, 5_000);
    assert_eq!(totals.wins, 5_000, "every fixture trade is a winner");
    assert_eq!(totals.net, Decimal::from(5_000));
    assert_eq!(totals.win_rate(), Some(100));
    // Nothing saved plus this session's own trades still adds up.
    assert_eq!(
        LedgerTotals::default().plus(totals),
        totals,
        "an empty half must be the identity"
    );
    assert_eq!(LedgerTotals::default().win_rate(), None, "0/0 is not 0%");
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

/// A detail line that outgrows its share of the row is cut with an
/// ellipsis, never clipped mid-glyph: a shortened "take prof" must not
/// be readable as a complete exit reason.
#[test]
fn a_detail_line_too_long_for_its_row_is_elided_not_clipped() {
    assert_eq!(elide_tail("take profit", 11), "take profit");
    assert_eq!(elide_tail("take profit", 12), "take profit");
    assert_eq!(elide_tail("take profit", 6), "take …");
    assert_eq!(elide_tail("take profit", 2), "t…");
    // Below the ellipsis plus a character there is nothing honest left
    // to say, so the line says nothing.
    assert_eq!(elide_tail("take profit", 1), "");
    assert_eq!(elide_tail("take profit", 0), "");
    // Multi-byte characters are counted as characters, not bytes.
    assert_eq!(elide_tail("WINV26 · 13:01", 8), "WINV26 …");
}

/// The ledger reveals saved history one page at a time and states how
/// much it is holding back — a list that simply ends looks like the
/// end of the history, which is the confusion the control exists for.
#[test]
fn the_ledger_reveals_saved_history_one_page_at_a_time() {
    let page = LedgerPage::of(120, 1);
    assert_eq!(page.shown, LEDGER_PAGE_TRADES);
    assert_eq!(page.remaining, 120 - LEDGER_PAGE_TRADES);
    let page = LedgerPage::of(120, 2);
    assert_eq!(page.shown, 2 * LEDGER_PAGE_TRADES);
    assert_eq!(page.remaining, 120 - 2 * LEDGER_PAGE_TRADES);
    // The last page shows the tail and offers nothing more.
    let page = LedgerPage::of(120, 3);
    assert_eq!(page.shown, 120);
    assert_eq!(page.remaining, 0);
    let page = LedgerPage::of(120, 99);
    assert_eq!(page.shown, 120, "extra pages cannot invent trades");
    assert_eq!(page.remaining, 0);
    // A short history fits in one page and never offers "show older".
    let page = LedgerPage::of(7, 1);
    assert_eq!((page.shown, page.remaining), (7, 0));
    // Page zero is treated as one: the ledger always shows something.
    assert_eq!(LedgerPage::of(7, 0), LedgerPage::of(7, 1));
    assert_eq!(
        LedgerPage::of(0, 1),
        LedgerPage {
            shown: 0,
            remaining: 0
        }
    );
}

/// Rows are grouped under the civil day they closed on, and each day's
/// caption carries that day's own count and net.
#[test]
fn ledger_rows_open_a_new_day_caption_when_the_day_changes() {
    let tz = TzOffset::new(-180);
    let day = CivilDate::from_ymd(2026, 8, 17);
    // Newest first, the order the ledger cuts in.
    let items = [
        (
            "WINV26",
            trade_at(day.offset_days(1).start_ms(tz) + 3_600_000, 8),
        ),
        ("WINV26", trade_at(day.end_ms(tz) - 1, -25)),
        ("WINV26", trade_at(day.start_ms(tz) + 60_000, 139)),
    ];
    let items: Vec<(&str, &ClosedTrade)> = items
        .iter()
        .map(|(symbol, trade)| (*symbol, trade))
        .collect();
    let mut rows = Vec::new();
    push_by_day(
        &mut rows,
        &items,
        tz,
        &std::collections::BTreeSet::new(),
        |item| item.1,
        |item| LedgerRow::Earlier(item.0, item.1),
    );
    assert_eq!(rows.len(), 5, "two day captions over three trades");
    match rows[0] {
        LedgerRow::Day(date, count, net, folded) => {
            assert_eq!(date, day.offset_days(1));
            assert_eq!(count, 1);
            assert_eq!(net, Decimal::from(8));
            assert!(!folded);
        }
        _ => panic!("the list opens on a day caption"),
    }
    assert!(matches!(rows[1], LedgerRow::Earlier(..)));
    match rows[2] {
        LedgerRow::Day(date, count, net, _) => {
            assert_eq!(date, day);
            assert_eq!(count, 2, "both of the 17th's trades");
            assert_eq!(net, Decimal::from(114), "and what the day netted");
        }
        _ => panic!("the next day opens its own caption"),
    }
    assert!(matches!(rows[3], LedgerRow::Earlier(..)));
    assert!(matches!(rows[4], LedgerRow::Earlier(..)));

    // Folded, the 17th keeps its caption — with its count and its net
    // intact — and contributes no trade rows at all.
    let mut folded_days = std::collections::BTreeSet::new();
    folded_days.insert(day.day_number());
    let mut rows = Vec::new();
    push_by_day(
        &mut rows,
        &items,
        tz,
        &folded_days,
        |item| item.1,
        |item| LedgerRow::Earlier(item.0, item.1),
    );
    assert_eq!(rows.len(), 3, "two captions and only the open day's row");
    match rows[2] {
        LedgerRow::Day(date, count, net, folded) => {
            assert_eq!(date, day);
            assert!(folded, "and it says it is folded");
            assert_eq!(count, 2, "a folded day still counts its trades");
            assert_eq!(net, Decimal::from(114), "and still states its net");
        }
        _ => panic!("a folded day keeps its caption"),
    }
}

/// A picked day cuts the report to that civil day, and a picked span
/// to both its ends inclusive. The pills stand down while it holds, so
/// a chosen date can never come back empty because a forgotten pill
/// was cutting too.
#[test]
fn a_picked_calendar_range_cuts_the_report_and_the_pills_stand_down() {
    let tz = TzOffset::new(-180);
    let first = CivilDate::from_ymd(2026, 8, 12);
    let mut paper = PaperTrading::new();
    paper.report_state_mut().report = Some(LoadedHistory {
        rows: vec![
            row("X", None, trade_at(first.start_ms(tz) + 3_600_000, 5)),
            row(
                "X",
                None,
                trade_at(first.offset_days(3).start_ms(tz) + 60_000, -2),
            ),
            // The last millisecond of the 17th, local — inside a range
            // that ends on the 17th.
            row("X", None, trade_at(first.offset_days(5).end_ms(tz) - 1, 7)),
            row("X", None, trade_at(first.offset_days(6).start_ms(tz), 11)),
        ],
        files: 1,
        unreadable_files: 0,
        problem_rows: 0,
    });
    // A pill that would hide the older trades is deliberately left on:
    // the range must win outright, not intersect.
    paper.report_state_mut().report_period = ReportPeriod::Today;

    let picked = paper.report_state().calendar.selection.click(first);
    paper.report_state_mut().pick_report_dates(picked);
    paper.report_state_mut().ensure_report_view(tz);
    let view = paper.report_state().report_view.as_ref().expect("a view");
    assert_eq!(view.rows.len(), 1, "one day, one trade");
    assert_eq!(
        view.range.map(DateRange::label),
        Some("2026-08-12".to_owned())
    );
    assert_eq!(view.hidden_outside, 3, "and it counts what it hides");
    assert_eq!(view.report.net_points, Decimal::from(5), "the tiles agree");

    let picked = paper
        .report_state()
        .calendar
        .selection
        .click(first.offset_days(5));
    paper.report_state_mut().pick_report_dates(picked);
    paper.report_state_mut().ensure_report_view(tz);
    let view = paper.report_state().report_view.as_ref().expect("a view");
    assert_eq!(view.rows.len(), 3, "both ends of the span are inside");
    assert_eq!(
        view.range.map(DateRange::label),
        Some("2026-08-12 to 2026-08-17".to_owned())
    );
    assert_eq!(view.hidden_outside, 1, "only the 18th sits outside");
    assert_eq!(view.report.net_points, Decimal::from(10));

    // Clearing hands the window back to the pills, unchanged.
    paper
        .report_state_mut()
        .pick_report_dates(DaySelection::None);
    paper.report_state_mut().report_period = ReportPeriod::All;
    paper.report_state_mut().ensure_report_view(tz);
    let view = paper.report_state().report_view.as_ref().expect("a view");
    assert!(view.range.is_none());
    assert_eq!(view.rows.len(), 4, "every saved trade is back");
    assert_eq!(view.hidden_outside, 0);
}

/// A day the trader picked that holds nothing is answered, not hidden:
/// the view is empty, the range still names itself, and every saved
/// trade is counted as sitting outside it.
#[test]
fn a_picked_day_with_no_trades_reports_an_honest_empty() {
    let tz = TzOffset::new(0);
    let day = CivilDate::from_ymd(2026, 8, 12);
    let mut paper = PaperTrading::new();
    paper.report_state_mut().report = Some(LoadedHistory {
        rows: vec![row("X", None, trade_at(day.offset_days(2).start_ms(tz), 5))],
        files: 1,
        unreadable_files: 0,
        problem_rows: 0,
    });
    let picked = paper.report_state().calendar.selection.click(day);
    paper.report_state_mut().pick_report_dates(picked);
    paper.report_state_mut().ensure_report_view(tz);
    let view = paper.report_state().report_view.as_ref().expect("a view");
    assert!(view.rows.is_empty());
    assert_eq!(
        view.range.map(DateRange::label),
        Some("2026-08-12".to_owned())
    );
    assert_eq!(view.hidden_outside, 1, "the trade exists, just not here");
}

/// The calendar highlights days from the same trades the report would
/// show — after the Source filter, on the display timezone's clock.
#[test]
fn the_day_index_follows_the_source_filter_and_the_timezone() {
    let tz = TzOffset::new(-180);
    let day = CivilDate::from_ymd(2026, 8, 17);
    let mut paper = PaperTrading::new();
    paper.report_state_mut().report = Some(LoadedHistory {
        rows: vec![
            row(
                "X",
                Some(history::SessionSource::Live),
                trade_at(day.start_ms(tz) + 60_000, 5),
            ),
            row(
                "X",
                Some(history::SessionSource::Replay),
                trade_at(day.offset_days(1).start_ms(tz) + 60_000, 9),
            ),
        ],
        files: 1,
        unreadable_files: 0,
        problem_rows: 0,
    });
    paper.report_state_mut().ensure_report_view(tz);
    assert_eq!(
        paper.report_state().report_days.len(),
        1,
        "Real hides the practice day"
    );
    assert!(paper.report_state().report_days.stat(day).is_some());
    assert!(
        paper
            .report_state()
            .report_days
            .stat(day.offset_days(1))
            .is_none()
    );

    paper.report_state_mut().report_source = SourceFilter::All;
    paper.report_state_mut().ensure_report_view(tz);
    assert_eq!(
        paper.report_state().report_days.len(),
        2,
        "Both lights up both days"
    );

    // The same trades, read on another clock, land on other days.
    paper
        .report_state_mut()
        .ensure_report_view(TzOffset::new(0));
    assert_eq!(paper.report_state().report_days.len(), 2);
    assert!(
        paper
            .report_state()
            .report_days
            .stat(CivilDate::from_ymd(2026, 8, 17))
            .is_some(),
        "03:01 UTC on the 17th is still the 17th in UTC"
    );
}

#[test]
fn the_report_view_filters_by_period_from_the_newest_trade() {
    let utc = TzOffset::new(0);
    let day = 86_400_000_i64;
    let mut paper = PaperTrading::new();
    paper.report_state_mut().report = Some(LoadedHistory {
        rows: vec![
            row("X", None, trade_at(10 * day, 5)),
            row("X", None, trade_at(18 * day, -2)),
            row("X", None, trade_at(20 * day + 3_600_000, 7)),
        ],
        files: 1,
        unreadable_files: 0,
        problem_rows: 0,
    });
    paper.report_state_mut().report_period = ReportPeriod::Week;
    paper.report_state_mut().ensure_report_view(utc);
    let view = paper
        .report_state()
        .report_view
        .as_ref()
        .expect("view built");
    assert_eq!(
        view.anchor_ms,
        Some(20 * day + 3_600_000),
        "anchored to the newest saved trade, not a clock"
    );
    assert_eq!(view.rows.len(), 2, "the 10-day-old trade is outside 7d");
    assert_eq!(view.hidden_outside, 1, "and the view counts what it hides");
    assert_eq!(view.report.net_points, Decimal::from(5));

    paper.report_state_mut().report_period = ReportPeriod::All;
    paper.report_state_mut().ensure_report_view(utc);
    assert_eq!(
        paper
            .report_state()
            .report_view
            .as_ref()
            .expect("rebuilt")
            .rows
            .len(),
        3,
        "All sees everything again"
    );
}

#[test]
fn all_symbols_hides_older_markets_but_says_so_and_scope_restores_them() {
    let utc = TzOffset::new(0);
    let day = 86_400_000_i64;
    let mut paper = PaperTrading::new();
    paper.report_state_mut().report = Some(LoadedHistory {
        rows: vec![
            row("OLDSYM", None, trade_at(day, 5)),
            row("NEWSYM", None, trade_at(60 * day, 7)),
        ],
        files: 2,
        unreadable_files: 0,
        problem_rows: 0,
    });
    paper.report_state_mut().report_period = ReportPeriod::Month;
    paper.report_state_mut().ensure_report_view(utc);
    let view = paper
        .report_state()
        .report_view
        .as_ref()
        .expect("view built");
    assert_eq!(
        view.rows.len(),
        1,
        "30d back from the newest trade hides the older market entirely"
    );
    assert_eq!(view.hidden_outside, 1, "the support line can say so");

    // Narrowing the combo re-anchors on the old market's own newest
    // trade — the "my trades came back" behaviour, now spelled out.
    paper.report_state_mut().report = Some(LoadedHistory {
        rows: vec![row("OLDSYM", None, trade_at(day, 5))],
        files: 1,
        unreadable_files: 0,
        problem_rows: 0,
    });
    paper.report_state_mut().report_view = None;
    paper.report_state_mut().ensure_report_view(utc);
    let view = paper.report_state().report_view.as_ref().expect("rebuilt");
    assert_eq!(view.rows.len(), 1, "its own scope shows the old market");
    assert_eq!(view.hidden_outside, 0);
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
