//! Eight real-data tests transferred from app/src/paper_report/tests/mod.rs at
//! 1d9fa33fc4db5fead8499593e80b8ca7255f8dda. Fixtures and assertions are retained;
//! only the account fixture is replaced by the headless report owner. Native,
//! filesystem, focus, scroll and host tests remain in app. Support is counted here.
use super::*;
use crate::Model as _;
use quantick_civil::DateRange;
use quantick_engine::Side;
use quantick_paper::report::HistoryRow;
use quantick_paper::report::snapshot::ReportWindow;
use quantick_sim::history;
use quantick_trading::{ClosedTrade, ExitReason};
use rust_decimal::Decimal;

// Fixture-compatible access keeps every transferred assertion and actual domain
// datum intact. Only projection installation and actions call Model::update.
struct ReportFixture(State);
impl ReportFixture {
    fn new() -> Self {
        Self(State::default())
    }
    fn report_state(&self) -> &State {
        &self.0
    }
    fn report_state_mut(&mut self) -> &mut State {
        &mut self.0
    }
    fn report_parts(&mut self) -> (&mut State, LiveCloses<'static>) {
        (&mut self.0, LiveCloses::default())
    }
}
impl State {
    fn ensure_report_view(&mut self, tz: TzOffset, pnl: PnlReader<'_>) {
        if let Some(receipt) = self.project_report(tz, pnl) {
            Model::update(self, Intent::InstallReport(receipt));
        }
    }
    fn pick_report_dates(&mut self, dates: DaySelection) {
        Model::update(self, Intent::Dates(dates));
    }
    fn set_day_collapsed(&mut self, day: CivilDate, folded: bool) {
        Model::update(self, Intent::Ledger(LedgerIntent::FoldDay(day, folded)));
    }
    fn toggle_all_days(&mut self, expand: bool, live: &LiveCloses<'_>) {
        let receipt = self.project_day_folds(expand, *live);
        Model::update(self, Intent::Ledger(LedgerIntent::FoldDays(receipt)));
    }
}

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
fn the_report_opens_on_real_and_keeps_replay_out_until_asked() {
    let utc = TzOffset::new(0);
    let day = 86_400_000_i64;
    let mut paper = ReportFixture::new();
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
        sessions: Default::default(),
    });
    paper
        .report_state_mut()
        .ensure_report_view(utc, PnlReader::plain());
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
    paper
        .report_state_mut()
        .ensure_report_view(utc, PnlReader::plain());
    let view = paper.report_state().report_view.as_ref().expect("rebuilt");
    assert_eq!(view.rows.len(), 1, "the practice run, alone");
    assert_eq!(view.report.net_points, Decimal::from(100));
    assert_eq!(view.hidden_by_source, 2);

    paper.report_state_mut().report_source = SourceFilter::All;
    paper
        .report_state_mut()
        .ensure_report_view(utc, PnlReader::plain());
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
    let mut paper = ReportFixture::new();
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
        sessions: Default::default(),
    });
    paper.report_state_mut().report_symbol = Some("WINV26".to_owned());

    // The pills in force, no dates picked.
    paper.report_state_mut().report_period = ReportPeriod::All;
    paper
        .report_state_mut()
        .ensure_report_view(tz, PnlReader::plain());
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
    paper
        .report_state_mut()
        .ensure_report_view(tz, PnlReader::plain());
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

/// Folding is per civil day and reversible, and the "fold everything"
/// control reports honestly whether it has anything left to fold.
#[test]
fn days_fold_shut_one_at_a_time_or_all_at_once() {
    let tz = TzOffset::new(-180);
    let day = CivilDate::from_ymd(2026, 8, 17);
    let mut paper = ReportFixture::new();
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
        sessions: Default::default(),
    });
    assert!(
        !{
            let (state, env) = paper.report_parts();
            state.all_days_collapsed(env)
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
            state.all_days_collapsed(env)
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
            state.all_days_collapsed(env)
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
        state.all_days_collapsed(env)
    });

    // An empty ledger has nothing folded *and* nothing to fold: the
    // control must not claim everything is already shut.
    let mut empty = ReportFixture::new();
    assert!(!{
        let (state, env) = empty.report_parts();
        state.all_days_collapsed(env)
    });
}

