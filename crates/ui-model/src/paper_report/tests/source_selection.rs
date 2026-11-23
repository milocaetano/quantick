use super::*;

#[test]
fn sources_api_helper_loads_real_journals_retries_and_inspects_without_writes() {
    let scratch = crate::scratch::ScratchDir::new("report-source-io");
    let dir = scratch.path().join("paper");
    let tz = TzOffset::new(-180);
    let day = quantick_civil::parse_iso_date("2026-11-19").unwrap();
    let files: Vec<_> = mixed_rows(day, tz)
        .into_iter()
        .map(|row| {
            let path = row.session.unwrap().path(&dir);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            let source = match row.source {
                Some(history::SessionSource::Replay) => "# source=replay\n",
                None => "",
                _ => "# source=live\n",
            };
            let text = format!(
                "# quantick-trades 2\n# symbol=X\n{source}{}\n{},{},long,1,100,{},{},manual,,,,\n",
                history::HEADER,
                row.trade.opened_ms,
                row.trade.closed_ms,
                row.trade.exit_price,
                row.trade.pnl_points
            );
            let text = if row.source.is_none() {
                text + "incomplete row\n"
            } else {
                text
            };
            std::fs::write(&path, &text).unwrap();
            (path, text)
        })
        .collect();
    let env = quantick_paper::report::snapshot::ReportEnv {
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
    let selection = SourceSelection::from(SourceFilter::Broker).union(SourceFilter::Real);
    adapter.with_updates(
        |_| {},
        |_, session| {
            session.with_local_effects(
                &mut |effects| io::local_effects(effects, &env),
                |session| {
                    let before = io::sources(session, None, tz, &env);
                    assert!(!before.open && !before.loaded);
                    for sources in [Some(selection), Some(selection), None] {
                        let view = io::sources(session, sources, tz, &env);
                        assert!(view.open && view.loaded);
                        assert_eq!(view.source, selection);
                        assert_eq!(view.trade_count, 3);
                        assert_eq!(view.skipped_rows, 1);
                        assert_eq!(view.unreadable_files, 0);
                        assert_eq!(view.net_points, Decimal::from(10));
                        assert_eq!(view.symbol.as_deref(), Some("X"));
                        assert!(view.scope.is_none());
                    }
                },
            );
        },
    );
    for (path, text) in files {
        assert_eq!(std::fs::read_to_string(path).unwrap(), text);
    }
}

fn mixed_rows(day: CivilDate, tz: TzOffset) -> Vec<HistoryRow> {
    [
        ("X/live.csv", Some(history::SessionSource::Live), 2),
        (
            "broker/X/demo.csv",
            Some(history::SessionSource::Broker(
                quantick_trading::TradeMode::Demo,
            )),
            3,
        ),
        ("X/replay.csv", Some(history::SessionSource::Replay), 100),
        ("X/legacy.csv", None, 5),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, (key, source, pnl))| {
        let mut row = row(
            "X",
            source,
            trade_at(day.start_ms(tz) + (i as i64 + 1) * 1000, pnl),
        );
        row.session = Some(SessionKey::parse(key).unwrap());
        row
    })
    .collect()
}

