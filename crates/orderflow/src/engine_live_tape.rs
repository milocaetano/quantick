//! The live half of a frame, continuing the last publication's sealed tape.
use super::BookEngine;
use crate::projection::{SealInputs, TapeReuse, project_live_after, seal_tape};
use crate::{BarTimeline, HeatmapProjection, PriceWindow, SettledProjection, VolumeDots};
use std::sync::Arc;

impl BookEngine {
    /// Build the moving half on `settled`, folding only the native tape after
    /// the last publication's seal, and seal the result for the next frame
    /// and for the painter. Sealed short of the correlation reach: a
    /// reduction still on its way may match a print that recent.
    pub(super) fn project_live_half(
        &self,
        timeline: &BarTimeline,
        prices: PriceWindow,
        settled: &SettledProjection,
        dots: Option<&VolumeDots>,
    ) -> HeatmapProjection {
        let previous = self.last_frame.clone();
        let previous = previous
            .as_ref()
            .and_then(|frame| frame.projection.tape_facts.as_deref());
        let live = project_live_after(
            &self.history,
            timeline,
            prices,
            settled,
            dots,
            previous.map(|previous| TapeReuse {
                previous,
                revision: self.config_revision,
            }),
        );
        let mut projection = settled.with_live(live, &self.config);
        if let (Some(facts), Some(dots)) =
            (projection.tape_facts.as_mut().and_then(Arc::get_mut), dots)
        {
            let reach = self.config.liquidity_correlation_ms.max(0);
            let inputs = SealInputs {
                revision: self.config_revision,
                window_ms: dots.tape_window_ms,
                seal_from_ms: self
                    .history
                    .latest_ms()
                    .map(|latest| latest.saturating_sub(reach)),
                lane_from_ms: timeline.lane_start_ms(),
                recorded: self.history.counters().aggressions_recorded,
            };
            seal_tape(facts, previous, inputs);
        }
        projection
    }
}