/// A picked day cuts the report to that civil day, and a picked span
/// to both its ends inclusive. The pills stand down while it holds, so
/// a chosen date can never come back empty because a forgotten pill
/// was cutting too.
#[test]
fn a_picked_calendar_range_cuts_the_report_and_the_pills_stand_down() {
    let tz = TzOffset::new(-180);
    let first = CivilDate::from_ymd(2026, 8, 12);
    let mut paper = ReportFixture::new();
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
        sessions: Default::default(),
    });
    // A pill that would hide the older trades is deliberately left on:
    // the range must win outright, not intersect.
    paper.report_state_mut().report_period = ReportPeriod::Today;

    let picked = paper.report_state().calendar.selection.click(first);
    paper.report_state_mut().pick_report_dates(picked);
    paper
        .report_state_mut()
        .ensure_report_view(tz, PnlReader::plain());
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
    paper
        .report_state_mut()
        .ensure_report_view(tz, PnlReader::plain());
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
    paper
        .report_state_mut()
        .ensure_report_view(tz, PnlReader::plain());
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
    let mut paper = ReportFixture::new();
    paper.report_state_mut().report = Some(LoadedHistory {
        rows: vec![row("X", None, trade_at(day.offset_days(2).start_ms(tz), 5))],
        files: 1,
        unreadable_files: 0,
        problem_rows: 0,
        sessions: Default::default(),
    });
    let picked = paper.report_state().calendar.selection.click(day);
    paper.report_state_mut().pick_report_dates(picked);
    paper
        .report_state_mut()
        .ensure_report_view(tz, PnlReader::plain());
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
    let mut paper = ReportFixture::new();
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
        sessions: Default::default(),
    });
    paper
        .report_state_mut()
        .ensure_report_view(tz, PnlReader::plain());
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
    paper
        .report_state_mut()
        .ensure_report_view(tz, PnlReader::plain());
    assert_eq!(
        paper.report_state().report_days.len(),
        2,
        "Both lights up both days"
    );

    // The same trades, read on another clock, land on other days.
    paper
        .report_state_mut()
        .ensure_report_view(TzOffset::new(0), PnlReader::plain());
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
    let mut paper = ReportFixture::new();
    paper.report_state_mut().report = Some(LoadedHistory {
        rows: vec![
            row("X", None, trade_at(10 * day, 5)),
            row("X", None, trade_at(18 * day, -2)),
            row("X", None, trade_at(20 * day + 3_600_000, 7)),
        ],
        files: 1,
        unreadable_files: 0,
        problem_rows: 0,
        sessions: Default::default(),
    });
    paper.report_state_mut().report_period = ReportPeriod::Week;
    paper
        .report_state_mut()
        .ensure_report_view(utc, PnlReader::plain());
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
    paper
        .report_state_mut()
        .ensure_report_view(utc, PnlReader::plain());
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
    let mut paper = ReportFixture::new();
    paper.report_state_mut().report = Some(LoadedHistory {
        rows: vec![
            row("OLDSYM", None, trade_at(day, 5)),
            row("NEWSYM", None, trade_at(60 * day, 7)),
        ],
        files: 2,
        unreadable_files: 0,
        problem_rows: 0,
        sessions: Default::default(),
    });
    paper.report_state_mut().report_period = ReportPeriod::Month;
    paper
        .report_state_mut()
        .ensure_report_view(utc, PnlReader::plain());
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
        sessions: Default::default(),
    });
    paper.report_state_mut().report_view = None;
    paper
        .report_state_mut()
        .ensure_report_view(utc, PnlReader::plain());
    let view = paper.report_state().report_view.as_ref().expect("rebuilt");
    assert_eq!(view.rows.len(), 1, "its own scope shows the old market");
    assert_eq!(view.hidden_outside, 0);
}

