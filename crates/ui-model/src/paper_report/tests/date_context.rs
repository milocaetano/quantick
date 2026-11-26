use super::*;
use quantick_paper::report::scope::{ReportOrigin, prepare};
use quantick_paper::report::snapshot::{ReportChange, ReportEnv};
use std::path::Path;

fn write_journal(dir: &Path, symbol: &str, file: &str, closes: &[(i64, i64)]) {
    let folder = dir.join(symbol);
    std::fs::create_dir_all(&folder).unwrap();
    let mut csv = history::write_header(symbol, history::SessionSource::Live);
    for &(closed_ms, net) in closes {
        csv.push_str(&history::write_trade(&trade_at(closed_ms, net)));
    }
    std::fs::write(folder.join(file), csv).unwrap();
}

fn day_requests(day: CivilDate) -> [ReportRequest; 2] {
    [
        ReportRequest::Day {
            origin: ReportOrigin::Simulator,
            day,
        },
        ReportRequest::Session {
            session: SessionKey::parse("X/first.csv").unwrap(),
            day: Some(day),
        },
    ]
}

#[test]
fn receipt_view_and_replacement_day_share_journal_origin() {
    let scratch = crate::scratch::ScratchDir::new("report-date-origin");
    let dir = scratch.join("paper");
    let tz = TzOffset::new(-180);
    let day = CivilDate::from_ymd(2026, 11, 30);
    let next = day.offset_days(1);
    write_journal(&dir, "X", "first.csv", &[(day.start_ms(tz), 1)]);
    write_journal(&dir, "X", "second.csv", &[(next.start_ms(tz), 4)]);
    write_journal(
        &dir,
        "X",
        ".imported.csv",
        &[(day.start_ms(tz), 8), (next.start_ms(tz), 16)],
    );
    for request in day_requests(day) {
        let prepared = prepare(&dir, request, tz).unwrap();
        assert_eq!(prepared.history.rows.len(), 4);
        assert_eq!(
            (prepared.receipt.trades, prepared.receipt.net_points),
            (1, Decimal::ONE)
        );
        let mut state = State::default();
        Model::update(&mut state, Intent::Scoped(Box::new(prepared)));
        state.ensure_report_view(tz, PnlReader::plain());
        let opened = state.snapshot().unwrap();
        assert_eq!(
            (opened.rows.len(), opened.report.net_points),
            (1, Decimal::ONE)
        );
        assert_eq!(state.report_scope_label(), Some("Simulator · 2026-11-30"));
        assert_eq!(
            state.report_days().stat(next).unwrap().net,
            Decimal::from(4)
        );

        Model::update(&mut state, Intent::Days(DaySet::single(next)));
        state.ensure_report_view(tz, PnlReader::plain());
        let replaced = state.snapshot().unwrap();
        assert_eq!(
            (replaced.rows.len(), replaced.report.net_points),
            (1, Decimal::from(4))
        );
        assert_eq!(
            replaced.rows[0].session.as_ref().unwrap().as_str(),
            "X/second.csv"
        );
        assert_eq!(state.report_scope_label(), Some("Simulator"));
        Model::update(&mut state, Intent::Days(DaySet::default()));
        state.ensure_report_view(tz, PnlReader::plain());
        assert_eq!(
            state.snapshot().unwrap().report.net_points,
            Decimal::from(5)
        );
        assert_eq!(state.report_scope_label(), Some("Simulator"));
    }
}

