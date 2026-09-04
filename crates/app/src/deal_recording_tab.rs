//! How a tab drives its deal recorder: the readings it feeds, the actions
//! the REC control asks for, and the views the chrome reads.
//!
//! A sibling of [`crate::tab`] holding an `impl Tab` block, so the tab's own
//! file grows by one field and three one-line calls. Everything here reads
//! and writes fields the tab already exposes.

use quantick_engine::DealSample;

use crate::deal_recording::{DealRecordingAction, RecState, RecordingView};
use crate::deal_recording_ui::{self, DealChip};
use crate::metrics;
use crate::state::BarKind;
use crate::tab::Tab;

impl Tab {
    /// One reading from the feed: written down if recording, retained by
    /// every pane either way, so a later switch to `trades` cuts from it.
    pub fn observe_deal_counter(&mut self, sample: DealSample) {
        let resumed = self.deal_recorder.observe(sample, metrics::wall_clock_ms());
        for pane in self.panes_mut() {
            pane.state.observe_deals(sample);
        }
        // A rotation into a day whose file exists resumes it, as a start
        // does: what the file holds reaches the panes.
        self.retain_deal_samples(&resumed);
    }

    /// Per drain: learn what the feed can count, honour the default once,
    /// and reach the disk on schedule.
    pub fn tick_deal_recording(&mut self) {
        let available = self.feed_capabilities.borrow().deal_counter;
        self.deal_recorder.set_available(available);
        let now_ms = metrics::wall_clock_ms();
        if self.deal_recorder.auto_start_due() {
            self.start_deal_recording(now_ms);
        }
        self.deal_recorder.flush_if_due(now_ms);
    }

    /// Start recording, resuming today's file when there is one: the
    /// readings it held reach every pane, so the day's earlier prints cut as
    /// trades bars again after a restart.
    pub fn start_deal_recording(&mut self, now_ms: i64) {
        let resumed = self.deal_recorder.start(now_ms);
        self.retain_deal_samples(&resumed);
    }

    /// Before a replay replaces the tape: the live market's readings are put
    /// aside, to come back with it. A replay is another day's prints, and
    /// the reset that clears the panes for it drops the readings with the
    /// series.
    pub fn stash_deal_readings(&mut self) {
        let readings = self.flow_pane.state.deal_samples().to_vec();
        self.deal_recorder.stash(readings);
    }

    /// After the replay closed and the live market is back: its readings
    /// return to every pane, ahead of the backfill about to arrive.
    pub fn restore_deal_readings(&mut self) {
        let readings = self.deal_recorder.take_stash();
        self.retain_deal_samples(&readings);
    }

    /// Load a recorded day's readings into every pane.
    pub fn load_recorded_day(&mut self, index: usize) {
        let loaded = self.deal_recorder.load_day(index);
        self.retain_deal_samples(&loaded);
    }

    fn retain_deal_samples(&mut self, samples: &[DealSample]) {
        if samples.is_empty() {
            return;
        }
        for pane in self.panes_mut() {
            pane.state.observe_deals_batch(samples);
            // The retained series changed under the bars. Only a pane cutting
            // by deals reads it now; the others replay it on their next
            // switch, and a rebuild over a day of prints is not free.
            if pane.kind == BarKind::Trades {
                pane.rebuild_bars();
            }
        }
    }

    /// What the REC control asked for.
    pub fn apply_deal_recording(&mut self, action: DealRecordingAction) {
        match action {
            DealRecordingAction::Start => self.start_deal_recording(metrics::wall_clock_ms()),
            DealRecordingAction::Stop => self.deal_recorder.stop(),
            // The selector's own path, on the pane the popover described —
            // the focused one, like every bar control; the change is picked
            // up a frame later by the same spec sync a toolbar click goes
            // through.
            DealRecordingAction::ShowAsTrades => self.focused_pane_mut().kind = BarKind::Trades,
            DealRecordingAction::OpenFolder => {
                crate::paper_trading::reveal_folder(&self.deal_recorder.view(None).dir);
            }
            DealRecordingAction::LoadDay(index) => self.load_recorded_day(index),
        }
    }

    /// The recorder as the chrome sees it now, or none on a feed with no
    /// counter and nothing loaded — where no REC control is drawn at all.
    #[must_use]
    pub fn deal_recording_view(&self) -> Option<RecordingView> {
        if self.replay.is_some() {
            // A replay is another tape and declares no counter: the live
            // market's recorder, and its recorded days, are not for it.
            return None;
        }
        // Judged on the tape's clock: the newest print the tab holds.
        let view = self.deal_recorder.view(self.latest_trade_ms);
        view.supported().then_some(view)
    }

    /// The status bar's recording cell.
    #[must_use]
    pub fn deal_status_cell(&self) -> Option<crate::statusbar::DealCell> {
        let view = self.deal_recording_view()?;
        Some(crate::statusbar::DealCell {
            text: view.status_cell()?,
            tone: view.state,
        })
    }

    /// The chart-corner chip for the flow pane, when it has something to say.
    ///
    /// Straight from the recorder, not through [`Self::deal_recording_view`]:
    /// the one chip a feed with no counter owes — a `trades` pane with
    /// nothing to cut on — is exactly the case that view withholds.
    #[must_use]
    pub fn deal_chip(&self) -> Option<DealChip> {
        let mut view = self.deal_recorder.view(self.latest_trade_ms);
        if self.replay.is_some() {
            // A replay carries no counter and is not this recorder's tape:
            // the one chip it can earn is the no-count one, on a trades pane
            // — never "recording" over another day's prints.
            view.state = RecState::Unsupported;
            view.loaded_days.clear();
        }
        let pane = &self.flow_pane;
        deal_recording_ui::chip_for(
            &view,
            pane.kind,
            pane.state.deal_samples().last().is_some(),
            pane.state.uncounted_trades(),
        )
    }
}
