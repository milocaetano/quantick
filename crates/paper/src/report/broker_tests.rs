use super::*;
use crate::scratch::ScratchDir;
use history::SessionSource;
use quantick_trading::{TradeMode, VenueEvent};
use scope::{ReportOrigin, ReportRequest, prepare};
use sessions::SessionKey;

fn write_session(dir: &Path, key: &SessionKey, name: Option<&str>, closes: &[i64]) {
    let path = key.path(dir);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    // Broker journals predating account receipts also said source=live.
    let mut text = history::write_header(key.parts().0, SessionSource::Live);
    if let Some(name) = name {
        text.push_str(&history::write_name(name));
    }
    for &close in closes {
        text.push_str(&history::write_trade(&trade_at(close, 5)));
    }
    std::fs::write(path, text).unwrap();
}

#[test]
fn broker_history_and_simulator_with_identical_names_never_alias() {
    let scratch = ScratchDir::new("broker-origins");
    let dir = scratch.join("paper");
    let key = SessionKey::parse("WINZ26/20261118-124104.csv").unwrap();
    write_session(&dir, &key, None, &[1000]);
    write_session(&dir, &key.for_broker(), Some("Opening"), &[2000]);
    let history = journals::load(&dir, None, &[]);
    assert_eq!(history.files, 2);
    assert_eq!(history.sessions.len(), 2);
    assert_eq!(history.rows[0].source, Some(SessionSource::Live));
    assert_eq!(
        history.rows[1].source,
        Some(SessionSource::Broker(TradeMode::Unknown))
    );
    assert_eq!(history.rows[1].session, Some(key.for_broker()));
    assert_eq!(
        sessions::SessionInfo::read(&dir, &key.for_broker(), false)
            .unwrap()
            .source,
        Some(SessionSource::Broker(TradeMode::Unknown))
    );
    let relative_root = Path::new("paper");
    let active = key.for_broker().path(relative_root);
    assert!(sessions::live_keys(relative_root, &[active]).contains(&key.for_broker()));
    let excluded = journals::load(&dir, None, &[key.path(&dir)]);
    assert_eq!(excluded.rows.len(), 1);
    assert_eq!(excluded.rows[0].session, Some(key.for_broker()));
    assert_eq!(excluded.sessions.len(), 2);
}

#[test]
fn account_receipts_survive_reattachment_reload_and_parked_closes() {
    let scratch = ScratchDir::new("broker-accounts");
    let dir = scratch.join("paper");
    let key = SessionKey::parse("WINZ26/simulator.csv").unwrap();
    write_session(&dir, &key, None, &[500]);
    let original = std::fs::read(key.path(&dir)).unwrap();
    let mut account = crate::PaperAccount::with_trades_dir(dir.clone());
    account.set_symbol("WINZ26");
    for (mode, timestamp) in [(TradeMode::Demo, 1000), (TradeMode::Real, 2000)] {
        let (broker, script) = crate::fake_broker::broker(mode);
        account.attach_broker(broker);
        account.handle_events(vec![VenueEvent::Closed {
            trade: trade_at(timestamp, 5),
            broker_account: Some(mode),
        }]);
        assert!(account.take_journal_changed());
        account.park_broker();
        script.borrow_mut().said.push(VenueEvent::Closed {
            trade: trade_at(timestamp + 1, 5),
            broker_account: Some(mode),
        });
        account.poll_broker(timestamp + 2);
        assert!(
            account.take_journal_changed(),
            "parked closes invalidate history too"
        );
    }
    assert!(account.simulator_trades().is_empty());
    drop(account);
    let history = journals::load(&dir, None, &[]);
    assert_eq!(
        history
            .rows
            .iter()
            .skip(1)
            .map(|row| row.source)
            .collect::<Vec<_>>(),
        [
            TradeMode::Demo,
            TradeMode::Demo,
            TradeMode::Real,
            TradeMode::Real
        ]
        .map(|mode| Some(SessionSource::Broker(mode)))
    );
    assert_eq!(std::fs::read(key.path(&dir)).unwrap(), original);
}