/// A click after a ledger day report adds to that day and applies at once;
/// a Shift+click grows from the last clicked day, not from the ledger's.
#[test]
fn grid_clicks_after_a_ledger_day_apply_at_once_and_leave_its_context() {
    let scratch = crate::scratch::ScratchDir::new("report-date-pick");
    let dir = scratch.join("paper");
    let tz = TzOffset::new(-180);
    let day = CivilDate::from_ymd(2026, 11, 30);
    write_journal(&dir, "X", "first.csv", &[(day.start_ms(tz), 1)]);
    let first_pick = day.offset_days(2);
    let second_pick = day.offset_days(4);
    for request in day_requests(day) {
        let mut state = State::default();
        Model::update(
            &mut state,
            Intent::Scoped(Box::new(prepare(&dir, request, tz).unwrap())),
        );
        assert_eq!(state.picked_days(), &DaySet::single(day));
        assert!(state.report_scope().is_some());
        Model::update(&mut state, Intent::DatesOpen(true));
        assert_eq!(state.calendar().month, Some(day.month_start()));
        Model::update(
            &mut state,
            Intent::PickDay {
                day: first_pick,
                extend: false,
            },
        );
        let mut expected = DaySet::single(day);
        expected.toggle(first_pick);
        assert_eq!(state.picked_days(), &expected, "no Generate step");
        assert!(
            state.report_scope().is_none(),
            "the ledger's day is no longer the question"
        );
        Model::update(
            &mut state,
            Intent::PickDay {
                day: second_pick,
                extend: true,
            },
        );
        expected.add_span(first_pick, second_pick);
        assert_eq!(state.picked_days(), &expected);
        assert_eq!(state.picked_days().len(), 4);
        state.ensure_report_view(tz, PnlReader::plain());
        assert_eq!(state.report_view().as_ref().unwrap().days, expected);
        Model::update(&mut state, Intent::Close);
        assert!(
            !state.calendar().open,
            "closing the report closes its popover"
        );
    }
}

#[test]
fn date_only_reveal_preserves_all_symbols_after_closed_cache_eviction() {
    let scratch = crate::scratch::ScratchDir::new("report-date-symbols");
    let dir = scratch.join("paper");
    let tz = TzOffset::new(-180);
    let day = CivilDate::from_ymd(2026, 11, 30);
    for (symbol, net) in [("X", 2), ("Y", 5)] {
        write_journal(&dir, symbol, "saved.csv", &[(day.start_ms(tz), net)]);
    }
    let env = ReportEnv {
        symbol: "X",
        dir: &dir,
        session_journal_paths: &[],
        session_trades: &[],
        session_trade_at: &[],
        session_trade_sources: &[],
        open: None,
        pnl: PnlReader::plain(),
    };
    let mut adapter = crate::Adapter::<Model, (), Effects>::new((), std::convert::identity);
    adapter.with_updates(
        |_| {},
        |_, updates| {
            updates.with_local_effects(
                &mut |effects| io::local_effects(effects, &env),
                |session| {
                    let picked = DaySet::single(day);
                    let first =
                        io::report(session, Some(ReportChange::Days(picked.clone())), tz, &env);
                    assert_eq!(first.symbol.as_deref(), Some("X"));
                    assert_eq!(first.trade_count, 1);
                    session.update(Intent::Symbol(None));
                    session.update(Intent::ReloadReport);
                    let all = io::report(session, None, tz, &env);
                    assert_eq!((all.symbol, all.trade_count), (None, 2));
                    session.update(Intent::Close);
                    session.update(Intent::DirectoryChanged);
                    let evicted = io::report(session, None, tz, &env);
                    assert!(!evicted.loaded && !evicted.open);
                    assert_eq!(evicted.symbol, None);
                    let reopened =
                        io::report(session, Some(ReportChange::Days(picked.clone())), tz, &env);
                    assert!(reopened.open && reopened.loaded);
                    assert_eq!(reopened.days, picked);
                    assert_eq!(
                        (reopened.symbol, reopened.trade_count, reopened.net_points),
                        (None, 2, Decimal::from(7))
                    );
                },
            );
        },
    );
}

