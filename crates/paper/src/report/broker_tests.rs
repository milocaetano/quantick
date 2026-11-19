use super::*;
use crate::scratch::ScratchDir;
use history::SessionSource;
use quantick_trading::{TradeMode, VenueEvent};
use scope::{ReportRequest, prepare};
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
                ReportRequest {
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
                None,
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
                ReportRequest {
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
                None,
                tz,
                Some(&whole_day.receipt.request),
            );
            assert_eq!(cut.rows.len(), 3);
            assert!(
                cut.rows
                    .iter()
                    .all(|row| row.session.as_ref().unwrap().is_broker() == broker)
            );
            let invalid_day = CivilDate::from_day_number(day.day_number() + 3);
            assert!(matches!(
                prepare(
                    &dir,
                    ReportRequest {
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