#[test]
fn context_reports_replace_scope_for_named_and_unnamed_sessions_across_origins() {
    let tz = TzOffset::new(-180);
    let day = quantick_civil::parse_iso_date("2026-11-18").unwrap();
    let midnight = day.start_ms(tz);
    for broker in [false, true] {
        for name in [None, Some("My morning")] {
            let scratch = ScratchDir::new("context-report");
            let dir = scratch.join("paper");
            let sim = SessionKey::parse("WINZ26/20261118-124104.csv").unwrap();
            let session = if broker {
                sim.for_broker()
            } else {
                sim.clone()
            };
            let other = SessionKey::parse("WDOZ26/another.csv").unwrap();
            let other = if broker { other.for_broker() } else { other };
            write_session(
                &dir,
                &session,
                name,
                &[midnight - 1, midnight, midnight + DAY_MS - 1],
            );
            write_session(&dir, &other, None, &[midnight + 1000, midnight + DAY_MS]);
            let opposite = if broker { sim } else { sim.for_broker() };
            write_session(&dir, &opposite, name, &[midnight + 2000]);
            let exact = prepare(
                &dir,
                ReportRequest::Session {
                    session: session.clone(),
                    day: None,
                },
                tz,
            )
            .unwrap();
            assert_eq!(
                exact.receipt.trades, 3,
                "the full session includes its previous day"
            );
            let cut = ReportView::cut_scoped(
                &exact.history,
                ReportPeriod::All,
                SourceFilter::All,
                &Default::default(),
                tz,
                Some(&exact.receipt.request),
            );
            assert!(
                cut.rows
                    .iter()
                    .all(|row| row.session.as_ref() == Some(&session))
            );
            let whole_day = prepare(
                &dir,
                ReportRequest::Session {
                    session: session.clone(),
                    day: Some(day),
                },
                tz,
            )
            .unwrap();
            assert_eq!(
                whole_day.receipt.trades, 3,
                "two sessions and two symbols on exactly the local day"
            );
            let cut = ReportView::cut_scoped(
                &whole_day.history,
                ReportPeriod::All,
                SourceFilter::All,
                &Default::default(),
                tz,
                Some(&whole_day.receipt.request),
            );
            assert_eq!(cut.rows.len(), 3);
            assert_eq!(
                cut.hidden_by_source_or_scope, 3,
                "All sources still excludes trades outside the origin and day"
            );
            assert!(
                cut.rows
                    .iter()
                    .all(|row| row.session.as_ref().unwrap().is_broker() == broker)
            );
            let invalid_day = CivilDate::from_day_number(day.day_number() + 3);
            assert!(matches!(
                prepare(
                    &dir,
                    ReportRequest::Session {
                        session,
                        day: Some(invalid_day)
                    },
                    tz
                ),
                Err(session_edit::SessionEditError::DayNotInSession(_))
            ));
        }
    }
}

#[test]
fn broker_session_edits_and_live_protection_address_only_the_broker_file() {
    let scratch = ScratchDir::new("broker-edits");
    let dir = scratch.join("paper");
    let sim = SessionKey::parse("WINZ26/session.csv").unwrap();
    let broker = sim.for_broker();
    write_session(&dir, &sim, None, &[1000]);
    write_session(&dir, &broker, None, &[2000]);
    session_edit::rename_session(&dir, &broker, "Opening").unwrap();
    assert!(
        sessions::SessionInfo::read(&dir, &sim, false)
            .unwrap()
            .name
            .is_none()
    );
    assert!(matches!(
        session_edit::delete_session(&dir, &broker, &[broker.path(&dir)]),
        Err(session_edit::SessionEditError::LiveSession(_))
    ));
    let moved = session_edit::delete_session(&dir, &broker, &[]).unwrap();
    assert!(moved.starts_with(journals::broker_dir(&dir).join(".deleted")));
    assert_eq!(journals::load(&dir, None, &[]).rows.len(), 1);
    for invalid in [
        "broker/../a.csv",
        "broker/X/../a.csv",
        "broker/X/a.csv/extra",
        "broker/C:/a.csv",
    ] {
        assert!(SessionKey::parse(invalid).is_none(), "{invalid}");
    }
    assert!(
        !SessionKey::parse("broker/a.csv").unwrap().is_broker(),
        "broker may be a simulator symbol"
    );
}

