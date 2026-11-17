//! The REC control's capability: start or stop recording a tab's deal
//! counter, from a script exactly as from the button.
//!
//! The reads live in [`super::feed`]'s `feed.status` scope, whose
//! `deal_recording` field is the same recorder view every chrome surface
//! draws. This module is the act. It sits in the `feed` module beside
//! `feed.reconnect`, under the same cockpit permission: recording writes a
//! file the trader asked for and nothing the chart holds is touched, so it
//! is neither destructive nor risky, and it can be undone by the same call.
//! The call, the view on the wire and every refusal are
//! [`quantick_control_handlers::deal_recording`]'s; here is the tab's
//! recorder as that call acts on it.

use quantick_control_handlers::deal_recorder::DealRecorder;
pub(crate) use quantick_control_handlers::deal_recording::snapshot;
use quantick_replay::recorder::{DealRecordingError, RecordingView};

use crate::deal_recording::DealRecordingAction;
use crate::tab::Tab;

impl DealRecorder for Tab {
    fn replaying(&self) -> bool {
        self.link.replay.is_some()
    }
    fn recorded_symbol(&self) -> &str {
        &self.symbol
    }
    fn recorder_view(&self) -> Option<RecordingView> {
        self.deal_recording_view()
    }
    fn load_recorded_day(&mut self, index: usize) -> Result<(), DealRecordingError> {
        let loaded = self.deal_recorder.load_day_checked(index)?;
        self.retain_deal_samples(&loaded);
        Ok(())
    }
    fn start_recording(&mut self) -> Result<(), DealRecordingError> {
        let now = crate::metrics::wall_clock_ms();
        let resumed = self.deal_recorder.start_checked(now)?;
        self.retain_deal_samples(&resumed);
        Ok(())
    }
    fn stop_recording(&mut self) {
        self.apply_deal_recording(DealRecordingAction::Stop);
    }
}