// New composition support: build the same real rows and move IO receipts once.
fn history_rows(rows: Vec<HistoryRow>) -> LoadedHistory {
    LoadedHistory {
        rows,
        files: 1,
        unreadable_files: 0,
        problem_rows: 0,
        sessions: Default::default(),
    }
}
fn loaded_report(rows: Vec<HistoryRow>) -> Intent {
    Intent::LoadedReport(LoadedReport {
        history: history_rows(rows),
        symbols: vec!["X".to_owned()],
    })
}
fn loaded_ledger(rows: Vec<HistoryRow>) -> Intent {
    let history = history_rows(rows);
    Intent::LoadedLedger(LoadedLedger::new(
        history,
        vec!["X".to_owned()],
        PnlReader::plain(),
    ))
}

#[test]
fn paper_report_filters_calendar_and_snapshot_keep_one_owner() {
    let tz = TzOffset::new(0);
    let day = CivilDate::from_ymd(2026, 8, 12);
    let mut adapter = crate::Adapter::<Model, (), Effects>::new((), std::convert::identity);
    let mut loaded = Some(loaded_report(vec![
        row("X", None, trade_at(day.start_ms(tz), 5)),
        row(
            "X",
            Some(history::SessionSource::Replay),
            trade_at(day.offset_days(1).start_ms(tz), 9),
        ),
    ]));
    let mut requests = 0;
    let mut emitted = Vec::new();
    adapter.with_updates(
        |effects| emitted.push(effects),
        |_, session| {
            session.with_local_effects(
                &mut |mut effects| {
                    let receipt = effects.load.take().map(|request| {
                        assert_eq!(request, Load::Report(Some("X".to_owned())));
                        requests += 1;
                        loaded.take().expect("one moved load")
                    });
                    (effects, receipt)
                },
                |session| {
                    session.update(Intent::Open("X".to_owned()));
                    assert_eq!(
                        session.state().report.as_ref().unwrap().rows.len(),
                        2,
                        "terminal receipt is synchronous"
                    );
                    let cut = session
                        .state()
                        .project_report(tz, PnlReader::plain())
                        .unwrap();
                    session.update(Intent::InstallReport(cut));
                    assert_eq!(session.state().snapshot().unwrap().rows.len(), 1);
                    session.update(Intent::Source(SourceFilter::All));
                    session.update(Intent::Dates(DaySelection::Anchor(day.offset_days(1))));
                    let cut = session
                        .state()
                        .project_report(tz, PnlReader::plain())
                        .unwrap();
                    session.update(Intent::InstallReport(cut));
                    let snapshot = session.state().snapshot().unwrap();
                    assert_eq!(snapshot.source, SourceFilter::All);
                    assert_eq!(
                        snapshot.window,
                        ReportWindow::Dates(DateRange::new(day.offset_days(1), day.offset_days(1)))
                    );
                    assert_eq!(snapshot.report.net_points, Decimal::from(9));
                },
            );
        },
    );
    assert_eq!(requests, 1);
    assert!(loaded.is_none());
    assert!(
        emitted.iter().all(|effects| effects == &Effects::default()),
        "terminal receipt asks for no second IO"
    );
    assert!(adapter.model_state.report_open);
}

