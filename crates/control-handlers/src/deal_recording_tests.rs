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

/// One tab (id 1) on a counter whose recorder holds one day.
fn window() -> crate::test_support::FakeWindow {
    let mut tab = crate::test_support::FakeTab::new(1, "WINV26");
    tab.recording = Some(view(RecState::Off));
    crate::test_support::FakeWindow::new(vec![tab])
}

fn call(
    window: &mut crate::test_support::FakeWindow,
    input: serde_json::Value,
) -> Result<serde_json::Value, ControlError> {
    set(
        window,
        &mut (),
        &crate::test_support::FakeWindow::assistant(),
        &input,
    )
}

#[test]
fn the_call_loads_starts_and_keeps_the_default_on_the_tabs_recorder() {
    let mut window = window();
    let input = serde_json::json!({"tab_id": "1", "load_day": "2026-09-02",
        "enabled": true, "record_by_default": true});
    let answer = call(&mut window, input).unwrap();
    assert_eq!(answer["symbol"], "WINV26");
    assert_eq!(answer["recording"]["state"], "off");
    assert_eq!(window.tabs[0].recorder_acts, ["load 0", "start"]);
    assert_eq!(window.record_by_default, Some(true));
    call(&mut window, serde_json::json!({"enabled": false})).unwrap();
    assert_eq!(
        window.tabs[0].recorder_acts.last().map(String::as_str),
        Some("stop")
    );
}

#[test]
fn a_refusal_leaves_the_recorder_and_the_default_untouched() {
    let mut window = window();
    let unknown = serde_json::json!({"load_day": "2026-01-01", "record_by_default": true});
    let error = call(&mut window, unknown).unwrap_err();
    assert!(
        format!("{error:?}").contains("no recorded day '2026-01-01' for WINV26"),
        "{error:?}"
    );
    window.tabs[0].recorder_refusal = Some(DealRecordingError::CounterUnavailable);
    let start = serde_json::json!({"enabled": true, "record_by_default": true});
    let error = call(&mut window, start).unwrap_err();
    assert_eq!(error.code.as_str(), codes::CAPABILITY_UNAVAILABLE);
    window.tabs[0].replaying = true;
    let error = call(&mut window, serde_json::json!({"record_by_default": true})).unwrap_err();
    assert!(
        format!("{error:?}").contains("tab 1 is replaying"),
        "{error:?}"
    );
    assert!(window.tabs[0].recorder_acts.is_empty());
    assert_eq!(window.record_by_default, None);
}
