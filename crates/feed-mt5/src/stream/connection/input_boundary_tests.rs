//! Admission guards preserve both the issued value and its original continuation.
use super::*;
use std::mem::discriminant;

const HELLO: &str = r#"{"type":"hello","schema":1,"bridge":"fixture","bridge_version":"0","symbol":"TEST","broker_symbol":"TEST.raw","digits":0,"server_utc_offset_s":2,"history_paging":true,"rates":true,"book_levels":5,"tick_size":"1"}"#;

fn msg(line: &str) -> Input {
    Input::Message(protocol::parse_line(line).unwrap())
}
fn tick(seq: u64) -> Input {
    msg(&format!(
        r#"{{"type":"tick","seq":{seq},"time_ms":{},"bid":"0","ask":"0","last":"{}","volume":1,"flags":56}}"#,
        10000 + seq,
        100 + seq % 2
    ))
}
fn new() -> SessionMachine<'static> {
    SessionMachine::new("TEST", SideMode::TickRule, 7, 10, 30)
}

// No raw bytes, production Clone/Debug derives, or draining state to compare it.
// Opaque history/depth/rates details are also checked by their later outputs.
fn snapshot(m: &SessionMachine<'_>) -> String {
    let connected = m.connected.as_ref().map(|s| {
        format!(
            "{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{}|{:?}|{:?}|{}|{:?}|{}",
            s.mapper,
            s.hello,
            s.tracker,
            s.highest_tick_ms,
            s.latency,
            s.deals,
            s.lag_reported,
            s.backfill,
            s.candles.as_ref().map(|b| (b.len(), b.stats())),
            s.history.len(),
            s.depth.close(),
            s.undecodable
        )
    });
    format!(
        "{:?}|{}|{:?}|{:?}|{:?}|{:?}|{}|{}|{:?}|{:?}",
        m.phase,
        m.generation_offset,
        connected,
        m.end.as_ref().map(|end| match end {
            ConnEnd::UiGone => ("ui", ""),
            ConnEnd::BridgeGone(reason) => ("bridge", reason.as_str()),
        }),
        m.pending
            .as_ref()
            .map(|(expect, after)| (discriminant(expect), discriminant(after))),
        m.diagnostics
            .each_ref()
            .map(|d| d.as_ref().map(discriminant)),
        m.diagnostic_count,
        m.diagnostic_taken,
        m.publication,
        m.deferred.as_ref().map(discriminant)
    )
}
fn reject(m: &mut SessionMachine<'_>) {
    let before = snapshot(m);
    // Would advance sequence/high-water, change mapper context, and observe deals
    // if the Connected Tick branch ran before the centralized guard.
    let input = msg(
        r#"{"type":"tick","seq":999,"time_ms":999999,"bid":"0","ask":"0","last":"999","volume":9,"flags":56,"deals":999,"sent_ms":999998}"#,
    );
    assert!(matches!(m.input(input), Err(InvalidInput)));
    assert_eq!(snapshot(m), before);
}
fn diagnostic(
    m: &mut SessionMachine<'_>,
    effect: Effect,
    check: impl FnOnce(Diagnostic),
) -> Effect {
    assert!(matches!(effect, Effect::Diagnostic));
    reject(m);
    check(m.take_diagnostic().unwrap());
    reject(m);
    let effect = m.resume(Ack::Diagnostic).unwrap();
    if m.deferred.is_some() {
        reject(m);
    }
    effect
}
fn published(m: &mut SessionMachine<'_>, effect: Effect, check: impl FnOnce(Mt5Event)) -> Effect {
    assert!(matches!(effect, Effect::Publish));
    reject(m);
    check(m.take_publication().unwrap());
    reject(m); // take is not acknowledgement
    m.resume(Ack::Published(true)).unwrap()
}
fn wait(effect: Effect) {
    assert!(matches!(effect, Effect::Wait(Phase::Connected)));
}
fn connected() -> SessionMachine<'static> {
    let mut m = new();
    let effect = m.input(msg(HELLO)).unwrap();
    let effect = diagnostic(&mut m, effect, |d| {
        assert!(matches!(d, Diagnostic::Hello(h) if h.symbol == "TEST"))
    });
    assert!(m.connected.is_none());
    let effect = published(&mut m, effect, |e| {
        assert!(
            matches!(e, Mt5Event::Status(Mt5Status::Connected { symbol, .. }) if symbol == "TEST")
        )
    });
    let effect = diagnostic(&mut m, effect, |d| {
        assert!(matches!(
            d,
            Diagnostic::Capabilities {
                paging: true,
                depth: true,
                ..
            }
        ))
    });
    wait(effect);
    m
}
fn live(m: &mut SessionMachine<'_>, effect: Effect, id: u64) {
    assert!(matches!(effect, Effect::Live(t) if t.agg_id == id));
    reject(m);
    wait(m.resume(Ack::Published(true)).unwrap());
}

