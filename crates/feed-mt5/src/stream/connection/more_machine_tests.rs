//! Additional pure page, rates, depth and failure transcripts.
use super::*;
use crate::depth::{BookDiagnostic, BookMapper};

#[test]
fn restarted_unsolicited_pages_restore_context_and_use_current_offset() {
    let (mut machine, mut tape) = admitted();
    tape.input(&mut machine, tick(1, 10000, 100, None));
    tape.input(&mut machine, tick(2, 10001, 101, None));
    for (first, price) in [(3, 90), (5, 80)] {
        tape.input(&mut machine, message(r#"{"type":"history_start"}"#));
        tape.input(&mut machine, tick(first, 5000, price, None));
        tape.input(&mut machine, tick(first + 1, 5001, price + 1, None));
    }
    tape.input(&mut machine, message(r#"{"type":"history_end"}"#));
    assert_eq!(tape.operations, ["query", "query", "settle"]);
    assert_eq!(tape.events.len(), 1); // unsolicited history never becomes live
    tape.owed = true;
    tape.input(&mut machine, message(r#"{"type":"history_start"}"#));
    tape.input(&mut machine, tick(7, 6000, 90, None));
    tape.input(&mut machine, tick(8, 6001, 91, None));
    tape.input(&mut machine,message(r#"{"type":"heartbeat","seq_last":8,"time_ms":12000,"ticks_sent":8,"server_utc_offset_s":3}"#));
    tape.input(
        &mut machine,
        message(r#"{"type":"history_end","exhausted":true,"scanned_to_ms":7000}"#),
    );
    assert!(
        matches!(tape.events.last(),Some(Mt5Event::HistoryPage{trades,exhausted:true,scanned_to_utc_ms:Some(4000)}) if trades.len()==1&&trades[0].agg_id==8&&trades[0].timestamp_ms==4001)
    );
    tape.input(&mut machine, tick(9, 12000, 100, None));
    assert!(
        matches!(tape.events.last(),Some(Mt5Event::Live(trade)) if trade.side==Side::Sell&&trade.timestamp_ms==9000)
    );
}

#[test]
fn rates_restart_partial_and_original_hello_offset_are_pure() {
    let (mut machine, mut tape) = admitted();
    let rate = r#"{"type":"rate","bars":[[120000,"20","22","19","21","3"]]}"#;
    tape.input(&mut machine, message(rate)); // outside block
    tape.input(&mut machine, message(r#"{"type":"rates_end"}"#));
    tape.input(
        &mut machine,
        message(r#"{"type":"rates_start","interval_ms":60000}"#),
    );
    tape.input(&mut machine, message(rate));
    tape.input(&mut machine,message(r#"{"type":"heartbeat","seq_last":0,"time_ms":12000,"ticks_sent":0,"server_utc_offset_s":3}"#));
    tape.input(
        &mut machine,
        message(r#"{"type":"rates_start","interval_ms":60000}"#),
    );
    tape.input(&mut machine, message(rate));
    tape.input(
        &mut machine,
        message(r#"{"type":"rates_end","partial":true}"#),
    );
    assert!(
        matches!(&tape.events[..],[Mt5Event::Rates{interval_ms:60000,bars,partial:true}]
        if bars.len()==1&&bars[0].open_time==118000&&bars[0].close_time==177999&&bars[0].close==rust_decimal::Decimal::from(21))
    );
    tape.input(
        &mut machine,
        message(r#"{"type":"rates_start","interval_ms":60000}"#),
    );
    tape.input(&mut machine, message(rate));
    tape.input(&mut machine, Input::Eof);
    assert_eq!(tape.events.len(), 1);
}

#[test]
fn unsupported_requests_never_write_and_write_failure_abandons_once() {
    let mut machine = bare();
    let mut tape = Tape::default();
    tape.input(
        &mut machine,
        message(&HELLO.replace(r#""history_paging":true"#, r#""history_paging":false"#)),
    );
    tape.events.clear();
    tape.owed = true;
    tape.input(
        &mut machine,
        Input::Request {
            count: 7,
            before_utc_ms: 100,
        },
    );
    assert!(tape.writes.is_empty());
    assert_eq!(tape.operations, ["settle"]);
    assert!(
        matches!(&tape.events[..],[Mt5Event::HistoryPage{trades,exhausted:false,scanned_to_utc_ms:None}] if trades.is_empty())
    );
    let (mut machine, mut tape) = admitted();
    tape.owed = true;
    let effect = machine
        .input(Input::Request {
            count: 7,
            before_utc_ms: 100,
        })
        .unwrap();
    let effect = skip_diagnostics(&mut machine, effect);
    assert!(matches!(
        effect,
        Effect::Write(protocol::FeedMsg::LoadOlder {
            count: 7,
            before_ms: 2100
        })
    ));
    let effect = machine
        .resume(Ack::Written(Err("fixture write reset".into())))
        .unwrap();
    tape.drain(&mut machine, effect);
    assert_eq!(tape.operations, ["abandon"]);
    assert!(!tape.owed);
    assert_eq!(tape.events.len(), 1);
    assert_eq!(
        reason(&mut machine),
        "socket error on write: fixture write reset"
    );
}

#[test]
fn a_failed_latency_publication_never_releases_its_live_trade() {
    let (mut machine, mut tape) = admitted();
    for seq in 1..=64 {
        tape.input(
            &mut machine,
            tick(seq, 10000 + seq as i64, 100 + seq % 2, None),
        );
    }
    let effect = machine.input(tick(65, 10065, 101, None)).unwrap();
    assert!(matches!(effect, Effect::SampleTime));
    let effect = machine.resume(Ack::Time(11000)).unwrap();
    let effect = skip_diagnostics(&mut machine, effect);
    assert!(matches!(
        publication(&mut machine, effect),
        Mt5Event::Latency(_)
    ));
    let effect = machine.resume(Ack::Published(false)).unwrap();
    tape.drain(&mut machine, effect);
    assert_eq!(
        tape.events
            .iter()
            .filter(|e| matches!(e, Mt5Event::Live(_)))
            .count(),
        63
    );
    assert_eq!(
        machine.connected.as_ref().unwrap().mapper.stats.trades(),
        64
    ); // map already happened
}

#[test]
fn quote_only_gap_still_observes_deals_without_inventing_a_print() {
    let (mut machine, mut tape) = admitted();
    tape.input(&mut machine, tick(1, 10000, 100, None));
    tape.events.clear();
    let Input::Message(BridgeMsg::Tick(mut quote)) = tick(3, 11000, 100, Some(7)) else {
        panic!("tick")
    };
    quote.flags = 0;
    tape.input(&mut machine, Input::Message(BridgeMsg::Tick(quote)));
    assert!(
        matches!(&tape.events[..],[Mt5Event::SequenceAnomaly{from_ms:8000,to_ms:9000,..},Mt5Event::DealCounter(sample)] if sample.session_deals==7)
    );
    assert_eq!(
        machine.connected.as_ref().unwrap().mapper.stats.quote_only,
        1
    );
}

#[test]
fn stopping_depth_resets_before_send_and_missing_capability_is_once() {
    let (mut machine, mut tape) = admitted();
    tape.capture = true;
    tape.base = 100;
    tape.input(&mut machine, book(1));
    let _ = machine.input(book(2)).unwrap();
    let effect = machine
        .resume(Ack::Capture {
            enabled: false,
            base_generation: 100,
        })
        .unwrap();
    assert!(matches!(
        publication(&mut machine, effect),
        Mt5Event::Depth(DepthEvent::Status {
            status: DepthStatus::Stopped,
            ..
        })
    ));
    assert!(
        machine
            .connected
            .as_ref()
            .unwrap()
            .depth
            .close()
            .1
            .is_none()
    );
    tape.events.clear();
    let effect = machine.resume(Ack::Published(false)).unwrap();
    tape.drain(&mut machine, effect);
    assert!(tape.events.is_empty());

    let mut machine = bare();
    let mut tape = Tape {
        capture: true,
        base: 500,
        ..Tape::default()
    };
    tape.input(
        &mut machine,
        message(&HELLO.replace(r#","book_levels":5,"tick_size":"1""#, "")),
    );
    tape.events.clear();
    for _ in 0..2 {
        tape.input(
            &mut machine,
            message(r#"{"type":"heartbeat","seq_last":0,"time_ms":10000,"ticks_sent":0}"#),
        );
    }
    assert!(matches!(
        &tape.events[..],
        [Mt5Event::Depth(DepthEvent::Status {
            generation: 500,
            status: DepthStatus::Disconnected {
                error_class: "bridge_without_depth"
            },
            ..
        })]
    ));
    assert_eq!((tape.times, tape.captures), (2, 2));
}

#[test]
fn pure_book_diagnostics_preserve_clock_then_image_and_warn_once() {
    let mut mapper = BookMapper::new("TEST", 1, Some(5), Some("1"), 0);
    let Input::Message(BridgeMsg::Book(mut image)) = book(1) else {
        panic!("book")
    };
    let first = mapper.map_with_diagnostics(&image);
    assert!(matches!(
        first.diagnostics,
        [None, Some(BookDiagnostic::Synchronized { seq: 1, .. })]
    ));
    image.time_ms = 9000;
    image.seq = 2;
    image.bids[0].0 = "105".into();
    let crossed = mapper.map_with_diagnostics(&image);
    assert!(matches!(
        crossed.diagnostics,
        [
            Some(BookDiagnostic::Backwards),
            Some(BookDiagnostic::Crossed {
                seq: 2,
                total: 1,
                ..
            })
        ]
    ));
    let crossed = mapper.map_with_diagnostics(&image);
    assert!(matches!(
        crossed.diagnostics,
        [None, Some(BookDiagnostic::Crossed { total: 2, .. })]
    ));
    let clamp_count = mapper.stats.clamped_timestamps;
    image.time_ms = 8000;
    image.bids[0].0 = "broken".into();
    let malformed = mapper.map_with_diagnostics(&image);
    assert!(matches!(
        malformed.diagnostics,
        [Some(BookDiagnostic::Malformed { .. }), None]
    ));
    assert_eq!(mapper.stats.clamped_timestamps, clamp_count); // malformed returns before timeline
    assert!(
        mapper
            .map_with_diagnostics(&image)
            .diagnostics
            .into_iter()
            .all(|d| d.is_none())
    );
}

#[test]
fn failed_block_publications_close_without_repeating_a_settled_page() {
    for kind in ["backfill", "rates", "opening", "history"] {
        let (mut machine, mut tape) = admitted();
        let (start, end) = match kind {
            "backfill" => (r#"{"type":"backfill_start"}"#, r#"{"type":"backfill_end"}"#),
            "rates" => (
                r#"{"type":"rates_start","interval_ms":60000}"#,
                r#"{"type":"rates_end"}"#,
            ),
            "opening" => (
                r#"{"type":"history_start","opening":true}"#,
                r#"{"type":"history_end"}"#,
            ),
            _ => (r#"{"type":"history_start"}"#, r#"{"type":"history_end"}"#),
        };
        tape.owed = kind == "history";
        tape.input(&mut machine, message(start));
        let effect = machine.input(message(end)).unwrap();
        let mut effect = skip_diagnostics(&mut machine, effect);
        if kind == "history" {
            assert!(matches!(effect, Effect::Pager(PagerOp::SettleOwed)));
            tape.owed = false;
            effect = machine.resume(Ack::Pager(true)).unwrap();
            effect = skip_diagnostics(&mut machine, effect);
        }
        assert!(matches!(effect, Effect::Publish));
        let _ = machine.take_publication().unwrap();
        let effect = machine.resume(Ack::Published(false)).unwrap();
        tape.drain(&mut machine, effect);
        assert!(tape.events.is_empty());
        assert_eq!(machine.phase, Phase::Closed);
        assert_eq!(reason(&mut machine), "consumer gone");
    }
}

#[test]
fn missing_depth_marks_reported_before_failed_send_and_undeclared_book_never_reads_capture() {
    let mut machine = bare();
    let mut tape = Tape {
        capture: true,
        base: 500,
        ..Tape::default()
    };
    tape.input(
        &mut machine,
        message(&HELLO.replace(r#","book_levels":5,"tick_size":"1""#, "")),
    );
    tape.events.clear();
    tape.input(&mut machine, book(1));
    assert_eq!(tape.captures, 0);
    let effect = machine
        .input(message(
            r#"{"type":"heartbeat","seq_last":0,"time_ms":10000,"ticks_sent":0}"#,
        ))
        .unwrap();
    assert!(matches!(effect, Effect::SampleTime));
    assert!(matches!(
        machine.resume(Ack::Time(11000)).unwrap(),
        Effect::ReadCapture
    ));
    let effect = machine
        .resume(Ack::Capture {
            enabled: true,
            base_generation: 500,
        })
        .unwrap();
    let effect = skip_diagnostics(&mut machine, effect);
    assert!(matches!(
        publication(&mut machine, effect),
        Mt5Event::Depth(DepthEvent::Status {
            status: DepthStatus::Disconnected {
                error_class: "bridge_without_depth"
            },
            ..
        })
    ));
    assert!(
        machine
            .connected
            .as_mut()
            .unwrap()
            .depth
            .missing(true, 500)
            .is_none()
    );
    let effect = machine.resume(Ack::Published(false)).unwrap();
    tape.drain(&mut machine, effect);
    assert!(tape.events.is_empty());
    assert_eq!(reason(&mut machine), "consumer gone");
}

#[test]
fn worst_case_cleanup_diagnostics_fit_the_fixed_budget() {
    let (mut machine, mut tape) = admitted();
    tape.capture = true;
    tape.input(&mut machine, book(1));
    tape.input(&mut machine, message(r#"{"type":"backfill_start"}"#));
    tape.input(
        &mut machine,
        message(r#"{"type":"rates_start","interval_ms":60000}"#),
    );
    tape.input(
        &mut machine,
        message(r#"{"type":"bye","reason":"full_cleanup"}"#),
    );
    assert_eq!(machine.phase, Phase::Closed);
    assert_eq!(reason(&mut machine), "bye: full_cleanup");
    assert_eq!(tape.operations, ["abandon"]);
}
