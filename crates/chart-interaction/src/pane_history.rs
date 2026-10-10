//! Series revision and deferred drawing-anchor bookkeeping owned by a pane.
#[derive(Debug, Default)]
pub struct HistoryState {
    revision: u64,
    pending_reanchor: Option<usize>,
}
impl HistoryState {
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn changed(&mut self) {
        self.revision = self.revision.saturating_add(1);
    }
    /// Repeated resets retain the first series' anchor basis.
    pub fn defer_reanchor(&mut self, old_slots: usize) {
        self.pending_reanchor.get_or_insert(old_slots);
    }
    #[must_use]
    pub fn settle_reanchor(&mut self, slots: usize) -> Option<usize> {
        if slots == 0 {
            None
        } else {
            self.pending_reanchor.take()
        }
    }
    #[must_use]
    pub fn edge_anchor(viewport: &crate::viewport::Viewport, slots: usize) -> (usize, f32) {
        let edge = viewport.right_edge_bar(slots);
        let reference = (edge.floor().max(0.0) as usize).min(slots.saturating_sub(1));
        (reference, edge - reference as f32)
    }
}

/// Deterministic folds are identified by their length and endpoint windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrefixIdentity {
    pub count: usize,
    pub first: Option<i64>,
    pub last: Option<i64>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefixChange {
    Unchanged,
    LeadOnly,
    Replace { delta: isize },
}
impl HistoryState {
    pub fn prefix_change(
        &mut self,
        old: PrefixIdentity,
        new: PrefixIdentity,
        lead_changed: bool,
    ) -> PrefixChange {
        if old == new {
            if lead_changed {
                self.changed();
                PrefixChange::LeadOnly
            } else {
                PrefixChange::Unchanged
            }
        } else {
            self.changed();
            PrefixChange::Replace {
                delta: new.count as isize - old.count as isize,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn empty_and_repeated_resets_preserve_the_first_anchor_basis() {
        let mut state = HistoryState::default();
        state.defer_reanchor(100);
        state.defer_reanchor(0);
        assert_eq!(state.settle_reanchor(0), None);
        assert_eq!(state.settle_reanchor(20), Some(100));
        assert_eq!(state.settle_reanchor(20), None);
    }
    #[test]
    fn settled_prefix_and_lead_changes_distinguish_rebuild_from_signed_anchor_shift() {
        let mut state = HistoryState::default();
        let old = PrefixIdentity {
            count: 10,
            first: Some(0),
            last: Some(9),
        };
        assert_eq!(
            state.prefix_change(old, old, false),
            PrefixChange::Unchanged
        );
        assert_eq!(state.revision(), 0);
        assert_eq!(state.prefix_change(old, old, true), PrefixChange::LeadOnly);
        let shorter = PrefixIdentity {
            count: 4,
            first: Some(0),
            last: Some(9),
        };
        assert_eq!(
            state.prefix_change(old, shorter, false),
            PrefixChange::Replace { delta: -6 }
        );
        assert_eq!(state.revision(), 2);
        let shifted = PrefixIdentity {
            first: Some(-10),
            ..old
        };
        assert_eq!(
            state.prefix_change(old, shifted, false),
            PrefixChange::Replace { delta: 0 }
        );
        assert_eq!(state.revision(), 3);
    }
}
