//! One tab's deal recorder, the REC control's owner, as
//! `feed.deal_recording.set` acts on it through
//! [`crate::recovery::FeedRecoveryPort::deal_recorder`].
use quantick_replay::recorder::{DealRecordingError, RecordingView};

/// One tab's deal recorder, as the REC control acts on it. Its names differ
/// from the tab's own methods, so an implementation that forwards to one
/// cannot fall back on itself.
pub trait DealRecorder {
    /// The tab plays a recorded session, not the live market.
    fn replaying(&self) -> bool;
    /// The market the tab shows.
    fn recorded_symbol(&self) -> &str;
    /// The recorder's view, as every chrome surface draws it; `None` on a
    /// feed with no counter and nothing loaded.
    fn recorder_view(&self) -> Option<RecordingView>;
    /// Load the recorded day at `index` of the view's days onto the chart.
    ///
    /// # Errors
    /// What the recorder refuses.
    fn load_recorded_day(&mut self, index: usize) -> Result<(), DealRecordingError>;
    /// Start recording, as the button does.
    ///
    /// # Errors
    /// What the recorder refuses.
    fn start_recording(&mut self) -> Result<(), DealRecordingError>;
    /// Stop recording, as the button does.
    fn stop_recording(&mut self);
}
