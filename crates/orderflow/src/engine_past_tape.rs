//! The native tape held at a past instant, rebuilt with every projection.
use std::sync::Arc;

use rust_decimal::Decimal;

use super::{BookEngine, ProjectionRequest, typical_bar_ms};
use crate::projection::{
    PastBars, PastTape, PriceWindow, SettledProjection, VolumeDots, project_past_tape,
};

/// What the view last asked of the engine beside its frame.
#[derive(Debug, Default)]
pub(super) struct ViewAsk {
    /// The newest request's price window; `None` until the first request or
    /// after a symbol reset, when the ladder falls back to each side's best.
    pub(super) price_window: Option<(Decimal, Decimal)>,
    /// Where the native tape's right edge is held; `None` follows live.
    pub(super) tape_end_ms: Option<i64>,
    /// The native tape at `tape_end_ms`, rebuilt with every projection.
    pub(super) past_tape: Option<Arc<PastTape>>,
}

impl BookEngine {
    /// Hold the native tape's right edge at `end_ms`, or follow live again
    /// with `None`. The UI tells the engine the instant; nothing here reads
    /// a clock, and the live frame never depends on it.
    pub fn set_tape_end(&mut self, end_ms: Option<i64>) {
        self.view.tape_end_ms = end_ms;
        if end_ms.is_none() {
            self.view.past_tape = None;
        }
    }

    /// The past tape for this request, beside — never inside — its frame.
    pub(super) fn project_past(
        &self,
        request: &ProjectionRequest,
        settled: &SettledProjection,
        dots: Option<&VolumeDots>,
        prices: PriceWindow,
        held: Option<Arc<PastTape>>,
    ) -> Option<Arc<PastTape>> {
        let end_ms = self
            .view
            .tape_end_ms
            .filter(|_| request.lane && self.config.native_tape())?;
        let dots = dots.filter(|dots| dots.native_tape)?;
        let reference_ms = typical_bar_ms(request);
        let window_ms = self.config.lane_window_ms(reference_ms).max(1);
        let latest_end = end_ms.min(self.history.latest_ms()?);
        let retained = self.history.tape_retained_from_ms();
        if let Some(reused) =
            held.and_then(|held| held.reused_at(latest_end, window_ms, dots, retained))
        {
            return Some(Arc::new(reused));
        }
        project_past_tape(
            &self.history,
            PastBars {
                first_bar_index: request.first_bar_index,
                closed: &request.closed,
                partial: request.partial.as_ref(),
            },
            end_ms,
            window_ms,
            reference_ms,
            prices,
            settled,
            dots,
        )
        .map(Arc::new)
    }
}
