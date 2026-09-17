//! Pure protocol transcripts; expectations are literal and do not call the dispatcher.
#[path = "machine_bench.rs"]
mod benchmark;
#[path = "more_machine_tests.rs"]
mod more;
#[path = "payload_tests.rs"]
mod payload;
use super::super::events::{ConnEnd, Mt5Event, Mt5Status};
use super::{effect::*, session::SessionMachine};
use crate::{
    map::SideMode,
    protocol::{self, BridgeMsg},
};
use quantick_engine::Side;
use quantick_orderbook::{DepthEvent, DepthStatus};

const HELLO: &str = r#"{"type":"hello","schema":1,"bridge":"fixture","bridge_version":"0","symbol":"TEST","broker_symbol":"TEST.raw","digits":0,"server_utc_offset_s":2,"history_paging":true,"rates":true,"deal_counter":true,"book_levels":5,"tick_size":"1"}"#;

fn message(line: &str) -> Input {
    Input::Message(protocol::parse_line(line).unwrap())
}
fn tick(seq: u64, time: i64, price: u64, deals: Option<u64>) -> Input {
    let BridgeMsg::Tick(mut tick)=protocol::parse_line(&format!(
        r#"{{"type":"tick","seq":{seq},"time_ms":{time},"bid":"0","ask":"0","last":"{price}","volume":1,"flags":56}}"#
    )).unwrap() else {panic!("tick")};
    tick.deals = deals;
    Input::Message(BridgeMsg::Tick(tick))
}
fn book(seq: u64) -> Input {
    message(&format!(
        r#"{{"type":"book","seq":{seq},"time_ms":10000,"bids":[["100","2"]],"asks":[["102","3"]]}}"#
    ))
}
fn bare() -> SessionMachine<'static> {
    SessionMachine::new("TEST", SideMode::TickRule, 0, 10, 30)
}
fn skip_diagnostics(machine: &mut SessionMachine<'_>, mut effect: Effect) -> Effect {
    while matches!(effect, Effect::Diagnostic) {
        let _ = machine.take_diagnostic().unwrap();
        effect = machine.resume(Ack::Diagnostic).unwrap();
    }
    effect
}
fn publication(machine: &mut SessionMachine<'_>, effect: Effect) -> Mt5Event {
    assert!(matches!(effect, Effect::Publish));
    machine.take_publication().unwrap()
}
#[derive(Default)]
struct Tape {
    events: Vec<Mt5Event>,
    operations: Vec<&'static str>,
    writes: Vec<protocol::FeedMsg>,
    owed: bool,
    capture: bool,
    base: u64,
    times: usize,
    captures: usize,
}
impl Tape {
    fn drain(&mut self, machine: &mut SessionMachine<'_>, mut effect: Effect) {
        loop {
            let ack = match effect {
                Effect::Wait(_) | Effect::Finished => return,
                Effect::Diagnostic => {
                    let _ = machine.take_diagnostic().unwrap();
                    Ack::Diagnostic
                }
                Effect::Publish => {
                    self.events.push(machine.take_publication().unwrap());
                    Ack::Published(true)
                }
                Effect::Live(trade) => {
                    self.events.push(Mt5Event::Live(trade));
                    Ack::Published(true)
                }
                Effect::SampleTime => {
                    self.times += 1;
                    Ack::Time(11000)
                }
                Effect::ReadCapture => {
                    self.captures += 1;
                    Ack::Capture {
                        enabled: self.capture,
                        base_generation: self.base,
                    }
                }
                Effect::Write(message) => {
                    self.writes.push(message);
                    Ack::Written(Ok(()))
                }
                Effect::Pager(operation) => {
                    let owed = self.owed;
                    self.operations.push(match operation {
                        PagerOp::IsInFlight => "query",
                        PagerOp::SettleOwed => {
                            self.owed = false;
                            "settle"
                        }
                        PagerOp::Abandon => {
                            self.owed = false;
                            "abandon"
                        }
                    });
                    Ack::Pager(owed)
                }
            };
            effect = machine.resume(ack).unwrap();
        }
    }
    fn input(&mut self, machine: &mut SessionMachine<'_>, input: Input) {
        let effect = machine.input(input).unwrap();
        self.drain(machine, effect);
    }
}
fn admitted() -> (SessionMachine<'static>, Tape) {
    let mut machine = bare();
    let mut tape = Tape::default();
    tape.input(&mut machine, message(HELLO));
    assert!(matches!(
        &tape.events[..],
        [Mt5Event::Status(Mt5Status::Connected {
            history_paging: true,
            ..
        })]
    ));
    tape.events.clear();
    (machine, tape)
}
fn reason(machine: &mut SessionMachine<'_>) -> String {
    match machine.take_end() {
        ConnEnd::BridgeGone(reason) => reason,
        ConnEnd::UiGone => "consumer gone".into(),
    }
}