/// The trader's 2026-11-25 report: a Session report opened from the ledger,
/// then 2026-11-24 through today picked. The dates are the question now, so
/// every trade of that origin closed on those days is in, not only the
/// clicked session's, and the session title no longer claims the cut.
#[test]
fn picked_dates_widen_a_whole_session_report_to_its_origin() {
    let scratch = crate::scratch::ScratchDir::new("report-session-dates");
    let dir = scratch.join("paper");
    let tz = TzOffset::new(-180);
    let yesterday = CivilDate::from_ymd(2026, 11, 24);
    let today = yesterday.offset_days(1);
    write_journal(&dir, "X", "yesterday.csv", &[(yesterday.start_ms(tz), 2)]);
    // The last minute of today, local: inside the picked span.
    write_journal(&dir, "X", "today.csv", &[(today.end_ms(tz) - 60_000, 3)]);
    write_journal(&dir, "X", "older.csv", &[(yesterday.start_ms(tz) - 1, 100)]);
    let request = ReportRequest::Session {
        session: SessionKey::parse("X/today.csv").unwrap(),
        day: None,
    };
    let mut state = State::default();
    Model::update(
        &mut state,
        Intent::Scoped(Box::new(prepare(&dir, request, tz).unwrap())),
    );
    state.ensure_report_view(tz, PnlReader::plain());
    assert_eq!(state.snapshot().unwrap().rows.len(), 1, "the session alone");
    assert_eq!(
        state.report_days().stat(yesterday).map(|stat| stat.trades),
        Some(1),
        "before any pick, the grid shows the origin's days a pick would report"
    );

    Model::update(&mut state, Intent::Days(DaySet::span(yesterday, today)));
    state.ensure_report_view(tz, PnlReader::plain());
    let picked = state.snapshot().unwrap();
    assert_eq!(
        (picked.rows.len(), picked.report.net_points),
        (2, Decimal::from(5)),
        "both days' trades of the origin, nothing before the first day"
    );
    assert_eq!(state.report_scope_label(), Some("Simulator"));
    assert_eq!(
        state.report_days().stat(yesterday).map(|stat| stat.trades),
        Some(1),
        "the grid shows the day the cut now includes"
    );
}

/// A period pill asks about days, like picked dates: on a whole-session
/// report it widens to the session's origin instead of cutting the one file.
#[test]
fn a_period_pick_widens_a_whole_session_report_to_its_origin() {
    let scratch = crate::scratch::ScratchDir::new("report-session-period");
    let dir = scratch.join("paper");
    let tz = TzOffset::new(-180);
    let yesterday = CivilDate::from_ymd(2026, 11, 24);
    let today = yesterday.offset_days(1);
    write_journal(
        &dir,
        "X",
        "yesterday.csv",
        &[(yesterday.start_ms(tz) + 60_000, 2)],
    );
    write_journal(&dir, "X", "today.csv", &[(today.start_ms(tz) + 60_000, 3)]);
    let request = ReportRequest::Session {
        session: SessionKey::parse("X/today.csv").unwrap(),
        day: None,
    };
    let mut state = State::default();
    Model::update(
        &mut state,
        Intent::Scoped(Box::new(prepare(&dir, request, tz).unwrap())),
    );
    state.ensure_report_view(tz, PnlReader::plain());
    assert_eq!(state.snapshot().unwrap().rows.len(), 1, "the session alone");
    let origin = state.report_source();

    Model::update(&mut state, Intent::Period(ReportPeriod::Week));
    state.ensure_report_view(tz, PnlReader::plain());
    let week = state.snapshot().unwrap();
    assert_eq!(
        (week.rows.len(), week.report.net_points),
        (2, Decimal::from(5)),
        "the week's trades of the origin, not the one session"
    );
    assert_eq!(state.report_source(), origin, "the origin's source stays picked");

    Model::update(&mut state, Intent::Period(ReportPeriod::Today));
    state.ensure_report_view(tz, PnlReader::plain());
    let day = state.snapshot().unwrap();
    assert_eq!(
        (day.rows.len(), day.report.net_points),
        (1, Decimal::from(3)),
        "today narrows within the origin"
    );
}