#[test]
fn a_whole_day_report_reads_every_session_of_one_origin_and_never_the_other() {
    let tz = TzOffset::new(-180);
    let day = quantick_civil::parse_iso_date("2026-11-18").unwrap();
    let midnight = day.start_ms(tz);
    let scratch = ScratchDir::new("whole-day-report");
    let dir = scratch.join("paper");
    let morning = SessionKey::parse("WINZ26/morning.csv").unwrap();
    let afternoon = SessionKey::parse("WDOZ26/afternoon.csv").unwrap();
    write_session(
        &dir,
        &morning,
        None,
        &[midnight - 1, midnight, midnight + 1000],
    );
    write_session(
        &dir,
        &afternoon,
        None,
        &[midnight + 2000, midnight + DAY_MS],
    );
    write_session(&dir, &morning.for_broker(), None, &[midnight + 3000]);
    for (origin, trades, title) in [
        (ReportOrigin::Simulator, 3, "Simulator · 2026-11-18"),
        (ReportOrigin::Broker, 1, "Broker · 2026-11-18"),
    ] {
        let prepared = prepare(&dir, ReportRequest::Day { origin, day }, tz).unwrap();
        assert_eq!(
            prepared.receipt.trades, trades,
            "{origin:?}: both sessions, one day"
        );
        assert_eq!(prepared.receipt.title, title);
        assert_eq!(
            prepared.receipt.net_points,
            Decimal::from(5 * trades as i64)
        );
        let cut = ReportView::cut_scoped(
            &prepared.history,
            ReportPeriod::All,
            SourceFilter::All,
            &Default::default(),
            tz,
            Some(&prepared.receipt.request),
        );
        assert_eq!(cut.rows.len(), trades);
        assert!(cut.rows.iter().all(|row| {
            row.session.as_ref().map(ReportOrigin::of) == Some(origin)
                && CivilDate::from_ms(row.trade.closed_ms, tz) == day
        }));
    }
    let empty = CivilDate::from_day_number(day.day_number() + 5);
    assert_eq!(
        prepare(
            &dir,
            ReportRequest::Day {
                origin: ReportOrigin::Simulator,
                day: empty
            },
            tz
        )
        .err()
        .map(|error| error.code()),
        Some("no_trades_in_scope"),
        "an empty day is refused, never an empty report"
    );
}

#[test]
fn a_tag_report_sums_one_origin_across_days_and_sessions() {
    let tz = TzOffset::new(-180);
    let midnight = quantick_civil::parse_iso_date("2026-11-18")
        .unwrap()
        .start_ms(tz);
    let scratch = ScratchDir::new("tag-report");
    let dir = scratch.join("paper");
    let first = SessionKey::parse("WINZ26/first.csv").unwrap();
    let second = SessionKey::parse("WDOZ26/second.csv").unwrap();
    let broker = first.for_broker();
    write_session(
        &dir,
        &first,
        None,
        &[midnight + 1000, midnight + DAY_MS + 1000],
    );
    write_session(&dir, &second, None, &[midnight + 2000, midnight + 3000]);
    write_session(&dir, &broker, None, &[midnight + 4000]);
    let history = journals::load(&dir, None, &[]);
    let tagged = [
        (midnight + 1000, "#Breakout"),
        (midnight + DAY_MS + 1000, "breakout"),
        (midnight + 2000, "#BREAKOUT"),
        (midnight + 3000, "#fade"),
        (midnight + 4000, "#breakout"),
    ];
    for (closed, tag) in tagged {
        let row = history
            .rows
            .iter()
            .find(|row| row.trade.closed_ms == closed)
            .unwrap();
        let key = row.session.as_ref().unwrap();
        session_edit::tag_trade(&dir, key, row.trade_ref, &[tag]).unwrap();
    }
    let open = |origin, tag: &str| {
        prepare(
            &dir,
            ReportRequest::Tag {
                origin,
                tag: tag.to_owned(),
            },
            tz,
        )
    };
    let simulator = open(ReportOrigin::Simulator, "#Breakout").unwrap();
    assert_eq!(simulator.receipt.trades, 3, "two days, two sessions");
    assert_eq!(simulator.receipt.net_points, Decimal::from(15));
    assert_eq!(simulator.receipt.title, "Simulator · #breakout");
    assert_eq!(
        simulator.receipt.request.menu_label(),
        "Report #breakout",
        "the tag is held as the journal stores it"
    );
    let cut = ReportView::cut_scoped(
        &simulator.history,
        ReportPeriod::All,
        SourceFilter::All,
        &Default::default(),
        tz,
        Some(&simulator.receipt.request),
    );
    assert_eq!(cut.rows.len(), 3);
    assert!(
        cut.rows
            .iter()
            .all(|row| { !row.session.as_ref().unwrap().is_broker() && row.tags == ["breakout"] })
    );
    assert_eq!(
        open(ReportOrigin::Broker, "breakout")
            .unwrap()
            .receipt
            .trades,
        1
    );
    let refused = |origin, tag| open(origin, tag).err().map(|error| error.code());
    assert_eq!(
        refused(ReportOrigin::Broker, "fade"),
        Some("no_trades_in_scope")
    );
    assert_eq!(
        refused(ReportOrigin::Simulator, "two words"),
        Some("invalid_tag")
    );
}

