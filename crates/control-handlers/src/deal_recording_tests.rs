use std::path::PathBuf;

use quantick_engine::DealSample;
use quantick_replay::recorder::RecordedDay;

use super::*;

fn view(state: RecState) -> RecordingView {
    RecordingView {
        symbol: "WINV26".to_owned(),
        state,
        reading: Some(2_301_455),
        since_ms: Some(1_788_436_800_000),
        first_reading_ms: Some(1_788_436_800_000),
        counter_age_ms: Some(20),
        written: 12,
        path: Some(PathBuf::from("deals/WINV26/2026-09-03.deals")),
        dir: PathBuf::from("deals"),
        error: None,
        days: std::rc::Rc::from(vec![RecordedDay {
            day: "2026-09-02".to_owned(),
            first: DealSample {
                time_ms: 1,
                session_deals: 12,
            },
            last: DealSample {
                time_ms: 2,
                session_deals: 5_000_000,
            },
            samples: 40_000,
            path: PathBuf::from("deals/WINV26/2026-09-02.deals"),
        }]),
        loaded_days: vec!["2026-09-02".to_owned()],
        tz_minutes: -180,
        default_on: false,
        counter_stale: false,
    }
}

#[test]
fn the_wire_says_the_same_words_as_the_chrome() {
    for (state, word) in [
        (RecState::Off, "off"),
        (RecState::Recording, "recording"),
        (RecState::Stale, "stale"),
        (RecState::Recorded, "recorded"),
        (RecState::Unsupported, "unsupported"),
    ] {
        assert_eq!(snapshot(&view(state)).state, word);
    }
    let wire = snapshot(&view(RecState::Recording));
    assert_eq!(wire.session_deals, Some(WireU64::new(2_301_455)));
    assert_eq!(wire.since_unix_ms, Some(1_788_436_800_000));
    assert_eq!(wire.samples_written, WireU64::new(12));
    assert!(wire.deal_count_available);
    assert_eq!(wire.recorded_days.len(), 1);
    assert!(
        wire.recorded_days[0].from_open,
        "the counter read 12 at the start"
    );
    assert_eq!(wire.loaded_days, ["2026-09-02"]);
    let json = serde_json::to_value(&wire).unwrap();
    assert_eq!(json["file"], "deals/WINV26/2026-09-03.deals");
    assert!(
        json.get("error").is_none(),
        "an absent error is not written"
    );
}

#[test]
fn a_recorded_day_is_found_by_name_and_an_unknown_one_is_refused() {
    let view = view(RecState::Off);
    assert_eq!(
        recorded_day(Some(&view), "2026-09-02", "WINV26").unwrap(),
        0
    );
    for refused in [
        recorded_day(Some(&view), "2026-09-01", "WINV26"),
        recorded_day(None, "2026-09-02", "WINV26"),
    ] {
        assert!(
            refused
                .unwrap_err()
                .message
                .contains("no recorded day '2026-09")
        );
    }
}
