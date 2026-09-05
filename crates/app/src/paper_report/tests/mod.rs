// The `paper_report.rs` unit tests, moved out of the file so a session
// opening the report no longer reads 1,305 lines of tests it did not ask
// for.
//
// They stay a child module of `crate::paper_report` rather than moving to an
// integration test: a child sees its ancestor's private items, so the move
// widens no visibility in production code, and the `use super::*` below is
// the line the module already had inline.

use quantick_sim::ExitReason;
use rust_decimal::Decimal;

use super::*;
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

#[test]
fn report_periods_anchor_to_the_given_trade_never_a_clock() {
    let utc = TzOffset::new(0);
    // 2026-03-16 13:01:08 UTC.
    let anchor = 1_773_666_068_000_i64;
    assert_eq!(ReportPeriod::All.cutoff_ms(anchor, utc), None);
    assert_eq!(
        ReportPeriod::Today.cutoff_ms(anchor, utc),
        Some(1_773_619_200_000),
        "the anchor's own UTC midnight"
    );
    assert_eq!(
        ReportPeriod::Week.cutoff_ms(anchor, utc),
        Some(anchor - 7 * 86_400_000)
    );
    assert_eq!(
        ReportPeriod::Custom(2 * 86_400_000).cutoff_ms(anchor, utc),
        Some(anchor - 2 * 86_400_000),
        "a typed 2d reaches back exactly two days"
    );
}

#[test]
fn today_breaks_at_the_displayed_midnight_not_utc() {
    let day = 86_400_000_i64;
    // 03 Jan 01:00 UTC is 02 Jan 22:00 in UTC-03:00.
    let anchor = 2 * day + 3_600_000;
    let sao_paulo = TzOffset::new(-180);
    assert_eq!(
        ReportPeriod::Today.cutoff_ms(anchor, sao_paulo),
        Some(day + 10_800_000),
        "midnight of the anchor's displayed day, expressed in UTC"
    );
    assert_eq!(
        ReportPeriod::Today.cutoff_ms(anchor, TzOffset::new(0)),
        Some(2 * day),
        "UTC viewers keep the UTC midnight"
    );
}