#[test]
fn paper_report_projection_receipts_preserve_freshness_and_month() {
    use quantick_paper::pnl::{PnlPrimary, PnlSettings};
    let tz = TzOffset::new(0);
    let january = CivilDate::from_ymd(2026, 1, 2);
    let august = CivilDate::from_ymd(2026, 8, 12);
    let mut state = State::default();
    Model::update(
        &mut state,
        loaded_report(vec![
            row("X", None, trade_at(january.start_ms(tz), 2)),
            row(
                "X",
                Some(history::SessionSource::Replay),
                trade_at(august.start_ms(tz), 9),
            ),
        ]),
    );
    let stale = state.project_report(tz, PnlReader::plain()).unwrap();
    Model::update(&mut state, Intent::Source(SourceFilter::Replay));
    Model::update(&mut state, Intent::InstallReport(stale));
    assert!(
        state.report_view.is_none(),
        "an obsolete projection cannot restore a retired filter"
    );
    state.ensure_report_view(tz, PnlReader::plain());
    assert_eq!(state.calendar.month, Some(august.month_start()));
    assert_eq!(state.report_days.len(), 1);
    assert!(
        state.project_report(tz, PnlReader::plain()).is_none(),
        "warm frames borrow the cache"
    );
    let shown = CalendarState {
        month: Some(january.month_start()),
        ..state.calendar
    };
    Model::update(&mut state, Intent::Calendar(shown));
    Model::update(&mut state, Intent::Dates(DaySelection::Anchor(august)));
    state.ensure_report_view(tz, PnlReader::plain());
    assert_eq!(
        state.calendar.month,
        Some(january.month_start()),
        "range changes keep the day-index key and month"
    );
    let settings = PnlSettings {
        chosen: Some(PnlPrimary::PointsTimesQty),
        ..Default::default()
    };
    let pnl = PnlReader {
        book: PnlReader::plain().book,
        settings: &settings,
    };
    assert!(
        state.project_report(tz, pnl).is_some(),
        "actual borrowed money policy invalidates wording"
    );
    state.ensure_report_view(tz, pnl);
    assert!(state.report_view.as_ref().unwrap().worded_by(pnl));
    assert_eq!(
        state.calendar.month,
        Some(january.month_start()),
        "money alone does not reset the month"
    );
    state.ensure_report_view(TzOffset::new(-180), pnl);
    assert_eq!(
        state.calendar.month,
        Some(august.month_start()),
        "timezone changes rebuild the day index"
    );
    Model::update(
        &mut state,
        loaded_report(vec![row("X", None, trade_at(january.start_ms(tz), 3))]),
    );
    Model::update(&mut state, Intent::Source(SourceFilter::Real));
    state.ensure_report_view(tz, pnl);
    assert_eq!(
        state.calendar.month,
        Some(january.month_start()),
        "new loaded data resets to its own newest month"
    );
}

#[test]
fn paper_report_lifecycle_requests_reload_only_at_existing_boundaries() {
    let mut state = State::default();
    Model::update(&mut state, Intent::Ledger(LedgerIntent::Pages(4)));
    for intent in [Intent::HistoryImported, Intent::DirectoryChanged] {
        assert_eq!(
            Model::update(&mut state, intent),
            Effects::default(),
            "closed reports do not read history"
        );
        assert_eq!(state.ledger_pages, 1);
    }
    assert_eq!(
        Model::update(&mut state, Intent::Open("X".to_owned())).load,
        Some(Load::Report(Some("X".to_owned())))
    );
    assert_eq!(
        Model::update(&mut state, loaded_report(vec![])),
        Effects::default()
    );
    Model::update(&mut state, Intent::Symbol(None));
    assert_eq!(
        Model::update(&mut state, Intent::Open("Y".to_owned())).load,
        Some(Load::Report(None)),
        "later opens retain scope"
    );
    for intent in [
        Intent::JournalChanged,
        Intent::HistoryImported,
        Intent::JournalEdited,
        Intent::DirectoryChanged,
    ] {
        assert_eq!(
            Model::update(&mut state, intent).load,
            Some(Load::Report(None))
        );
    }
    Model::update(&mut state, Intent::Ledger(LedgerIntent::Pages(4)));
    Model::update(&mut state, Intent::SymbolChanged);
    assert_eq!(
        state.ledger_pages, 4,
        "per-frame symbol sync preserves pages"
    );
    assert_eq!(
        Model::update(&mut state, Intent::Ledger(LedgerIntent::EnsureLoaded)).load,
        Some(Load::Ledger(LedgerScope::Chart))
    );
    Model::update(&mut state, loaded_ledger(vec![]));
    assert_eq!(state.ledger_pages, 4, "lazy loading preserves pages");
    assert_eq!(
        Model::update(&mut state, Intent::Ledger(LedgerIntent::EnsureLoaded)),
        Effects::default()
    );
    Model::update(&mut state, Intent::JournalEdited);
    assert!(state.history_cache.is_none());
    assert_eq!(state.ledger_pages, 4, "journal edits preserve pages");
    Model::update(&mut state, Intent::Ledger(LedgerIntent::Rescope));
    assert_eq!(state.ledger_pages, 1);
    Model::update(&mut state, Intent::CustomText("2x".to_owned()));
    Model::update(&mut state, Intent::SubmitCustom { enter: false });
    assert!(state.toast.is_none());
    Model::update(&mut state, Intent::SubmitCustom { enter: true });
    Model::update(&mut state, Intent::Close);
    assert_eq!(
        Model::update(&mut state, Intent::DrainToast)
            .toast
            .as_deref(),
        Some("SIM: could not read `2x` as a period - use 45m, 12h, 2d or 1w")
    );
    assert_eq!(
        Model::update(&mut state, Intent::DrainToast),
        Effects::default()
    );
}

