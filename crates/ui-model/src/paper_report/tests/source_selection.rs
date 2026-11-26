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
                "# quantick-trades 3\n# symbol=X\n{source}{}\n{},{},long,1,100,{},{},manual,,,,,0\n",
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
                    use quantick_paper::report::snapshot::ReportChange;
                    let revision = session.state().revision;
                    let before = io::report(session, None, tz, &env);
                    assert!(!before.open && !before.loaded);
                    assert_eq!(session.state().revision, revision);
                    let picked = DaySet::single(day);
                    for _ in 0..2 {
                        let set =
                            io::report(session, Some(ReportChange::Days(picked.clone())), tz, &env);
                        assert_eq!(set.days, picked, "a retry sets, never toggles");
                        assert_eq!(set.trade_count, 2);
                        assert_eq!(set.net_points, Decimal::from(7));
                    }
                    io::report(
                        session,
                        Some(ReportChange::Days(DaySet::default())),
                        tz,
                        &env,
                    );
                    let inspected = io::report(session, None, tz, &env);
                    assert!(inspected.days.is_empty());
                    for sources in [Some(selection), Some(selection), None] {
                        let view = io::report(
                            session,
                            sources.map(quantick_paper::report::snapshot::ReportChange::Sources),
                            tz,
                            &env,
                        );
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
                    request: ReportRequest::Session {
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
        Model::update(&mut state, Intent::Days(DaySet::single(day)));
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
            assert_eq!(state.picked_days(), &DaySet::single(day));
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
                    assert!(view.scope_line().contains("No sources selected"));
                    assert!(view.scope_line().contains("Nov 19 · 1 day"));
                    assert!(view.scope_line().contains("UTC-03:00"));
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
fn context_dates_cut_the_view_but_leave_other_days_available_for_selection() {
    let tz = TzOffset::new(0);
    let day = quantick_civil::parse_iso_date("2026-11-19").unwrap();
    let mut rows = mixed_rows(day, tz);
    rows.extend(mixed_rows(day.offset_days(1), tz));
    let mut state = State::default();
    Model::update(
        &mut state,
        Intent::Scoped(Box::new(PreparedReport {
            receipt: ReportReceipt {
                request: ReportRequest::Session {
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
    assert_eq!(state.report_days().len(), 2);
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

#[test]
fn picked_days_replace_session_context_and_keep_tags_through_period_changes() {
    let tz = TzOffset::new(-180);
    let first = CivilDate::from_ymd(2026, 11, 30);
    let last = first.offset_days(2);
    let key = SessionKey::parse("X/live.csv").unwrap();
    let other = SessionKey::parse("X/other.csv").unwrap();
    for (request, count) in [
        (
            // Picked dates widen a whole-session report to its origin.
            ReportRequest::Session {
                session: key.clone(),
                day: None,
            },
            3,
        ),
        (
            ReportRequest::Tag {
                origin: quantick_paper::report::scope::ReportOrigin::Simulator,
                tag: "setup".to_owned(),
            },
            2,
        ),
        (
            ReportRequest::Session {
                session: key.clone(),
                day: Some(first.offset_days(-1)),
            },
            3,
        ),
    ] {
        let rows = [
            (first.start_ms(tz) - 1, &key),
            (first.start_ms(tz), &key),
            (last.start_ms(tz), &other),
            (last.end_ms(tz) - 1, &key),
            (last.end_ms(tz), &key),
        ]
        .map(|(ms, session)| {
            let mut row = row("X", None, trade_at(ms, 1));
            row.session = Some(session.clone());
            if session == &key {
                row.tags = vec!["setup".to_owned()];
            }
            row
        });
        let mut state = State::default();
        let day_context = request.day().is_some();
        let widened = matches!(request, ReportRequest::Session { .. });
        Model::update(
            &mut state,
            Intent::Scoped(Box::new(PreparedReport {
                receipt: ReportReceipt {
                    request,
                    title: "Context".to_owned(),
                    trades: 1,
                    net_points: Decimal::ONE,
                },
                history: history_rows(rows.to_vec()),
                symbols: vec!["X".to_owned()],
            })),
        );
        state.ensure_report_view(tz, PnlReader::plain());
        let pick = |state: &mut State, day, extend| {
            Model::update(state, Intent::PickDay { day, extend });
        };
        // A set replaces the context's day and anchors the next Shift+click.
        Model::update(&mut state, Intent::Days(DaySet::single(first)));
        pick(&mut state, last, true);
        assert_eq!(state.picked_days(), &DaySet::span(first, last));
        for _ in 0..2 {
            state.ensure_report_view(tz, PnlReader::plain());
            let view = state.report_view().as_ref().unwrap();
            assert_eq!(view.days.len(), 3);
            assert_eq!(view.rows.len(), count);
            assert_eq!(view.report.net_points, Decimal::from(count));
            assert_eq!(view.equity.points.last(), Some(&view.report.net_points));
            assert_eq!(state.report_scope().is_none(), widened);
        }
        Model::update(&mut state, Intent::CustomText("2h".to_owned()));
        Model::update(&mut state, Intent::SubmitCustom { enter: true });
        assert!(state.picked_days().is_empty(), "a period clears the days");
        assert_eq!(state.report_period(), ReportPeriod::Custom(7_200_000));
        pick(&mut state, last, false);
        Model::update(&mut state, Intent::SubmitCustom { enter: false });
        assert_eq!(
            state.picked_days(),
            &DaySet::single(last),
            "tabbing through an unchanged custom field preserves the picked days"
        );
        Model::update(&mut state, Intent::Days(DaySet::default()));
        assert_eq!(state.report_period(), ReportPeriod::Custom(7_200_000));
        assert_eq!(state.report_scope().is_none(), day_context);
        assert_eq!(state.report_source(), SourceFilter::Simulator.into());
    }
}