#[test]
fn typed_periods_parse_strictly_and_format_back() {
    assert_eq!(parse_period("2d"), Some(2 * 86_400_000));
    assert_eq!(parse_period(" 3D "), Some(3 * 86_400_000));
    assert_eq!(parse_period("12h"), Some(12 * 3_600_000));
    assert_eq!(parse_period("45m"), Some(45 * 60_000));
    assert_eq!(parse_period("1w"), Some(7 * 86_400_000));
    for refused in ["", "d", "2", "2x", "0d", "-2d", "2.5d", "d2"] {
        assert_eq!(parse_period(refused), None, "{refused:?} must be refused");
    }
    assert_eq!(fmt_period_ms(2 * 86_400_000), "2d");
    assert_eq!(fmt_period_ms(36 * 3_600_000), "36h");
    assert_eq!(fmt_period_ms(45 * 60_000), "45m");
    assert_eq!(fmt_period_ms(14 * 86_400_000), "2w");
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

/// The equity walk is cut with the view, not re-walked per frame, so
/// the curve above the list and the running total beside each trade
/// are literally the same numbers.
#[test]
fn the_equity_walk_is_cut_once_and_both_readers_share_it() {
    let rows = vec![
        row("X", None, trade_at(1_000, 5)),
        row("X", None, trade_at(2_000, -12)),
        row("X", None, trade_at(3_000, 4)),
    ];
    let walk = EquityWalk::of(&rows);
    assert_eq!(walk.points.len(), rows.len() + 1, "E_0 rides in front");
    assert_eq!(walk.points[0], Decimal::ZERO, "flat before the first trade");
    assert_eq!(walk.points[1], Decimal::from(5));
    assert_eq!(walk.points[2], Decimal::from(-7));
    assert_eq!(walk.points[3], Decimal::from(-3));
    assert_eq!(walk.plot.len(), walk.points.len());
    assert!((walk.low - (-7.0)).abs() < f32::EPSILON, "the trough");
    assert!((walk.high - 5.0).abs() < f32::EPSILON, "the peak");
    // A view with no trades still has a walk, and it is the flat one.
    let empty = EquityWalk::of(&[]);
    assert_eq!(empty.points, vec![Decimal::ZERO]);
    assert_eq!((empty.low, empty.high), (0.0, 0.0));
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

/// The expected report, byte for byte. Regenerated only when a
/// *behaviour* change is intended and argued for.
const GOLDEN_REPORT: &str = r#"== ALL ==
rows: 8
hidden_outside: 0
hidden_by_source: 1
anchor_ms: Some(1728000000)
equity.points: [0, 12, 7, 4, 4, 13, 5, 9, 15]
equity.plot: [0.0, 12.0, 7.0, 4.0, 4.0, 13.0, 5.0, 9.0, 15.0]
equity.low: 0.0  equity.high: 15.0
  report: PerformanceReport {
      trades: 8,
      wins: 4,
      losses: 3,
      scratches: 1,
      long_trades: 4,
      short_trades: 4,
      net_points: 15,
      gross_profit: 31,
      gross_loss: 16,
      win_rate_pct: Some(
          50,
      ),
      loss_rate_pct: Some(
          37.50,
      ),
      profit_factor: Some(
          1.9375,
      ),
      payoff_ratio: Some(
          1.453125000000000000000,
      ),
      expectancy_points: Some(
          1.875,
      ),
      max_drawdown_points: 8,
      max_drawdown_span: Some(
          (
              1,
              3,
          ),
      ),
      max_runup_points: 15,
      recovery_factor: Some(
          1.875,
      ),
      max_consecutive_wins: 2,
      max_consecutive_losses: 2,
      stddev_points: Some(
          7.0394,
      ),
      largest_win: 12,
      largest_loss: 8,
      avg_win: Some(
          7.75,
      ),
      avg_loss: Some(
          5.3333333333333333333333333333,
      ),
      avg_duration_ms: Some(
          2700000,
      ),
      median_duration_ms: Some(
          2100000,
      ),
      avg_win_duration_ms: Some(
          4500000,
      ),
      avg_loss_duration_ms: Some(
          1000000,
      ),
      avg_winner_mae_points: Some(
          2.50,
      ),
      winners_with_mae: 4,
      avg_loser_mfe_points: Some(
          1.50,
      ),
      losers_with_mfe: 2,
      long: SideReport {
          trades: 4,
          net_points: 3,
          win_rate_pct: Some(
              50,
          ),
          profit_factor: Some(
              1.2307692307692307692307692308,
          ),
          avg_win: Some(
              8,
          ),
          avg_loss: Some(
              6.50,
          ),
          expectancy_points: Some(
              0.75,
          ),
      },
      short: SideReport {
          trades: 4,
          net_points: 12,
          win_rate_pct: Some(
              50,
          ),
          profit_factor: Some(
              5,
          ),
          avg_win: Some(
              7.50,
          ),
          avg_loss: Some(
              3,
          ),
          expectancy_points: Some(
              3,
          ),
      },
      by_exit_reason: [
          ReasonReport {
              reason: StopLoss,
              trades: 3,
              net_points: -16,
          },
          ReasonReport {
              reason: TakeProfit,
              trades: 3,
              net_points: 27,
          },
          ReasonReport {
              reason: Manual,
              trades: 2,
              net_points: 4,
          },
      ],
  }
== WEEK ==
rows: 5
hidden_outside: 3
hidden_by_source: 1
anchor_ms: Some(1728000000)
equity.points: [0, 0, 9, 1, 5, 11]
equity.plot: [0.0, 0.0, 9.0, 1.0, 5.0, 11.0]
equity.low: 0.0  equity.high: 11.0
  report: PerformanceReport {
      trades: 5,
      wins: 3,
      losses: 1,
      scratches: 1,
      long_trades: 2,
      short_trades: 3,
      net_points: 11,
      gross_profit: 19,
      gross_loss: 8,
      win_rate_pct: Some(
          60,
      ),
      loss_rate_pct: Some(
          20,
      ),
      profit_factor: Some(
          2.375,
      ),
      payoff_ratio: Some(
          0.7916666666666666666666666667,
      ),
      expectancy_points: Some(
          2.20,
      ),
      max_drawdown_points: 8,
      max_drawdown_span: Some(
          (
              2,
              3,
          ),
      ),
      max_runup_points: 11,
      recovery_factor: Some(
          1.375,
      ),
      max_consecutive_wins: 2,
      max_consecutive_losses: 1,
      stddev_points: Some(
          6.5727,
      ),
      largest_win: 9,
      largest_loss: 8,
      avg_win: Some(
          6.3333333333333333333333333333,
      ),
      avg_loss: Some(
          8,
      ),
      avg_duration_ms: Some(
          3060000,
      ),
      median_duration_ms: Some(
          2400000,
      ),
      avg_win_duration_ms: Some(
          4800000,
      ),
      avg_loss_duration_ms: Some(
          300000,
      ),
      avg_winner_mae_points: Some(
          2,
      ),
      winners_with_mae: 3,
      avg_loser_mfe_points: None,
      losers_with_mfe: 0,
      long: SideReport {
          trades: 2,
          net_points: -4,
          win_rate_pct: Some(
              50,
          ),
          profit_factor: Some(
              0.50,
          ),
          avg_win: Some(
              4,
          ),
          avg_loss: Some(
              8,
          ),
          expectancy_points: Some(
              -2,
          ),
      },
      short: SideReport {
          trades: 3,
          net_points: 15,
          win_rate_pct: Some(
              66.666666666666666666666666667,
          ),
          profit_factor: None,
          avg_win: Some(
              7.50,
          ),
          avg_loss: None,
          expectancy_points: Some(
              5,
          ),
      },
      by_exit_reason: [
          ReasonReport {
              reason: StopLoss,
              trades: 1,
              net_points: -8,
          },
          ReasonReport {
              reason: TakeProfit,
              trades: 2,
              net_points: 15,
          },
          ReasonReport {
              reason: Manual,
              trades: 2,
              net_points: 4,
          },
      ],
  }
"#;

/// The fixture the golden report is computed from: nine closed trades
/// over eleven days, chosen so that every branch of the arithmetic has
/// something to say. Both sides trade; there are wins, losses and a
/// scratch; the equity curve draws down and runs up again; two exit
/// reasons appear and three do not; and two rows carry no MAE/MFE, the
/// way a version-1 history file loads, so the disclosed denominators
/// are not simply the trade count.
///
/// It is fixed on purpose. The numbers below are the trader's own
/// results as this repository computes them, and a move that shifts one
/// of them is the code telling them a lie about their trading. Nothing
/// here may be edited to make a failing assertion pass: a change to this
/// fixture invalidates the golden, and a change to the golden has to be
/// argued for as a change in behaviour.
fn golden_history() -> LoadedHistory {
    let day = 86_400_000_i64;
    let t = |closed_ms: i64,
             side: Side,
             pnl: i64,
             held_ms: i64,
             reason: quantick_sim::ExitReason,
             excursions: Option<(i64, i64)>| ClosedTrade {
        side,
        quantity: Decimal::ONE,
        entry_price: Decimal::from(100),
        exit_price: Decimal::from(100 + pnl),
        opened_ms: closed_ms - held_ms,
        closed_ms,
        pnl_points: Decimal::from(pnl),
        exit_reason: reason,
        entry_agg_id: None,
        exit_agg_id: None,
        mae_points: excursions.map(|(mae, _)| Decimal::from(mae)),
        mfe_points: excursions.map(|(_, mfe)| Decimal::from(mfe)),
    };
    use quantick_sim::ExitReason::{Manual, StopLoss, TakeProfit};
    LoadedHistory {
        rows: vec![
            row(
                "AAAUSDT",
                Some(history::SessionSource::Live),
                t(
                    10 * day,
                    Side::Buy,
                    12,
                    3_600_000,
                    TakeProfit,
                    Some((4, 15)),
                ),
            ),
            row(
                "AAAUSDT",
                Some(history::SessionSource::Live),
                t(11 * day, Side::Buy, -5, 900_000, StopLoss, Some((7, 2))),
            ),
            row(
                "AAAUSDT",
                Some(history::SessionSource::Live),
                t(12 * day, Side::Sell, -3, 1_800_000, StopLoss, Some((6, 1))),
            ),
            row(
                "AAAUSDT",
                None,
                t(13 * day, Side::Sell, 0, 600_000, Manual, None),
            ),
            row(
                "BBBUSDT",
                Some(history::SessionSource::Live),
                t(
                    14 * day,
                    Side::Sell,
                    9,
                    7_200_000,
                    TakeProfit,
                    Some((2, 11)),
                ),
            ),
            row(
                "BBBUSDT",
                None,
                t(15 * day, Side::Buy, -8, 300_000, StopLoss, None),
            ),
            // A practice run. The default Source filter keeps it out
            // of the real track record, so the golden covers that
            // filter too rather than assuming it never fires.
            row(
                "BBBUSDT",
                Some(history::SessionSource::Replay),
                t(
                    16 * day,
                    Side::Buy,
                    40,
                    1_200_000,
                    TakeProfit,
                    Some((1, 44)),
                ),
            ),
            row(
                "BBBUSDT",
                Some(history::SessionSource::Live),
                t(19 * day, Side::Buy, 4, 2_400_000, Manual, Some((3, 6))),
            ),
            row(
                "BBBUSDT",
                Some(history::SessionSource::Live),
                t(20 * day, Side::Sell, 6, 4_800_000, TakeProfit, Some((1, 8))),
            ),
        ],
        files: 3,
        unreadable_files: 0,
        problem_rows: 0,
    }
}

/// The report and its equity walk as one block of text, every field
/// named. `{:#?}` over the report rather than a hand-written field list
/// on purpose: a hand-written list can silently omit a metric, and the
/// one metric nobody thought to write down is exactly the one a move
/// would break unnoticed. The equity walk is dumped beside it because
/// the curve and the trade list's running total read it, and it is
/// computed here rather than in `quantick-sim`.
fn dump_report(view: &ReportView) -> String {
    let mut out = String::new();
    out.push_str(&format!("rows: {}\n", view.rows.len()));
    out.push_str(&format!("hidden_outside: {}\n", view.hidden_outside));
    out.push_str(&format!("hidden_by_source: {}\n", view.hidden_by_source));
    out.push_str(&format!("anchor_ms: {:?}\n", view.anchor_ms));
    out.push_str(&format!("equity.points: {:?}\n", view.equity.points));
    out.push_str(&format!("equity.plot: {:?}\n", view.equity.plot));
    out.push_str(&format!(
        "equity.low: {:?}  equity.high: {:?}\n",
        view.equity.low, view.equity.high
    ));
    // Indented, and not merely for looks. `{:#?}` closes its braces in
    // column 0, and the size guard finds the end of a `#[cfg(test)]`
    // module by scanning for a line that is exactly `}` — so an
    // unindented dump inside the golden below walks that scan out of
    // the test module and scores three thousand lines of test code as
    // production. Two spaces keep the ratchet honest, and cost the
    // golden nothing: it is the same text, byte for byte.
    for line in format!("report: {:#?}", view.report).lines() {
        out.push_str("  ");
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// The numbers must not move.
///
/// A fixed journal in, the whole report out, asserted byte for byte —
/// across two cuts, so the filter arithmetic is under the golden too
/// and not only `PerformanceReport::from_trades`. `All` covers the
/// unfiltered aggregation; `Week` covers the anchor-relative cutoff,
/// which is the part that lives in this crate rather than in
/// `quantick-sim`.
///
/// This test is written before the report moves out of this file and
/// its expected text does not change when it does. That is the whole
/// point: an extraction that alters a rounding, a filter boundary or an
/// equity walk fails here rather than in front of the trader.
#[test]
fn the_report_numbers_are_fixed() {
    let utc = TzOffset::new(0);
    let mut paper = PaperTrading::new();
    paper.report_state_mut().report = Some(golden_history());

    paper.report_state_mut().report_period = ReportPeriod::All;
    paper.report_state_mut().ensure_report_view(utc);
    let all = dump_report(
        paper
            .report_state()
            .report_view
            .as_ref()
            .expect("the All cut"),
    );

    paper.report_state_mut().report_period = ReportPeriod::Week;
    paper.report_state_mut().ensure_report_view(utc);
    let week = dump_report(
        paper
            .report_state()
            .report_view
            .as_ref()
            .expect("the Week cut"),
    );

    assert_eq!(
        format!("== ALL ==\n{all}== WEEK ==\n{week}"),
        GOLDEN_REPORT,
        "the report's numbers moved"
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