#[test]
fn input_boundary_prehello_tick_keeps_first_message_and_no_hello_end() {
    let mut m = new();
    let effect = m.input(tick(1)).unwrap();
    assert_eq!(m.phase, Phase::Closed);
    assert!(m.pending.is_none());
    let effect = diagnostic(&mut m, effect, |d| {
        assert!(
            matches!(d, Diagnostic::FirstMessage(BridgeMsg::Tick(t)) if t.seq == 1 && t.time_ms == 10001)
        );
    });
    assert!(matches!(effect, Effect::Finished));
    reject(&mut m);
    assert!(matches!(m.take_end(), ConnEnd::BridgeGone(reason) if reason == "no hello"));
    reject(&mut m);
    assert!(m.connected.is_none());
    assert_eq!(m.generation_offset, 7);
}

#[test]
fn input_boundary_hello_publication_and_connected_live_take_do_not_acknowledge() {
    let mut m = connected(); // checks Hello diagnostic, payload and admission ack
    wait(m.input(tick(1)).unwrap());
    let effect = m.input(tick(2)).unwrap();
    live(&mut m, effect, 2);
    assert_eq!(m.state().tracker.highest(), Some(2));
    assert_eq!(m.state().highest_tick_ms, Some(8002));
    let effect = m.input(tick(3)).unwrap();
    live(&mut m, effect, 3);
}

#[test]
fn input_boundary_anomaly_publication_retains_owned_tick_until_ack() {
    let mut m = connected();
    wait(m.input(tick(1)).unwrap());
    let effect = m.input(tick(3)).unwrap();
    assert_eq!(m.state().highest_tick_ms, Some(8001));
    let effect = published(&mut m, effect, |e| {
        assert!(matches!(
            e,
            Mt5Event::SequenceAnomaly {
                from_ms: 8001,
                to_ms: 8003,
                anomaly: crate::session::SeqAnomaly::Gap {
                    expected: 2,
                    got: 3,
                    missing: 1
                }
            }
        ));
    });
    // The repeated price is still the original first-price context, not 999.
    wait(effect);
    assert_eq!(m.state().tracker.highest(), Some(3));
    assert_eq!(m.state().highest_tick_ms, Some(8003));
    let effect = m.input(tick(4)).unwrap();
    live(&mut m, effect, 4);
}

#[test]
fn input_boundary_write_and_its_deferred_diagnostic_keep_original_request() {
    let mut m = connected();
    let effect = m
        .input(Input::Request {
            count: 17,
            before_utc_ms: 8000,
        })
        .unwrap();
    let effect = diagnostic(&mut m, effect, |d| {
        assert!(matches!(
            d,
            Diagnostic::Request {
                count: 17,
                before_ms: 10000,
                before_utc_ms: 8000
            }
        ))
    });
    assert!(matches!(
        effect,
        Effect::Write(protocol::FeedMsg::LoadOlder {
            count: 17,
            before_ms: 10000
        })
    ));
    reject(&mut m);
    wait(m.resume(Ack::Written(Ok(()))).unwrap());
    wait(m.input(tick(1)).unwrap());
}

#[test]
fn input_boundary_time_then_latency_publication_then_original_live() {
    let mut m = connected();
    wait(m.input(tick(1)).unwrap());
    for id in 2..65 {
        let effect = m.input(tick(id)).unwrap();
        live(&mut m, effect, id);
    }
    let effect = m.input(tick(65)).unwrap();
    assert!(matches!(effect, Effect::SampleTime));
    reject(&mut m);
    let effect = m.resume(Ack::Time(11000)).unwrap();
    let effect = diagnostic(&mut m, effect, |d| {
        assert!(matches!(d, Diagnostic::Lag { late:true, sample } if sample.prints == 64))
    });
    let effect = published(&mut m, effect, |e| {
        assert!(matches!(e, Mt5Event::Latency(s) if s.prints == 64))
    });
    live(&mut m, effect, 65);
    assert_eq!(m.state().tracker.highest(), Some(65));
}

