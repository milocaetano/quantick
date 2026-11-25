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

        Model::update(
            &mut state,
            Intent::Draft(DateDraft {
                start: next.iso(),
                end: next.iso(),
            }),
        );
        Model::update(&mut state, Intent::GenerateDates);
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
        Model::update(&mut state, Intent::Dates(DaySelection::None));
        state.ensure_report_view(tz, PnlReader::plain());
        assert_eq!(
            state.snapshot().unwrap().report.net_points,
            Decimal::from(5)
        );
        assert_eq!(state.report_scope_label(), Some("Simulator"));
    }
}

#[test]
fn a_new_calendar_gesture_starts_after_an_applied_ledger_day() {
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
        let calendar = CalendarState {
            open: true,
            ..state.calendar()
        };
        Model::update(&mut state, Intent::Calendar(calendar));
        Model::update(&mut state, Intent::PickDate(first_pick));
        assert_eq!(
            state.date_draft().range().unwrap(),
            DateRange::new(first_pick, first_pick)
        );
        assert_eq!(state.applied_dates(), Some(DateRange::new(day, day)));
        Model::update(&mut state, Intent::PickDate(second_pick));
        let range = DateRange::new(first_pick, second_pick);
        assert_eq!(state.date_draft().range().unwrap(), range);
        assert_eq!(state.applied_dates(), Some(DateRange::new(day, day)));
        Model::update(&mut state, Intent::GenerateDates);
        assert_eq!(state.applied_dates(), Some(range));
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
                    let draft = DateDraft {
                        start: day.iso(),
                        end: day.iso(),
                    };
                    let first =
                        io::report(session, Some(ReportChange::Draft(draft.clone())), tz, &env)
                            .unwrap();
                    assert_eq!(first.symbol.as_deref(), Some("X"));
                    assert_eq!(first.trade_count, 1);
                    session.update(Intent::Symbol(None));
                    session.update(Intent::ReloadReport);
                    let all = io::report(session, None, tz, &env).unwrap();
                    assert_eq!((all.symbol, all.trade_count), (None, 2));
                    session.update(Intent::Close);
                    session.update(Intent::DirectoryChanged);
                    let evicted = io::report(session, None, tz, &env).unwrap();
                    assert!(!evicted.loaded && !evicted.open);
                    assert_eq!(evicted.symbol, None);
                    let staged =
                        io::report(session, Some(ReportChange::Draft(draft)), tz, &env).unwrap();
                    assert!(staged.open && staged.loaded && staged.applied.is_none());
                    assert_eq!(
                        (staged.symbol, staged.trade_count, staged.net_points),
                        (None, 2, Decimal::from(7))
                    );
                    let generated =
                        io::report(session, Some(ReportChange::Generate), tz, &env).unwrap();
                    assert_eq!(
                        (
                            generated.symbol,
                            generated.trade_count,
                            generated.net_points
                        ),
                        (None, 2, Decimal::from(7))
                    );
                },
            );
        },
    );
}
