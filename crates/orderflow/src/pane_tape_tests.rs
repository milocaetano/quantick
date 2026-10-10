//! The pane's own tape: a clock it is told, and prints it holds until the
//! worker publishes them.
use crate::config::HeatmapConfig;
use crate::pane_tape::{PaneTape, PendingPrint};
use crate::tape_clock::TapeClock;
use quantick_engine::{Side, Trade};
use rust_decimal::Decimal;

fn print(agg_id: u64, timestamp_ms: i64) -> Trade {
    Trade {
        agg_id,
        timestamp_ms,
        price: Decimal::from(100),
        quantity: Decimal::from(2),
        side: Side::Buy,
    }
}

#[test]
fn only_a_native_tape_runs_a_clock_between_prints() {
    let mut tape = PaneTape::default();
    tape.follow_live(true, Some(1_000), 10_000);
    tape.follow_live(true, Some(1_000), 10_500);
    assert_eq!(tape.lane_now_ms(true), Some(1_500));
    assert_eq!(tape.lane_now_ms(false), None, "an ordinary lane reads none");

    tape.follow_live(false, Some(1_000), 11_000);
    assert_eq!(
        tape.lane_now_ms(true),
        None,
        "leaving the native tape drops the anchor"
    );
}

#[test]
fn a_replayed_tape_reads_the_clock_the_replay_places() {
    let mut tape = PaneTape::default();
    let mut clock = TapeClock::default();
    tape.follow_replay(true, 9_000, Some(1_000), Some(4_000));
    clock.replay_at(9_000, Some(1_000), Some(4_000));
    assert_eq!(tape.lane_now_ms(true), clock.now_ms());

    tape.follow_replay(false, 9_000, Some(1_000), None);
    assert_eq!(tape.lane_now_ms(true), None);
    tape.follow_live(true, Some(2_000), 5);
    tape.reset_clock();
    assert_eq!(tape.lane_now_ms(true), None);
}

#[test]
fn the_first_print_of_each_epoch_opens_it_for_the_worker() {
    let mut tape = PaneTape::default();
    let config = HeatmapConfig::default();
    let first = tape.record(&print(1, 1_000), &config);
    assert_eq!(first.opens, Some(tape.pending().epoch()));
    let second = tape.record(&print(2, 1_001), &config);
    assert_eq!(
        second,
        PendingPrint {
            opens: None,
            ordinal: first.ordinal + 1
        }
    );
    assert_eq!(tape.pending().len(), 2);

    let epoch = tape.reset();
    assert_eq!(epoch, tape.pending().epoch());
    assert!(tape.pending().is_empty());
    assert!(tape.frame().is_none());
    assert_eq!(tape.record(&print(1, 2_000), &config).opens, Some(epoch));
}

#[test]
fn an_epoch_keeps_the_sources_opening_facts_and_a_source_reset_drops_them() {
    let mut tape = PaneTape::default();
    tape.record(&print(1, 1_017), &HeatmapConfig::default());
    let recorded = tape.opening_bursts(None);
    tape.reset();
    assert_eq!(tape.opening_bursts(None), recorded);
    tape.pending_mut().clear_opening_bursts();
    assert!(tape.opening_bursts(None).is_empty());
}
