//! Route the source clock through the applied chart tape before painting.
use super::Tab;

impl Tab {
    pub(crate) fn update_tape_clock_at(&mut self, monotonic_ms: u64) {
        let applied = self.flow_pane.state.trades();
        let applied_ms = applied.last().map(|trade| trade.timestamp_ms);
        // TradeTape retains every print and never evicts a prefix. A replay
        // reset replaces it before its new backfill, so this is the applied
        // cursor in that recording, including a joined prior session.
        let replay = self.replay.as_ref().map(|replay| {
            (
                replay.status.position_ms(),
                replay
                    .session
                    .trades
                    .get(applied.len())
                    .map(|trade| trade.timestamp_ms),
            )
        });
        match replay {
            Some((position_ms, next_ms)) => {
                self.tape_mut()
                    .set_replay_clock_at(position_ms, applied_ms, next_ms);
            }
            None => self.tape_mut().set_live_clock_at(applied_ms, monotonic_ms),
        }
    }
}
