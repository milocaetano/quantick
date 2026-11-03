//! The REC control's capability: start or stop recording a tab's deal
//! counter, from a script exactly as from the button.
//!
//! The reads live in [`super::feed`]'s `feed.status` scope, whose
//! `deal_recording` field is the same [`RecordingView`] every chrome surface
//! draws. This module is the act. It sits in the `feed` module beside
//! `feed.reconnect`, under the same cockpit permission: recording writes a
//! file the trader asked for and nothing the chart holds is touched, so it
//! is neither destructive nor risky, and it can be undone by the same call.

pub(crate) use quantick_control_schema::deal_recording::*;

use crate::app::{RecordingPort, TabsMutPort, TabsPort};
use quantick_control::{
    error::ControlError,
    registry::RegistryError,
    wire::{ActorContext, WireU64},
};

use serde_json::Value;

use crate::deal_recording::DealRecordingError;

use crate::deal_recording::{DealRecordingAction, RecState, RecordingView};

use super::{actions::ActionRegistry, gateway::ControlAccess};
use quantick_control_handlers::{TabDirectory, tab_index};

/// The view, on the wire.
pub(crate) fn snapshot(view: &RecordingView) -> DealRecordingSnapshot {
    DealRecordingSnapshot {
        state: match view.state {
            RecState::Unsupported => "unsupported",
            RecState::Off => "off",
            RecState::Recording => "recording",
            RecState::Stale => "stale",
            RecState::Recorded => "recorded",
        }
        .to_owned(),
        since_unix_ms: view.since_ms,
        first_reading_unix_ms: view.first_reading_ms,
        session_deals: view.reading.map(WireU64::new),
        file: view.path.as_ref().map(|path| path.display().to_string()),
        samples_written: WireU64::new(view.written),
        deal_count_available: view.deal_count_available(),
        recorded_days: view
            .days
            .iter()
            .map(|day| RecordedDaySnapshot {
                day: day.day.clone(),
                first_unix_ms: day.first.time_ms,
                last_unix_ms: day.last.time_ms,
                session_deals: WireU64::new(day.last.session_deals),
                samples: WireU64::new(day.samples),
                from_open: day.started_at_open(),
            })
            .collect(),
        loaded_days: view.loaded_days.clone(),
        error: view.error.clone(),
        record_by_default: view.default_on,
        counter_stale: view.counter_stale,
    }
}

pub(crate) fn register(registry: &mut ActionRegistry) -> Result<(), RegistryError> {
    registry.register(descriptor(), set)
}

fn set<P: TabDirectory + TabsPort + TabsMutPort + RecordingPort + ?Sized>(
    app: &mut P,
    _access: &mut ControlAccess,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: DealRecordingInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let index = tab_index(app, input.tab_id)?;
    let tab_id = app.tab_reads().tabs().id_at(index);
    let (tab, _config) = app
        .tabs_mut()
        .tab_with_config(index)
        .ok_or_else(|| ControlError::invalid_request("the tab closed while the call ran"))?;
    // A replay is another tape: the live market's recorder is not reachable
    // over it, and the answer says so rather than reading as "no counter".
    if tab.replay.is_some() {
        return Err(ControlError::invalid_request(format!(
            "tab {} is replaying; the live market's deal recorder is reachable again when the \
             replay closes",
            tab_id
        )));
    }
    // Resolve every data-dependent refusal before changing the durable
    // default. A call is atomic with respect to that setting: bad day, replay,
    // or unsupported start leaves the workspace untouched.
    let day_index = if let Some(day) = &input.load_day {
        let view = tab.deal_recording_view().ok_or_else(|| {
            ControlError::invalid_request(format!(
                "no recorded day '{day}' for {}; feed.status lists the recorded days",
                tab.symbol
            ))
        })?;
        Some(
            view.days
                .iter()
                .position(|recorded| recorded.day == *day)
                .ok_or_else(|| {
                    ControlError::invalid_request(format!(
                        "no recorded day '{day}' for {}; feed.status lists the recorded days",
                        tab.symbol
                    ))
                })?,
        )
    } else {
        None
    };
    let _ = tab;
    let (tab, _config) = app
        .tabs_mut()
        .tab_with_config(index)
        .ok_or_else(|| ControlError::invalid_request("the tab closed while the call ran"))?;
    if let Some(index) = day_index {
        tab.load_recorded_day_checked(index)
            .map_err(recording_error)?;
    }
    if let Some(enabled) = input.enabled {
        if enabled {
            tab.start_deal_recording_checked(crate::metrics::wall_clock_ms())
                .map_err(recording_error)?;
        } else {
            tab.apply_deal_recording(DealRecordingAction::Stop);
        }
    }
    let _ = tab;
    if let Some(on) = input.record_by_default {
        app.set_deal_recording_default(on);
    }
    let (tab, _config) = app
        .tabs_mut()
        .tab_with_config(index)
        .ok_or_else(|| ControlError::invalid_request("the tab closed while the call ran"))?;
    let result = DealRecordingResult {
        tab_id: WireU64::new(tab_id),
        symbol: tab.symbol.clone(),
        recording: tab.deal_recording_view().as_ref().map(snapshot),
    };
    serde_json::to_value(result).map_err(|error| {
        ControlError::invalid_request(format!(
            "the deal recording result could not be encoded: {error}"
        ))
    })
}

fn recording_error(error: DealRecordingError) -> ControlError {
    use quantick_control::{error::codes, id::ErrorCode};

    let retryable = matches!(error, DealRecordingError::Storage(_));
    ControlError::new(
        ErrorCode::new(codes::CAPABILITY_UNAVAILABLE)
            .expect("the control error code is registered"),
        error.to_string(),
        retryable,
    )
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use quantick_engine::DealSample;

    use super::*;
    use crate::deal_recording::RecordedDay;

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
}
