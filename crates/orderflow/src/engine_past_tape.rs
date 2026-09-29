//! The native tape held at a past instant, rebuilt with every projection.
use std::sync::Arc;

use super::{BookEngine, ProjectionRequest, typical_bar_ms};
use crate::projection::{
    PastBars, PastTape, PriceWindow, SettledProjection, VolumeDots, project_past_tape,
};

impl BookEngine {
    /// Hold the native tape's right edge at `end_ms`, or follow live again
    /// with `None`. The UI tells the engine the instant; nothing here reads
    /// a clock, and the live frame never depends on it.
    pub fn set_tape_end(&mut self, end_ms: Option<i64>) {
        self.tape_end_ms = end_ms;
        if end_ms.is_none() {
            self.past_tape = None;
        }
    }

    /// The past tape for this request, beside — never inside — its frame.
    pub(super) fn project_past(
        &self,
        request: &ProjectionRequest,
        settled: &SettledProjection,
        dots: Option<&VolumeDots>,
        prices: PriceWindow,
    ) -> Option<Arc<PastTape>> {
        let end_ms = self
            .tape_end_ms
            .filter(|_| request.lane && self.config.native_tape())?;
        let dots = dots.filter(|dots| dots.native_tape)?;
        let reference_ms = typical_bar_ms(request);
        project_past_tape(
            &self.history,
            PastBars {
                first_bar_index: request.first_bar_index,
                closed: &request.closed,
                partial: request.partial.as_ref(),
            },
            end_ms,
            self.config.lane_window_ms(reference_ms),
            reference_ms,
            prices,
            settled,
            dots,
        )
        .map(Arc::new)
    }
}
