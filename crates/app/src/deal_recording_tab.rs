//! How a tab drives its deal recorder: the readings it feeds, the actions
//! the REC control asks for, and the views the chrome reads.
//!
//! A sibling of [`crate::tab`] holding an `impl Tab` block, so the tab's own
//! file grows by one field and three one-line calls. Everything here reads
//! and writes fields the tab already exposes.

use quantick_engine::DealSample;

use crate::deal_recording::{DealRecordingAction, RecordingView};
use crate::deal_recording_ui::{self, DealChip};
use crate::metrics;
use crate::state::BarKind;
use crate::tab::Tab;

impl Tab {
    /// One reading from the feed: written down if recording, retained by
    /// every pane either way, so a later switch to `trades` cuts from it.
    pub fn observe_deal_counter(&mut self, sample: DealSample) {
        self.deal_recorder.observe(sample, metrics::wall_clock_ms());
        for pane in self.panes_mut() {
            pane.state.observe_deals(sample);
        }
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
            for sample in samples {
                pane.state.observe_deals(*sample);
            }
            // The retained series changed under the bars: cut them again.
            pane.state.rebuild_bars();
        }
    }

    /// What the REC control asked for.
    pub fn apply_deal_recording(&mut self, action: DealRecordingAction) {
        match action {
            DealRecordingAction::Start => self.start_deal_recording(metrics::wall_clock_ms()),
            DealRecordingAction::Stop => self.deal_recorder.stop(),
            // The selector's own path: the change is picked up a frame later
            // by the same spec sync a toolbar click goes through.
            DealRecordingAction::ShowAsTrades => self.flow_pane.kind = BarKind::Trades,
            DealRecordingAction::OpenFolder => {
                crate::paper_trading::reveal_folder(&self.deal_recorder.view(0, None).dir);
            }
            DealRecordingAction::LoadDay(index) => self.load_recorded_day(index),
        }
    }

    /// The recorder as the chrome sees it now, or none on a feed with no
    /// counter and nothing loaded — where no REC control is drawn at all.
    #[must_use]
    pub fn deal_recording_view(&self) -> Option<RecordingView> {
        let view = self
            .deal_recorder
            .view(metrics::wall_clock_ms(), self.trade_arrival_ms());
        view.supported().then_some(view)
    }

    /// The status bar's recording cell.
    #[must_use]
    pub fn deal_status_cell(&self) -> Option<String> {
        self.deal_recording_view()
            .and_then(|view| view.status_cell())
    }

    /// The chart-corner chip for the flow pane, when it has something to say.
    #[must_use]
    pub fn deal_chip(&self) -> Option<DealChip> {
        let view = self.deal_recording_view()?;
        let pane = &self.flow_pane;
        deal_recording_ui::chip_for(
            &view,
            pane.kind,
            pane.state.deal_samples().last().is_some(),
            pane.state.uncounted_trades(),
        )
    }
}