#[test]
fn input_boundary_capture_and_depth_publications_preserve_generation() {
    let mut m = connected();
    let effect = m
        .input(msg(
            r#"{"type":"book","seq":1,"time_ms":10000,"bids":[["100","2"]],"asks":[["102","3"]]}"#,
        ))
        .unwrap();
    assert!(matches!(effect, Effect::ReadCapture));
    reject(&mut m);
    let effect = m
        .resume(Ack::Capture {
            enabled: true,
            base_generation: 100,
        })
        .unwrap();
    let effect = published(&mut m, effect, |e| {
        assert!(matches!(
            e,
            Mt5Event::Depth(quantick_orderbook::DepthEvent::Status {
                generation: 108,
                status: quantick_orderbook::DepthStatus::Connecting,
                ..
            })
        ))
    });
    let effect = diagnostic(&mut m, effect, |d| {
        assert!(matches!(
            d,
            Diagnostic::Book(crate::depth::BookDiagnostic::Synchronized { seq: 1, .. })
        ))
    });
    let effect = published(&mut m, effect, |e| {
        assert!(matches!(
            e,
            Mt5Event::Depth(quantick_orderbook::DepthEvent::Snapshot {
                generation: 108,
                ..
            })
        ))
    });
    let effect = published(&mut m, effect, |e| {
        assert!(matches!(
            e,
            Mt5Event::Depth(quantick_orderbook::DepthEvent::Status {
                generation: 108,
                status: quantick_orderbook::DepthStatus::Synchronized { .. },
                ..
            })
        ))
    });
    wait(effect);
    assert_eq!(m.generation_offset, 8);
    assert_eq!(m.state().depth.close().0.unwrap().images, 1);
}

