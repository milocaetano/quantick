//! Settled venue-prefix storage and deterministic replacement admission.
use quantick_chart_interaction::pane_history::{HistoryState, PrefixChange, PrefixIdentity};
use quantick_engine::Bar;
#[derive(Default)]
pub struct VenueHistory(Vec<Bar>);
impl VenueHistory {
    pub fn as_slice(&self) -> &[Bar] {
        &self.0
    }
    pub fn clear(&mut self) {
        self.0.clear();
    }
    pub fn install(
        &mut self,
        bars: Vec<Bar>,
        history: &mut HistoryState,
        lead_changed: bool,
    ) -> PrefixChange {
        let change = history.prefix_change(identity(&self.0), identity(&bars), lead_changed);
        if matches!(change, PrefixChange::Replace { .. }) {
            self.0 = bars;
        }
        change
    }
}
impl std::ops::Deref for VenueHistory {
    type Target = [Bar];
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl FromIterator<Bar> for VenueHistory {
    fn from_iter<T: IntoIterator<Item = Bar>>(iter: T) -> Self {
        Self(iter.into_iter().collect())
    }
}
fn identity(bars: &[Bar]) -> PrefixIdentity {
    PrefixIdentity {
        count: bars.len(),
        first: bars.first().map(|bar| bar.open_time),
        last: bars.last().map(|bar| bar.open_time),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn bar(ms: i64) -> Bar {
        Bar {
            open_time: ms,
            close_time: ms + 1,
            open: 1.into(),
            high: 1.into(),
            low: 1.into(),
            close: 1.into(),
            buy_volume: 1.into(),
            sell_volume: 0.into(),
            trade_count: 1,
        }
    }
    #[test]
    fn admission_owns_storage_and_preserves_lead_only_prefix() {
        let mut venue = VenueHistory::default();
        let mut history = HistoryState::default();
        assert_eq!(
            venue.install(vec![bar(1), bar(2)], &mut history, false),
            PrefixChange::Replace { delta: 2 }
        );
        assert_eq!(venue.len(), 2);
        assert_eq!(history.revision(), 1);
        assert_eq!(
            venue.install(vec![bar(1), bar(2)], &mut history, false),
            PrefixChange::Unchanged
        );
        assert_eq!(history.revision(), 1);
        assert_eq!(
            venue.install(vec![bar(1), bar(2)], &mut history, true),
            PrefixChange::LeadOnly
        );
        assert_eq!(venue[0].open_time, 1);
        assert_eq!(
            venue.install(vec![bar(2)], &mut history, false),
            PrefixChange::Replace { delta: -1 }
        );
        assert_eq!(venue.len(), 1);
        assert_eq!(venue[0].open_time, 2);
    }
}