#[test]
#[ignore = "manual layout observation before and after private representation changes"]
fn observe_private_session_layout() {
    println!(
        "{{\"effect\":{},\"result_effect\":{},\"after\":{},\"ack\":{},\"machine\":{},\"event\":{},\"diagnostic\":{},\"trade\":{}}}",
        std::mem::size_of::<Effect>(),
        std::mem::size_of::<Result<Effect, InvalidInput>>(),
        std::mem::size_of::<super::session::After>(),
        std::mem::size_of::<Ack>(),
        std::mem::size_of::<SessionMachine<'_>>(),
        std::mem::size_of::<Mt5Event>(),
        std::mem::size_of::<super::diagnostic::Diagnostic>(),
        std::mem::size_of::<quantick_engine::Trade>(),
    );
}

#[test]
fn blank_admission_keeps_the_parser_diagnostic_but_connected_blanks_are_ignored() {
    for (line, error) in [
        ("", "EOF while parsing a value at line 1 column 0"),
        ("  ", "EOF while parsing a value at line 1 column 2"),
    ] {
        let mut machine = bare();
        let input = super::decode(Ok(super::BoundedLine::Line(line.into())));
        let effect = machine.input(input).unwrap();
        assert!(matches!(effect, Effect::Diagnostic));
        let super::diagnostic::Diagnostic::Undecodable {
            hello,
            error: actual_error,
            snippet,
            total,
        } = machine.take_diagnostic().unwrap()
        else {
            panic!("blank hello must retain its parse diagnostic");
        };
        assert!(hello);
        assert_eq!(actual_error, protocol::ParseError::Malformed(error.into()));
        assert_eq!(snippet, line);
        assert_eq!(total, 0);
        assert!(matches!(
            machine.resume(Ack::Diagnostic).unwrap(),
            Effect::Finished
        ));
        assert_eq!(reason(&mut machine), "undecodable hello");

        let (mut machine, _) = admitted();
        let input = super::decode(Ok(super::BoundedLine::Line(line.into())));
        assert!(matches!(
            machine.input(input).unwrap(),
            Effect::Wait(Phase::Connected)
        ));
        assert_eq!(machine.connected.as_ref().unwrap().undecodable, 0);
    }
}