#[test]
fn report_menus_offer_one_scope_per_button_with_its_label() {
    let day = quantick_civil::parse_iso_date("2026-11-18").unwrap();
    let broker = SessionKey::parse("broker/WINZ26/a.csv").unwrap();
    let labels = |requests: Vec<ReportRequest>| {
        requests
            .iter()
            .map(ReportRequest::menu_label)
            .collect::<Vec<_>>()
    };
    let session = ReportRequest::sessions(&broker, day);
    assert_eq!(session[1].day(), Some(day));
    assert_eq!(labels(session.to_vec()), ["Session report", "Day report"]);
    let origins = [ReportOrigin::Simulator, ReportOrigin::Broker];
    assert_eq!(
        labels(ReportRequest::days(day, &origins).collect()),
        ["Day report · Simulator", "Day report · Broker"]
    );
    let tags = ["breakout".to_owned(), "fade".to_owned()];
    let tagged: Vec<_> = ReportRequest::tags(&broker, &tags).collect();
    assert!(
        tagged
            .iter()
            .all(|request| request.origin() == ReportOrigin::Broker)
    );
    assert_eq!(labels(tagged), ["Report #breakout", "Report #fade"]);
}

#[test]
fn the_symbol_filter_reads_each_chosen_folder_from_both_journals() {
    let scratch = ScratchDir::new("symbol-filter");
    let dir = scratch.join("paper");
    let win = SessionKey::parse("WINZ26/simulator.csv").unwrap();
    let btc = SessionKey::parse("BTCUSDT/simulator.csv").unwrap();
    let eth = SessionKey::parse("ETHUSDT/simulator.csv").unwrap();
    write_session(&dir, &win, None, &[3000]);
    write_session(&dir, &win.for_broker(), None, &[1000]);
    write_session(&dir, &btc, None, &[2000]);
    write_session(&dir, &eth, None, &[4000]);
    let chosen = ["BTCUSDT", "WINZ26"].map(str::to_owned).into();
    let history = journals::load_symbols(&dir, &chosen);
    let closes: Vec<_> = history
        .rows
        .iter()
        .map(|row| (row.symbol.as_str(), row.trade.closed_ms))
        .collect();
    assert_eq!(
        closes,
        vec![("WINZ26", 1000), ("BTCUSDT", 2000), ("WINZ26", 3000)],
        "broker and simulator WIN, BTC, in closing order; ETH left out"
    );
    assert_eq!((history.files, history.sessions.len()), (3, 3));
    let all = journals::load_symbols(&dir, &Default::default());
    assert_eq!(all.rows.len(), 4, "an empty choice reads every symbol");
}