#[test]
fn source_edits_leave_context_and_all_readouts_follow_the_union_across_reloads() {
    let tz = TzOffset::new(-180);
    let day = quantick_civil::parse_iso_date("2026-11-19").unwrap();
    let rows = mixed_rows(day, tz);
    let mut state = State::default();
    for context_day in [None, Some(day)] {
        Model::update(
            &mut state,
            Intent::Scoped(Box::new(PreparedReport {
                receipt: ReportReceipt {
                    request: ReportRequest {
                        session: SessionKey::parse("X/live.csv").unwrap(),
                        day: context_day,
                    },
                    title: "Simulator session".to_owned(),
                    trades: 1,
                    net_points: Decimal::from(2),
                },
                history: history_rows(rows.clone()),
                symbols: vec!["X".to_owned()],
            })),
        );
        state.ensure_report_view(tz, PnlReader::plain());
        Model::update(&mut state, Intent::Symbol(Some("X".to_owned())));
        Model::update(&mut state, Intent::Dates(DaySelection::Anchor(day)));
        for (selection, count, net) in [
            (SourceFilter::Broker.into(), 1, 3),
            (SourceFilter::Real.into(), 2, 7),
            (SourceFilter::Replay.into(), 1, 100),
            (
                SourceSelection::from(SourceFilter::Broker).union(SourceFilter::Real),
                3,
                10,
            ),
            (SourceFilter::Simulator.into(), 3, 107),
            (SourceFilter::All.into(), 4, 110),
            (SourceSelection::default(), 0, 0),
        ] {
            Model::update(&mut state, Intent::Source(selection));
            assert!(state.report_scope().is_none());
            assert_eq!(state.report_symbol().as_deref(), Some("X"));
            assert_eq!(state.calendar().selection, DaySelection::Anchor(day));
            for reload in [
                Intent::ReloadReport,
                Intent::JournalChanged,
                Intent::HistoryImported,
                Intent::Open("X".to_owned()),
            ] {
                assert_eq!(
                    Model::update(&mut state, reload).load,
                    Some(Load::Report(Some("X".to_owned())))
                );
                Model::update(&mut state, loaded_report(rows.clone()));
                state.ensure_report_view(tz, PnlReader::plain());
                let view = state.report_view().as_ref().unwrap();
                assert_eq!(view.rows.len(), count);
                assert_eq!(view.report.net_points, Decimal::from(net));
                assert_eq!(view.equity.points.last(), Some(&Decimal::from(net)));
                assert_eq!(
                    state.report_days().stat(day).map_or(0, |stat| stat.trades),
                    count
                );
                assert!(
                    state.project_report(tz, PnlReader::plain()).is_none(),
                    "warm frames reuse the cut"
                );
                if count == 0 {
                    assert_eq!(view.scope_line(), "No sources selected");
                    assert!(state.filters_narrowed("X"));
                    assert_eq!(
                        state.empty_hint(),
                        "Select one or more sources to see their saved trades."
                    );
                }
            }
        }
    }
    Model::update(&mut state, Intent::Source(SourceFilter::All.into()));
    Model::update(&mut state, loaded_report(rows.clone()));
    state.ensure_report_view(tz, PnlReader::plain());
    let stale = state
        .project_report(TzOffset::new(0), PnlReader::plain())
        .unwrap();
    Model::update(&mut state, Intent::Source(SourceFilter::Broker.into()));
    Model::update(&mut state, Intent::InstallReport(stale));
    assert!(
        state.snapshot().is_none(),
        "late cuts cannot overwrite a new selection"
    );
    let mut updated = rows;
    updated[1].trade.pnl_points = Decimal::from(7);
    Model::update(&mut state, Intent::JournalChanged);
    Model::update(&mut state, loaded_report(updated));
    state.ensure_report_view(tz, PnlReader::plain());
    assert_eq!(
        state.snapshot().unwrap().report.net_points,
        Decimal::from(7)
    );
    assert_eq!(state.report_days().stat(day).unwrap().net, Decimal::from(7));
}

#[test]
fn leaving_scope_with_the_same_sources_invalidates_the_day_index() {
    let tz = TzOffset::new(0);
    let day = quantick_civil::parse_iso_date("2026-11-19").unwrap();
    let mut rows = mixed_rows(day, tz);
    rows.extend(mixed_rows(day.offset_days(1), tz));
    let mut state = State::default();
    Model::update(
        &mut state,
        Intent::Scoped(Box::new(PreparedReport {
            receipt: ReportReceipt {
                request: ReportRequest {
                    session: SessionKey::parse("X/live.csv").unwrap(),
                    day: Some(day),
                },
                title: "Simulator day".to_owned(),
                trades: 3,
                net_points: Decimal::from(107),
            },
            history: history_rows(rows),
            symbols: vec!["X".to_owned()],
        })),
    );
    state.ensure_report_view(tz, PnlReader::plain());
    assert_eq!(state.report_days().len(), 1);
    Model::update(&mut state, Intent::Source(SourceFilter::Simulator.into()));
    state.ensure_report_view(tz, PnlReader::plain());
    assert_eq!(state.report_days().len(), 2);
    assert_eq!(
        state.snapshot().unwrap().rows.len(),
        3,
        "visible dates still constrain the rows"
    );
    Model::update(&mut state, Intent::Period(ReportPeriod::All));
    state.ensure_report_view(tz, PnlReader::plain());
    assert_eq!(state.snapshot().unwrap().rows.len(), 6);
}
