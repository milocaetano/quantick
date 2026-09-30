//! Same-frame tape facts; heavy book history remains on its existing worker.
use super::*;
use quantick_orderflow::engine::{ProjectionRequest, VisibleOrderflow};
use std::sync::Arc;

impl OrderflowView {
    pub(crate) fn recorded_opening_bursts(&self) -> Vec<i64> {
        self.pending_tape.opening_bursts(
            self.published
                .frame
                .as_ref()
                .map(|frame| frame.projection.as_ref()),
        )
    }

    pub(super) fn immediate_tape(&self) -> bool {
        self.config.native_tape() && self.config.volume_dots.enabled
    }

    pub(super) fn reset_pending_tape(&mut self) {
        self.tape_dots.get_mut().clear();
        self.tape_rebuilds.get_mut().discard();
        self.past_dots.get_mut().clear();
        self.set_tape_end(quantick_orderflow::tape_view::TapeEnd::Live);
        let epoch = self.pending_tape.reset();
        self.pending_frame = None;
        self.published.frame = None;
        self.worker.send(BookCommand::TapeEpoch(epoch));
    }

    pub(super) fn record_pending_trade(&mut self, trade: &Trade) {
        if !self.pending_tape.started() {
            self.worker
                .send(BookCommand::ApplyVisualConfig(self.config.clone()));
            self.worker
                .send(BookCommand::TapeEpoch(self.pending_tape.epoch()));
        }
        let ordinal = self.pending_tape.record(trade, &self.config);
        self.worker.send(BookCommand::TapeTrade {
            ordinal,
            trade: trade.clone(),
        });
    }

    pub(super) fn complete_pending_frame(
        &mut self,
        request: &ProjectionRequest,
    ) -> Option<Arc<VisibleOrderflow>> {
        if !self.immediate_tape() || self.pending_tape.is_empty() {
            self.pending_frame = None;
            return self.published.frame.clone();
        }
        let frame = Arc::new(VisibleOrderflow::with_pending_overlay(
            &self.pending_tape,
            &self.config,
            request,
            self.published.frame.as_deref(),
        )?);
        self.pending_frame = Some(Arc::clone(&frame));
        Some(frame)
    }
}
