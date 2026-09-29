//! Tape time advances only from caller-supplied clocks and applied market data.
use crate::tape_clock::TapeClock;

#[test]
fn live_tape_moves_between_prints_on_the_supplied_monotonic_clock() {
    let mut clock = TapeClock::default();
    assert_eq!(clock.live_at(None, 10_000), None);
    assert_eq!(clock.live_at(Some(1_000), 10_000), Some(1_000));
    assert_eq!(clock.live_at(Some(1_000), 10_016), Some(1_016));
    assert_eq!(clock.live_at(Some(1_000), 11_200), Some(2_200));
}

#[test]
fn a_late_live_print_never_rewinds_the_tape_edge() {
    let mut clock = TapeClock::default();
    assert_eq!(clock.live_at(Some(1_000), 10_000), Some(1_000));
    assert_eq!(clock.live_at(Some(1_000), 10_500), Some(1_500));
    assert_eq!(clock.live_at(Some(1_100), 10_600), Some(1_600));
    assert_eq!(clock.live_at(Some(1_100), 10_616), Some(1_616));
    assert_eq!(clock.live_at(Some(900), 10_632), Some(1_632));
}

#[test]
fn source_reset_cannot_carry_the_previous_markets_clock() {
    let mut clock = TapeClock::default();
    assert_eq!(clock.live_at(Some(90_000), 100), Some(90_000));
    clock.reset();
    assert_eq!(clock.live_at(None, 1_000), None);
    assert_eq!(clock.live_at(Some(500), 1_010), Some(500));
}

#[test]
fn replay_pause_speed_and_seek_follow_only_the_supplied_playhead() {
    let mut clock = TapeClock::default();
    assert_eq!(
        clock.replay_at(1_000, Some(1_000), Some(10_000)),
        Some(1_000)
    );
    assert_eq!(
        clock.replay_at(1_100, Some(1_000), Some(10_000)),
        Some(1_100)
    );
    assert_eq!(
        clock.replay_at(1_100, Some(1_000), Some(10_000)),
        Some(1_100)
    );
    // A faster playhead already contains the speed multiplier; never apply it twice.
    assert_eq!(
        clock.replay_at(1_600, Some(1_000), Some(10_000)),
        Some(1_600)
    );
    assert_eq!(clock.replay_at(700, Some(500), Some(1_000)), Some(700));
    assert_eq!(clock.live_at(Some(2_000), 80_000), Some(2_000));
}

#[test]
fn replay_never_runs_past_the_first_print_not_applied_to_the_chart() {
    let mut clock = TapeClock::default();
    assert_eq!(clock.replay_at(9_000, None, Some(1_000)), None);
    assert_eq!(
        clock.replay_at(9_000, Some(1_000), Some(2_000)),
        Some(2_000)
    );
    assert_eq!(
        clock.replay_at(9_000, Some(2_000), Some(2_000)),
        Some(2_000)
    );
    assert_eq!(
        clock.replay_at(9_000, Some(2_000), Some(3_000)),
        Some(3_000)
    );
    assert_eq!(clock.replay_at(9_000, Some(3_000), None), Some(9_000));
    // A worker sample can briefly lag a batch already consumed by the chart.
    assert_eq!(clock.replay_at(2_500, Some(3_000), None), Some(3_000));
}

#[test]
fn a_bad_monotonic_sample_and_integer_limit_do_not_reverse_or_overflow() {
    let mut clock = TapeClock::default();
    assert_eq!(clock.live_at(Some(1_000), 100), Some(1_000));
    assert_eq!(clock.live_at(Some(1_000), 116), Some(1_016));
    assert_eq!(clock.live_at(Some(1_000), 110), Some(1_016));
    clock.reset();
    assert_eq!(clock.live_at(Some(i64::MAX - 5), 0), Some(i64::MAX - 5));
    assert_eq!(clock.live_at(Some(i64::MAX - 5), u64::MAX), Some(i64::MAX));
}