#[test]
fn input_boundary_pager_retains_history_and_parked_live_price_context() {
    let mut m = connected();
    wait(m.input(tick(1)).unwrap());
    let effect = m
        .input(msg(r#"{"type":"history_start","count_hint":2}"#))
        .unwrap();
    assert!(matches!(effect, Effect::Pager(PagerOp::IsInFlight)));
    reject(&mut m);
    let effect = m.resume(Ack::Pager(true)).unwrap();
    let effect = diagnostic(&mut m, effect, |d| {
        assert!(matches!(
            d,
            Diagnostic::HistoryStart {
                count_hint: Some(2),
                unsolicited: false
            }
        ))
    });
    wait(effect);
    wait(m.input(tick(2)).unwrap());
    wait(m.input(tick(3)).unwrap());
    let effect = m
        .input(msg(
            r#"{"type":"history_end","exhausted":true,"scanned_to_ms":5000}"#,
        ))
        .unwrap();
    assert!(matches!(effect, Effect::Pager(PagerOp::SettleOwed)));
    reject(&mut m);
    let effect = m.resume(Ack::Pager(true)).unwrap();
    let effect = diagnostic(&mut m, effect, |d| {
        assert!(matches!(
            d,
            Diagnostic::HistoryEnd {
                trades: 1,
                exhausted: true,
                scanned_to_ms: Some(5000)
            }
        ))
    });
    let effect = published(&mut m, effect, |e| {
        assert!(
            matches!(e, Mt5Event::HistoryPage { trades, exhausted:true, scanned_to_utc_ms:Some(3000) } if trades.len()==1 && trades[0].agg_id==3)
        )
    });
    wait(effect);
    let effect = m.input(tick(4)).unwrap();
    assert!(matches!(effect, Effect::Live(ref t) if t.side == quantick_engine::Side::Sell));
    live(&mut m, effect, 4);
}

#[test]
fn input_boundary_multiple_deferred_diagnostics_before_wait_preserve_rates() {
    let mut m = connected();
    let effect = m
        .input(msg(r#"{"type":"rates_start","interval_ms":60000}"#))
        .unwrap();
    let effect = diagnostic(&mut m, effect, |d| {
        assert!(matches!(
            d,
            Diagnostic::RatesStart {
                interval_ms: 60000,
                count_hint: None
            }
        ))
    });
    wait(effect);
    let effect = m
        .input(msg(
            r#"{"type":"rates_start","interval_ms":1000,"count_hint":4}"#,
        ))
        .unwrap();
    assert!(m.pending.is_none());
    let effect = diagnostic(&mut m, effect, |d| {
        assert!(matches!(
            d,
            Diagnostic::RatesStart {
                interval_ms: 1000,
                count_hint: Some(4)
            }
        ))
    });
    assert_eq!(m.diagnostic_count, 1);
    let effect = diagnostic(&mut m, effect, |d| {
        assert!(matches!(
            d,
            Diagnostic::Violation {
                message: "a second rates_start arrived inside an open block; discarding the first",
                bars: 0,
                action: "discard_open_block"
            }
        ))
    });
    wait(effect);
    let effect = m
        .input(msg(r#"{"type":"rates_end","partial":true}"#))
        .unwrap();
    let effect = diagnostic(&mut m, effect, |d| {
        assert!(matches!(
            d,
            Diagnostic::RatesSummary {
                interval_ms: 1000,
                ..
            }
        ))
    });
    let effect = diagnostic(&mut m, effect, |d| {
        assert!(matches!(
            d,
            Diagnostic::RatesPartial {
                bars: 0,
                bridge: true,
                clipped: false
            }
        ))
    });
    let effect = published(&mut m, effect, |e| {
        assert!(
            matches!(e, Mt5Event::Rates { interval_ms:1000, bars, partial:true } if bars.is_empty())
        )
    });
    wait(effect);
}

#[test]
fn input_boundary_closing_and_closed_keep_cleanup_and_terminal_reason() {
    let mut m = connected();
    let effect = m
        .input(msg(r#"{"type":"backfill_start","count_hint":2}"#))
        .unwrap();
    let effect = diagnostic(&mut m, effect, |d| {
        assert!(matches!(d, Diagnostic::BackfillStart(Some(2))))
    });
    wait(effect);
    let effect = m.input(Input::Eof).unwrap();
    assert_eq!(m.phase, Phase::Closing);
    let effect = diagnostic(&mut m, effect, |d| {
        assert!(matches!(d, Diagnostic::Eof { hello: false }))
    });
    let effect = diagnostic(&mut m, effect, |d| {
        assert!(matches!(d, Diagnostic::BackfillDiscard))
    });
    assert!(matches!(effect, Effect::Pager(PagerOp::Abandon)));
    reject(&mut m);
    let effect = m.resume(Ack::Pager(false)).unwrap();
    assert_eq!(m.phase, Phase::Closed);
    assert!(m.pending.is_none());
    let effect = diagnostic(&mut m, effect, |d| {
        assert!(matches!(d, Diagnostic::MapSummary(_)))
    });
    let effect = diagnostic(&mut m, effect, |d| {
        assert!(matches!(d, Diagnostic::BookSummary(_)))
    });
    assert!(matches!(effect, Effect::Finished));
    reject(&mut m);
    assert!(matches!(m.take_end(), ConnEnd::BridgeGone(reason) if reason == "eof"));
    reject(&mut m);
}

#[test]
fn input_boundary_terminal_phase_guard_is_independent_of_pending_guard() {
    // Explicit white-box phase cross-product, including states a valid driver
    // cannot produce, proves neither part of the centralized guard substitutes
    // for the other. Retain and finish the original continuation afterward.
    for phase in [Phase::Closing, Phase::Closed] {
        for pending in [false, true] {
            let mut m = connected();
            wait(m.input(tick(1)).unwrap());
            if pending {
                assert!(matches!(m.input(tick(2)).unwrap(), Effect::Live(_)));
            }
            m.phase = phase;
            m.end = Some(ConnEnd::BridgeGone("retained terminal".into()));
            reject(&mut m);
            assert!(
                matches!(m.take_end(), ConnEnd::BridgeGone(reason) if reason == "retained terminal")
            );
            m.phase = Phase::Connected;
            if pending {
                wait(m.resume(Ack::Published(true)).unwrap());
            }
            let id = if pending { 3 } else { 2 };
            let effect = m.input(tick(id)).unwrap();
            live(&mut m, effect, id);
        }
    }
}