#[test]
fn paper_ledger_folds_selection_and_session_edits_delegate_to_domain() {
    let mut state = State::default();
    let session = SessionKey::parse("X/record.csv").unwrap();
    let trade = trade_at(1000, 5);
    let reference = history::TradeRefs::default().next(&trade);
    Model::update(&mut state, loaded_ledger(vec![row("X", None, trade)]));
    let stale = state
        .project_ledger(LiveCloses::default(), TzOffset::new(0), PnlReader::plain())
        .unwrap();
    Model::update(
        &mut state,
        Intent::Ledger(LedgerIntent::View(LedgerViewChange {
            source: Some(SourceFilter::Replay),
            ..Default::default()
        })),
    );
    Model::update(&mut state, Intent::InstallLedger(stale));
    assert!(state.sessions.cut.is_none());
    let cut = state
        .project_ledger(LiveCloses::default(), TzOffset::new(0), PnlReader::plain())
        .unwrap();
    Model::update(&mut state, Intent::InstallLedger(cut));
    assert!(state.sessions.cut.as_ref().unwrap().kept.is_empty());
    assert!(
        state
            .project_ledger(LiveCloses::default(), TzOffset::new(0), PnlReader::plain())
            .is_none()
    );
    Model::update(
        &mut state,
        Intent::Ledger(LedgerIntent::Session(SessionPress::Fold(
            session.clone(),
            true,
        ))),
    );
    assert!(state.sessions.view.folded.contains(&session));
    assert_eq!(state.sessions.focused.as_ref(), Some(&session));
    Model::update(
        &mut state,
        Intent::Ledger(LedgerIntent::Rows(Box::new(RowInput {
            select_saved: Some(Some((session.clone(), reference))),
            ..Default::default()
        }))),
    );
    assert!(state.sessions.focused.is_none());
    Model::update(
        &mut state,
        Intent::Ledger(LedgerIntent::Rows(Box::new(RowInput {
            select: Some(Some(2)),
            ..Default::default()
        }))),
    );
    assert_eq!(state.selected_trade, Some(2));
    assert!(state.sessions.selected_saved.is_none());
    let rename = JournalEdit::Rename {
        session: session.clone(),
        name: "Opening".to_owned(),
    };
    let result = Model::update(
        &mut state,
        Intent::Ledger(LedgerIntent::Rows(Box::new(RowInput {
            session: Some(SessionPress::Apply(rename.clone())),
            keyed: Some(SessionPress::StartRename(session.clone())),
            reveal_more: true,
            navigate: Some(LedgerAction::Navigate(1, 2)),
            ..Default::default()
        }))),
    );
    assert_eq!(
        result.ledger,
        Some(LedgerAction::Edit(rename)),
        "row edit wins before paging/navigation/key output"
    );
    assert_eq!(
        state.ledger_pages, 1,
        "the row's early return preserves paging"
    );
    assert!(
        state.sessions.edit.is_some(),
        "the key transition still executes"
    );
    let edit = state.sessions.edit.clone();
    Model::update(
        &mut state,
        Intent::Ledger(LedgerIntent::Rows(Box::new(RowInput {
            edit,
            ..Default::default()
        }))),
    );
    assert!(
        state.sessions.edit.is_none(),
        "an unreachable inline row cancels through domain policy"
    );
    Model::update(
        &mut state,
        Intent::Ledger(LedgerIntent::TagTarget(Some((
            session.clone(),
            reference,
            vec!["old".to_owned()],
        )))),
    );
    Model::update(
        &mut state,
        Intent::Ledger(LedgerIntent::TagText("#new".to_owned())),
    );
    assert_eq!(
        Model::update(&mut state, Intent::Ledger(LedgerIntent::SubmitTags)).ledger,
        Some(LedgerAction::Edit(JournalEdit::Tag {
            session,
            trade: reference,
            tags: vec!["#new".to_owned()]
        }))
    );
}
