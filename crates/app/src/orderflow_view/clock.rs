//! The pane's supplied market clock, separate from the worker's event cursor.
use super::OrderflowView;

impl OrderflowView {
    /// Only the independent tape advances between market events. Ordinary
    /// lanes retain their existing event-anchored clock, including BTC.
    pub(crate) fn lane_now_ms(&self) -> Option<i64> {
        if self.config.native_tape() {
            self.tape_clock.now_ms()
        } else {
            None
        }
    }

    pub(crate) fn set_live_clock_at(&mut self, applied_ms: Option<i64>, monotonic_ms: u64) {
        if self.config.native_tape() {
            self.tape_clock.live_at(applied_ms, monotonic_ms);
        } else {
            self.tape_clock.reset();
        }
    }

    pub(crate) fn set_replay_clock_at(
        &mut self,
        position_ms: i64,
        applied_ms: Option<i64>,
        next_unapplied_ms: Option<i64>,
    ) {
        if self.config.native_tape() {
            self.tape_clock
                .replay_at(position_ms, applied_ms, next_unapplied_ms);
        } else {
            self.tape_clock.reset();
        }
    }
}
