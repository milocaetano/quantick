//! The REC control on the wire: the recorder's view as the control plane
//! reports it, and the refusals its act answers with.
//!
//! The view is the same [`RecordingView`] every chrome surface draws, so the
//! words here are the chrome's words. The act itself, which reaches a tab's
//! recorder, stays with the window; what it says back is decided here.
//!
//! Rate class: a human or an agent asking about, or flipping, the recorder.
//! Never per trade.

use quantick_control::{
    error::{ControlError, codes},
    id::ErrorCode,
    wire::WireU64,
};
use quantick_control_schema::deal_recording::{DealRecordingSnapshot, RecordedDaySnapshot};
use quantick_replay::recorder::{DealRecordingError, RecState, RecordingView};

/// The view, on the wire.
pub fn snapshot(view: &RecordingView) -> DealRecordingSnapshot {
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

/// A replay is another tape: the live market's recorder is not reachable
/// over it, and the answer says so rather than reading as "no counter".
pub fn replaying(tab_id: u64) -> ControlError {
    ControlError::invalid_request(format!(
        "tab {} is replaying; the live market's deal recorder is reachable again when the \
         replay closes",
        tab_id
    ))
}

/// Which of the recorder's days `day` names, by index into
/// [`RecordingView::days`]; `view` is `None` on a feed with no counter and
/// nothing loaded. Either way an unknown day is refused before anything
/// changes, so the call stays atomic.
pub fn recorded_day(
    view: Option<&RecordingView>,
    day: &str,
    symbol: &str,
) -> Result<usize, ControlError> {
    let unknown = || {
        ControlError::invalid_request(format!(
            "no recorded day '{day}' for {symbol}; feed.status lists the recorded days"
        ))
    };
    view.ok_or_else(unknown)?
        .days
        .iter()
        .position(|recorded| recorded.day == day)
        .ok_or_else(unknown)
}

/// A recorder refusal, on the wire. Only storage can succeed on a retry.
pub fn recording_error(error: DealRecordingError) -> ControlError {
    let retryable = matches!(error, DealRecordingError::Storage(_));
    ControlError::new(
        ErrorCode::new(codes::CAPABILITY_UNAVAILABLE)
            .expect("the control error code is registered"),
        error.to_string(),
        retryable,
    )
}

#[cfg(test)]
#[path = "deal_recording_tests.rs"]
mod deal_recording_tests;
