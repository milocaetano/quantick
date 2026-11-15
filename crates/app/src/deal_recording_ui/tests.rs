use super::*;
use std::path::PathBuf;

fn view(state: RecState) -> RecordingView {
    RecordingView {
        symbol: "WINV26".to_owned(),
        state,
        reading: Some(2_301_455),
        since_ms: Some(1_788_436_800_000),
        first_reading_ms: Some(1_788_436_800_000),
        counter_age_ms: Some(50),
        written: 10,
        path: None,
        dir: PathBuf::from("deals"),
        error: None,
        days: std::rc::Rc::from(Vec::new()),
        loaded_days: vec!["2026-09-03".to_owned()],
        tz_minutes: -180,
        default_on: false,
        counter_stale: false,
    }
}

/// A counter that stands still is reported with REC off too: the
/// trades pane is the one that suffers, and "counting" would be a lie.
#[test]
fn a_stuck_counter_is_named_with_rec_off() {
    let mut off = view(RecState::Off);
    off.loaded_days.clear();
    off.reading = Some(0);
    off.counter_age_ms = Some(100_000);
    off.counter_stale = true;
    let chip = chip_for(&off, BarKind::Trades, true, 0).unwrap();
    assert_eq!(chip.text, "no deal count · counter stuck at 0");
    off.reading = Some(2_301_455);
    let chip = chip_for(&off, BarKind::Trades, true, 0).unwrap();
    assert_eq!(chip.text, "counting · counter stale");
    off.counter_age_ms = Some(50);
    off.counter_stale = false;
    let chip = chip_for(&off, BarKind::Trades, true, 0).unwrap();
    assert_eq!(chip.text, "counting · not written to disk");
}

#[test]
fn the_chip_names_the_three_states_and_the_uncounted_prints() {
    let recording = chip_for(&view(RecState::Recording), BarKind::Trades, true, 0).unwrap();
    assert_eq!(recording.text, "recording · 09:00:00 →");
    let with_uncounted =
        chip_for(&view(RecState::Recording), BarKind::Trades, true, 1_204_118).unwrap();
    assert_eq!(
        with_uncounted.text,
        "recording · 09:00:00 → · 1 204 118 prints have no count"
    );
    let recorded = chip_for(&view(RecState::Recorded), BarKind::Tick, false, 0).unwrap();
    assert_eq!(recorded.text, "recorded · 2026-09-03");
    let counting = chip_for(&view(RecState::Off), BarKind::Trades, true, 0).unwrap();
    assert_eq!(counting.text, "counting · not written to disk");
    let mut no_reading = view(RecState::Off);
    no_reading.reading = None;
    let off_on_trades = chip_for(&no_reading, BarKind::Trades, false, 0).unwrap();
    assert_eq!(off_on_trades.text, "no deal count");
}

#[test]
fn a_tick_pane_on_a_feed_that_is_not_recording_shows_no_chip() {
    assert!(chip_for(&view(RecState::Off), BarKind::Tick, false, 0).is_none());
    let mut unsupported = view(RecState::Unsupported);
    unsupported.loaded_days.clear();
    assert!(chip_for(&unsupported, BarKind::Tick, false, 0).is_none());
    let blank = chip_for(&unsupported, BarKind::Trades, false, 0).unwrap();
    assert_eq!(
        blank.text,
        "no deal count · this source has no deal counter"
    );
}
