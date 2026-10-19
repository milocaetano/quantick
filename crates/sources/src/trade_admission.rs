//! Bounded exact-ID admission for one independently sequenced market session.
//!
//! A highest-ID-only filter would discard genuine reordered executions. This
//! ring remembers every observed ID in a numerical window, including holes.
//! IDs below that window cannot be verified and are explicitly classified as
//! too old, never claimed to be duplicates. Each feed session owns its ring.

/// Recent numerical source-ID span retained across reconnects (1 MiB).
pub const RECONNECT_ID_WINDOW: usize = 65_536;

/// Admission decision without changing quantity, side, time or execution ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TradeAdmission {
    /// Previously unseen ID inside the retained window, or a new high ID.
    Accept,
    /// This exact ID was already observed within the retained window.
    Duplicate,
    /// Older than the window; duplicate status cannot be established.
    TooOld,
}

/// Constant-memory, constant-time admission seeded with opening history.
pub struct RecentTradeIds {
    slots: Box<[Option<u64>]>,
    highest: Option<u64>,
}

impl Default for RecentTradeIds {
    fn default() -> Self {
        Self::with_window(RECONNECT_ID_WINDOW)
    }
}

impl RecentTradeIds {
    fn with_window(window: usize) -> Self {
        Self {
            slots: vec![None; window.max(1)].into_boxed_slice(),
            highest: None,
        }
    }

    /// Classify and remember one source ID. Call only within its market owner.
    pub fn observe(&mut self, id: u64) -> TradeAdmission {
        let window = self.slots.len() as u64;
        if self
            .highest
            .is_some_and(|highest| id <= highest && highest - id >= window)
        {
            return TradeAdmission::TooOld;
        }
        let slot = &mut self.slots[(id % window) as usize];
        if *slot == Some(id) {
            return TradeAdmission::Duplicate;
        }
        *slot = Some(id);
        self.highest = Some(self.highest.map_or(id, |highest| highest.max(id)));
        TradeAdmission::Accept
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_history_seeds_reconnects_but_does_not_fill_unseen_holes() {
        let mut ids = RecentTradeIds::with_window(4);
        for id in [100, 102, 103] {
            assert_eq!(ids.observe(id), TradeAdmission::Accept);
        }
        assert_eq!(ids.observe(102), TradeAdmission::Duplicate);
        assert_eq!(ids.observe(101), TradeAdmission::Accept);
        assert_eq!(ids.observe(101), TradeAdmission::Duplicate);
        assert_eq!(ids.observe(99), TradeAdmission::TooOld);
        assert_eq!(ids.observe(104), TradeAdmission::Accept);
        assert_eq!(ids.observe(100), TradeAdmission::TooOld);
        assert_eq!(ids.observe(102), TradeAdmission::Duplicate);
    }

    #[test]
    fn jumps_wraps_zero_and_maximum_id_preserve_the_bounded_contract() {
        let mut ids = RecentTradeIds::with_window(4);
        assert_eq!(ids.observe(0), TradeAdmission::Accept);
        assert_eq!(ids.observe(0), TradeAdmission::Duplicate);
        assert_eq!(ids.observe(u64::MAX), TradeAdmission::Accept);
        assert_eq!(ids.observe(u64::MAX - 3), TradeAdmission::Accept);
        assert_eq!(ids.observe(u64::MAX - 4), TradeAdmission::TooOld);
        assert_eq!(ids.observe(u64::MAX), TradeAdmission::Duplicate);
        assert_eq!(ids.slots.len(), 4);
    }

    #[test]
    fn markets_have_independent_id_ownership() {
        let mut spot = RecentTradeIds::default();
        let mut perpetual = RecentTradeIds::default();
        assert_eq!(spot.observe(42), TradeAdmission::Accept);
        assert_eq!(perpetual.observe(42), TradeAdmission::Accept);
        assert_eq!(spot.observe(42), TradeAdmission::Duplicate);
    }
}
