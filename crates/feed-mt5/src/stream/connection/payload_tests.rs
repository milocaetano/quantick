//! Payload moves and acknowledgements are distinct, ordered protocol boundaries.
use super::super::diagnostic::Diagnostic;
use super::*;

fn reject_publication_acks(machine: &mut SessionMachine<'_>) {
    let phase = machine.phase;
    let generation = machine.generation_offset;
    for success in [true, false] {
        assert!(machine.resume(Ack::Published(success)).is_err());
        assert_eq!(machine.phase, phase);
        assert_eq!(machine.generation_offset, generation);
    }
}

#[test]
fn publication_wait_continuation_requires_take_before_success_or_failure_ack() {
    for success in [true, false] {
        let (mut machine, mut tape) = admitted();
        tape.input(&mut machine, tick(1, 10000, 100, None));
        let stats = machine.connected.as_ref().unwrap().mapper.stats;
        let effect = machine
            .input(message(r#"{"type":"backfill_end"}"#))
            .unwrap();
        assert!(matches!(effect, Effect::Diagnostic));
        assert!(machine.take_publication().is_err());
        assert!(machine.resume(Ack::Diagnostic).is_err());
        reject_publication_acks(&mut machine);
        assert!(matches!(
            machine.take_diagnostic().unwrap(),
            Diagnostic::BackfillEnd(0)
        ));
        assert!(machine.take_diagnostic().is_err());
        assert!(machine.take_publication().is_err());
        reject_publication_acks(&mut machine);
        assert!(matches!(
            machine.resume(Ack::Diagnostic).unwrap(),
            Effect::Publish
        ));

        assert!(machine.take_diagnostic().is_err());
        assert!(machine.input(Input::Eof).is_err());
        reject_publication_acks(&mut machine);
        assert_eq!(machine.connected.as_ref().unwrap().mapper.stats, stats);
        assert!(
            matches!(machine.take_publication().unwrap(), Mt5Event::Backfilled(trades) if trades.is_empty())
        );
        assert!(machine.take_publication().is_err());
        assert!(machine.resume(Ack::Diagnostic).is_err());
        assert_eq!(machine.phase, Phase::Connected);
        let effect = machine.resume(Ack::Published(success)).unwrap();
        if success {
            assert!(matches!(effect, Effect::Wait(Phase::Connected)));
            assert!(machine.resume(Ack::Published(true)).is_err());
        } else {
            tape.drain(&mut machine, effect);
            assert_eq!(reason(&mut machine), "consumer gone");
        }
        assert_eq!(machine.connected.as_ref().unwrap().mapper.stats, stats);
    }
}

#[test]
fn admission_diagnostics_preserve_publish_then_wait_and_take_never_connects() {
    let mut machine = bare();
    assert!(matches!(
        machine.input(message(HELLO)).unwrap(),
        Effect::Diagnostic
    ));
    reject_publication_acks(&mut machine);
    assert!(machine.take_publication().is_err());
    assert!(machine.resume(Ack::Diagnostic).is_err());
    assert!(
        matches!(machine.take_diagnostic().unwrap(), Diagnostic::Hello(hello) if hello.symbol=="TEST")
    );
    assert!(machine.take_diagnostic().is_err());
    assert!(machine.take_publication().is_err());
    reject_publication_acks(&mut machine);
    assert!(matches!(
        machine.resume(Ack::Diagnostic).unwrap(),
        Effect::Publish
    ));
    reject_publication_acks(&mut machine);
    assert!(machine.connected.is_none());
    assert!(
        matches!(machine.take_publication().unwrap(), Mt5Event::Status(Mt5Status::Connected {symbol,broker_symbol,..}) if symbol=="TEST"&&broker_symbol=="TEST.raw")
    );
    assert!(
        machine.connected.is_none(),
        "taking the event does not confirm its send"
    );
    assert!(machine.take_publication().is_err());
    assert!(machine.resume(Ack::Time(0)).is_err());
    assert!(matches!(
        machine.resume(Ack::Published(true)).unwrap(),
        Effect::Diagnostic
    ));
    assert!(machine.connected.is_some());
    assert!(machine.resume(Ack::Diagnostic).is_err());
    assert!(machine.take_publication().is_err());
    assert!(matches!(
        machine.take_diagnostic().unwrap(),
        Diagnostic::Capabilities {
            paging: true,
            depth: true,
            ..
        }
    ));
    assert!(machine.take_diagnostic().is_err());
    reject_publication_acks(&mut machine);
    assert!(matches!(
        machine.resume(Ack::Diagnostic).unwrap(),
        Effect::Wait(Phase::Connected)
    ));
    assert!(machine.take_diagnostic().is_err());
    assert!(machine.resume(Ack::Diagnostic).is_err());
}

#[test]
fn diagnostics_before_finished_reject_invalid_operations_without_losing_the_reason() {
    let mut machine = bare();
    assert!(matches!(
        machine.input(Input::Timeout).unwrap(),
        Effect::Diagnostic
    ));
    assert_eq!(machine.phase, Phase::Closed);
    assert!(machine.take_publication().is_err());
    assert!(machine.resume(Ack::Diagnostic).is_err());
    reject_publication_acks(&mut machine);
    assert!(machine.input(Input::Eof).is_err());
    assert!(matches!(
        machine.take_diagnostic().unwrap(),
        Diagnostic::Timeout {
            hello: true,
            seconds: 10
        }
    ));
    assert!(machine.take_diagnostic().is_err());
    assert!(machine.take_publication().is_err());
    reject_publication_acks(&mut machine);
    assert!(matches!(
        machine.resume(Ack::Diagnostic).unwrap(),
        Effect::Finished
    ));
    assert_eq!(reason(&mut machine), "hello timeout");
    assert!(machine.resume(Ack::Diagnostic).is_err());
}

#[test]
fn live_ack_needs_no_publication_take_and_a_wrong_take_preserves_the_trade() {
    let (mut machine, mut tape) = admitted();
    tape.input(&mut machine, tick(1, 10000, 100, None));
    let effect = machine.input(tick(2, 10001, 101, None)).unwrap();
    assert!(matches!(effect, Effect::Live(ref trade) if trade.agg_id==2));
    assert!(machine.take_publication().is_err());
    assert!(machine.take_diagnostic().is_err());
    assert!(machine.resume(Ack::Diagnostic).is_err());
    assert!(matches!(
        machine.resume(Ack::Published(true)).unwrap(),
        Effect::Wait(Phase::Connected)
    ));
    assert_eq!(machine.connected.as_ref().unwrap().mapper.stats.trades(), 1);
}

#[test]
fn cleanup_diagnostics_remain_ordered_around_failed_publications() {
    let (mut machine, mut tape) = admitted();
    tape.capture = true;
    tape.base = 100;
    tape.input(&mut machine, book(1));
    tape.input(&mut machine, message(r#"{"type":"backfill_start"}"#));
    tape.input(
        &mut machine,
        message(r#"{"type":"rates_start","interval_ms":60000}"#),
    );
    assert!(matches!(
        machine
            .input(message(r#"{"type":"bye","reason":"ordered_cleanup"}"#))
            .unwrap(),
        Effect::Diagnostic
    ));
    assert!(
        matches!(machine.take_diagnostic().unwrap(), Diagnostic::Bye(reason) if reason=="ordered_cleanup")
    );
    assert!(matches!(
        machine.resume(Ack::Diagnostic).unwrap(),
        Effect::Diagnostic
    ));
    assert!(matches!(
        machine.take_diagnostic().unwrap(),
        Diagnostic::BackfillDiscard
    ));
    assert!(matches!(
        machine.resume(Ack::Diagnostic).unwrap(),
        Effect::Pager(PagerOp::Abandon)
    ));
    assert!(matches!(
        machine.resume(Ack::Pager(true)).unwrap(),
        Effect::Diagnostic
    ));
    assert!(matches!(
        machine.take_diagnostic().unwrap(),
        Diagnostic::HistoryUnanswered(0)
    ));
    assert!(matches!(
        machine.resume(Ack::Diagnostic).unwrap(),
        Effect::Publish
    ));
    reject_publication_acks(&mut machine);
    assert!(
        matches!(machine.take_publication().unwrap(), Mt5Event::HistoryPage {trades,..} if trades.is_empty())
    );
    assert!(matches!(
        machine.resume(Ack::Published(false)).unwrap(),
        Effect::Diagnostic
    ));

    // The depth-close publication is parked behind all three cleanup facts.
    for expected in ["rates", "mapper", "book"] {
        assert!(machine.take_publication().is_err());
        reject_publication_acks(&mut machine);
        let fact = machine.take_diagnostic().unwrap();
        assert!(match (expected, fact) {
            ("rates", Diagnostic::RatesDiscard(0)) => true,
            ("mapper", Diagnostic::MapSummary(_)) => true,
            ("book", Diagnostic::BookSummary(stats)) => stats.images == 1,
            _ => false,
        });
        assert!(machine.take_diagnostic().is_err());
        assert!(machine.take_publication().is_err());
        assert!(machine.resume(Ack::Time(0)).is_err());
        let effect = machine.resume(Ack::Diagnostic).unwrap();
        assert!(match expected {
            "book" => matches!(effect, Effect::Publish),
            _ => matches!(effect, Effect::Diagnostic),
        });
    }
    assert!(matches!(
        machine.take_publication().unwrap(),
        Mt5Event::Depth(DepthEvent::Status {
            generation: 101,
            status: DepthStatus::Disconnected {
                error_class: "bridge_lost"
            },
            ..
        })
    ));
    assert!(matches!(
        machine.resume(Ack::Published(false)).unwrap(),
        Effect::Finished
    ));
    assert_eq!(machine.generation_offset, 1);
    assert_eq!(reason(&mut machine), "bye: ordered_cleanup");
}