#[test]
fn admission_faults_and_connected_faults_keep_distinct_literal_reasons() {
    let cases = [
        (Input::Eof, "closed before hello"),
        (Input::Timeout, "hello timeout"),
        (Input::Oversized, "oversized hello"),
        (Input::NonUtf8 { len: 2 }, "undecodable hello"),
        (
            Input::ReadError("fixture reset".into()),
            "socket error before hello: fixture reset",
        ),
        (message(r#"{"type":"bye","reason":"x"}"#), "no hello"),
        (
            message(&HELLO.replace(r#""schema":1"#, r#""schema":2"#)),
            "schema mismatch (bridge 2)",
        ),
        (
            message(&HELLO.replace(r#""symbol":"TEST""#, r#""symbol":"OTHER""#)),
            "symbol mismatch (OTHER)",
        ),
    ];
    for (input, expected) in cases {
        let mut machine = bare();
        let mut tape = Tape {
            owed: true,
            ..Tape::default()
        };
        tape.input(&mut machine, input);
        assert_eq!(machine.phase, Phase::Closed);
        assert_eq!(reason(&mut machine), expected);
        assert!(tape.events.is_empty());
        assert!(tape.operations.is_empty());
        assert!(tape.owed);
    }
    for (input, expected) in [
        (Input::Eof, "eof"),
        (Input::Timeout, "silent"),
        (Input::Oversized, "oversized line"),
        (
            Input::ReadError("fixture reset".into()),
            "socket error: fixture reset",
        ),
        (
            message(r#"{"type":"bye","reason":"fixture"}"#),
            "bye: fixture",
        ),
    ] {
        let (mut machine, mut tape) = admitted();
        tape.input(&mut machine, input);
        assert_eq!(reason(&mut machine), expected);
        assert_eq!(tape.operations, ["abandon"]);
    }
}

#[test]
fn connected_admission_ack_is_required_before_owners_or_page_queries_exist() {
    let mut machine = bare();
    let effect = machine.input(message(HELLO)).unwrap();
    let effect = skip_diagnostics(&mut machine, effect);
    assert!(matches!(
        publication(&mut machine, effect),
        Mt5Event::Status(Mt5Status::Connected { .. })
    ));
    assert!(machine.connected.is_none());
    assert!(machine.input(Input::Eof).is_err());
    assert!(machine.resume(Ack::Time(0)).is_err());
    assert!(matches!(
        machine.resume(Ack::Published(false)).unwrap(),
        Effect::Finished
    ));
    assert!(machine.connected.is_none());
    assert_eq!(reason(&mut machine), "consumer gone");
    assert!(machine.resume(Ack::Published(true)).is_err());
}

#[test]
fn anomaly_failure_changes_sequence_but_not_high_water_deals_or_mapping() {
    let (mut machine, mut tape) = admitted();
    tape.input(&mut machine, tick(1, 10000, 100, Some(7)));
    tape.events.clear();
    let before = machine.connected.as_ref().unwrap().mapper.stats;
    let effect = machine.input(tick(3, 12000, 101, Some(8))).unwrap();
    assert!(matches!(
        publication(&mut machine, effect),
        Mt5Event::SequenceAnomaly {
            from_ms: 8000,
            to_ms: 10000,
            ..
        }
    ));
    let state = machine.connected.as_ref().unwrap();
    assert_eq!(state.tracker.highest(), Some(3));
    assert_eq!(state.highest_tick_ms, Some(8000));
    assert_eq!(state.deals.stats.stamped, 1);
    assert_eq!(state.mapper.stats, before);
    let effect = machine.resume(Ack::Published(false)).unwrap();
    tape.drain(&mut machine, effect);
    let state = machine.connected.as_ref().unwrap();
    assert_eq!(state.highest_tick_ms, Some(8000));
    assert_eq!(state.mapper.stats, before);
    assert_eq!(state.deals.stats.stamped, 1);
    assert!(tape.events.is_empty());
    assert_eq!(reason(&mut machine), "consumer gone");
}

#[test]
fn deal_failure_and_success_pin_anomaly_then_deal_then_single_map() {
    for success in [false, true] {
        let (mut machine, mut tape) = admitted();
        tape.input(&mut machine, tick(1, 10000, 100, Some(7)));
        tape.events.clear();
        let before = machine.connected.as_ref().unwrap().mapper.stats;
        let effect = machine.input(tick(3, 12000, 101, Some(8))).unwrap();
        assert!(matches!(
            publication(&mut machine, effect),
            Mt5Event::SequenceAnomaly { .. }
        ));
        let effect = machine.resume(Ack::Published(true)).unwrap();
        assert!(
            matches!(publication(&mut machine, effect),Mt5Event::DealCounter(sample) if sample.session_deals==8&&sample.time_ms==10000)
        );
        let state = machine.connected.as_ref().unwrap();
        assert_eq!(state.highest_tick_ms, Some(10000));
        assert_eq!(state.mapper.stats, before);
        assert_eq!(state.deals.stats.stamped, 2);
        let effect = machine.resume(Ack::Published(success)).unwrap();
        if success {
            assert!(
                matches!(effect,Effect::Live(ref trade) if trade.agg_id==3&&trade.side==Side::Buy)
            );
            assert_eq!(machine.connected.as_ref().unwrap().mapper.stats.trades(), 1);
        } else {
            tape.drain(&mut machine, effect);
            assert_eq!(machine.connected.as_ref().unwrap().mapper.stats, before);
        }
    }
}

#[test]
fn sampling_is_only_every_64_live_prints_and_heartbeat_capture_follows_ack() {
    let (mut machine, mut tape) = admitted();
    for seq in 1..=65 {
        tape.input(
            &mut machine,
            tick(seq, 10000 + seq as i64, 100 + seq % 2, None),
        );
    }
    assert_eq!((tape.times, tape.captures), (1, 0));
    assert_eq!(
        tape.events
            .iter()
            .filter(|e| matches!(e, Mt5Event::Live(_)))
            .count(),
        64
    );
    tape.input(&mut machine, tick(66, 10066, 100, None));
    let effect=machine.input(message(r#"{"type":"heartbeat","seq_last":66,"time_ms":10067,"ticks_sent":66,"server_utc_offset_s":3}"#)).unwrap();
    assert!(matches!(effect, Effect::SampleTime));
    assert_eq!(
        machine
            .connected
            .as_ref()
            .unwrap()
            .mapper
            .server_utc_offset_ms(),
        3000
    );
    let effect = machine.resume(Ack::Time(11000)).unwrap();
    let effect = skip_diagnostics(&mut machine, effect);
    assert!(matches!(
        publication(&mut machine, effect),
        Mt5Event::Latency(_)
    ));
    assert!(
        machine
            .resume(Ack::Capture {
                enabled: false,
                base_generation: 0
            })
            .is_err()
    );
    let effect = machine.resume(Ack::Published(true)).unwrap();
    assert!(matches!(effect, Effect::ReadCapture));
    tape.drain(&mut machine, effect);
    assert_eq!(tape.captures, 1);
    tape.input(
        &mut machine,
        message(r#"{"type":"heartbeat","seq_last":66,"time_ms":10068,"ticks_sent":66}"#),
    );
    assert_eq!((tape.times, tape.captures), (2, 2)); // even an empty heartbeat samples the clock
}

#[test]
fn history_context_opening_and_missing_start_never_duplicate_debt() {
    let (mut machine, mut tape) = admitted();
    tape.input(&mut machine, tick(1, 10000, 100, None));
    tape.input(&mut machine, tick(2, 10001, 101, None));
    tape.input(&mut machine, message(r#"{"type":"backfill_start"}"#));
    tape.owed = true;
    tape.input(
        &mut machine,
        message(r#"{"type":"history_start","opening":true}"#),
    );
    tape.input(&mut machine, tick(3, 5000, 90, None));
    tape.input(&mut machine, tick(4, 5001, 91, None));
    tape.input(
        &mut machine,
        message(r#"{"type":"history_end","remaining":2}"#),
    );
    assert!(tape.owed);
    assert!(tape.operations.is_empty());
    assert!(
        matches!(tape.events.last(),Some(Mt5Event::OpeningPage{trades,remaining:Some(2)}) if trades.len()==1&&trades[0].agg_id==4)
    );
    tape.input(&mut machine, message(r#"{"type":"history_end"}"#));
    tape.input(&mut machine, message(r#"{"type":"history_end"}"#));
    assert_eq!(tape.operations, ["settle", "settle"]);
    assert_eq!(
        tape.events
            .iter()
            .filter(|e| matches!(e, Mt5Event::HistoryPage { .. }))
            .count(),
        1
    );
    tape.input(&mut machine, message(r#"{"type":"backfill_end"}"#));
    assert!(matches!(tape.events.last(),Some(Mt5Event::Backfilled(trades)) if trades.is_empty()));
    tape.input(&mut machine, tick(5, 12000, 100, None));
    assert!(matches!(tape.events.last(),Some(Mt5Event::Live(trade)) if trade.side==Side::Sell));
}

#[test]
fn depth_ack_boundaries_preserve_generation_and_map_only_after_connecting() {
    let (mut machine, mut tape) = admitted();
    let effect = machine.input(book(1)).unwrap();
    assert!(matches!(effect, Effect::ReadCapture));
    let effect = machine
        .resume(Ack::Capture {
            enabled: true,
            base_generation: 100,
        })
        .unwrap();
    assert!(matches!(
        publication(&mut machine, effect),
        Mt5Event::Depth(DepthEvent::Status {
            generation: 101,
            status: DepthStatus::Connecting,
            ..
        })
    ));
    assert_eq!(machine.generation_offset, 1);
    assert_eq!(
        machine
            .connected
            .as_ref()
            .unwrap()
            .depth
            .close()
            .0
            .unwrap()
            .images,
        0
    );
    let effect = machine.resume(Ack::Published(false)).unwrap();
    tape.drain(&mut machine, effect);
    assert_eq!(machine.generation_offset, 1);
    assert!(matches!(
        tape.events.last(),
        Some(Mt5Event::Depth(DepthEvent::Status {
            generation: 101,
            status: DepthStatus::Disconnected {
                error_class: "bridge_lost"
            },
            ..
        }))
    ));
    assert_eq!(
        machine
            .connected
            .as_ref()
            .unwrap()
            .depth
            .close()
            .0
            .unwrap()
            .images,
        0
    );
}

#[test]
fn failed_resync_does_not_increment_and_failed_snapshot_never_synchronizes() {
    let (mut machine, mut tape) = admitted();
    tape.capture = true;
    tape.base = 100;
    tape.input(&mut machine, book(1));
    tape.events.clear();
    let effect = machine.input(book(3)).unwrap();
    assert!(matches!(effect, Effect::ReadCapture));
    let effect = machine
        .resume(Ack::Capture {
            enabled: true,
            base_generation: 100,
        })
        .unwrap();
    assert!(matches!(
        publication(&mut machine, effect),
        Mt5Event::Depth(DepthEvent::Status {
            status: DepthStatus::Resyncing { .. },
            ..
        })
    ));
    let effect = machine.resume(Ack::Published(false)).unwrap();
    tape.drain(&mut machine, effect);
    assert_eq!(machine.generation_offset, 1);
    assert_eq!(
        machine
            .connected
            .as_ref()
            .unwrap()
            .depth
            .close()
            .0
            .unwrap()
            .images,
        1
    );

    let (mut machine, mut tape) = admitted();
    let effect = machine.input(book(1)).unwrap();
    assert!(matches!(effect, Effect::ReadCapture));
    let _ = machine
        .resume(Ack::Capture {
            enabled: true,
            base_generation: 0,
        })
        .unwrap();
    let _ = machine.take_publication().unwrap();
    let effect = machine.resume(Ack::Published(true)).unwrap();
    let effect = skip_diagnostics(&mut machine, effect);
    assert!(matches!(
        publication(&mut machine, effect),
        Mt5Event::Depth(DepthEvent::Snapshot { .. })
    ));
    let effect = machine.resume(Ack::Published(false)).unwrap();
    tape.drain(&mut machine, effect);
    assert!(!tape.events.iter().any(|e| matches!(
        e,
        Mt5Event::Depth(DepthEvent::Status {
            status: DepthStatus::Synchronized { .. },
            ..
        })
    )));
}

#[test]
fn every_cleanup_send_failure_keeps_original_end_and_continues_to_closed() {
    let (mut machine, mut tape) = admitted();
    tape.capture = true;
    tape.base = 100;
    tape.input(&mut machine, book(1));
    tape.input(&mut machine,message(r#"{"type":"tick","seq":1,"time_ms":10000,"sent_ms":10001,"bid":"0","ask":"0","last":"100","volume":1,"flags":56,"deals":7}"#));
    tape.input(&mut machine, message(r#"{"type":"backfill_start"}"#));
    tape.input(
        &mut machine,
        message(r#"{"type":"rates_start","interval_ms":60000}"#),
    );
    let effect = machine.input(Input::Eof).unwrap();
    let effect = skip_diagnostics(&mut machine, effect);
    assert!(matches!(effect, Effect::Pager(PagerOp::Abandon)));
    let effect = machine.resume(Ack::Pager(true)).unwrap();
    let effect = skip_diagnostics(&mut machine, effect);
    assert!(matches!(
        publication(&mut machine, effect),
        Mt5Event::HistoryPage { .. }
    ));
    let effect = machine.resume(Ack::Published(false)).unwrap();
    let effect = skip_diagnostics(&mut machine, effect);
    assert!(
        matches!(publication(&mut machine, effect),Mt5Event::DealCounter(sample) if sample.session_deals==7)
    );
    let effect = machine.resume(Ack::Published(false)).unwrap();
    let effect = skip_diagnostics(&mut machine, effect);
    assert!(matches!(
        publication(&mut machine, effect),
        Mt5Event::Depth(DepthEvent::Status {
            status: DepthStatus::Disconnected { .. },
            ..
        })
    ));
    assert!(matches!(
        machine.resume(Ack::Published(false)).unwrap(),
        Effect::Finished
    ));
    assert_eq!(reason(&mut machine), "eof");
    assert!(machine.input(Input::Eof).is_err());
    assert!(machine.resume(Ack::Published(false)).is_err());
}
