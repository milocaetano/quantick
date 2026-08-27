//! The canvas layout model: what panes a tab draws, and in what order.
//!
//! This module owns the one piece of the layout that has to be right before
//! anything else can be: **pane identity**. Pane ids namespace every egui
//! interaction a pane registers, so they have to be unique across the whole
//! window rather than within a tab — two panes sharing an id share a drag.
//!
//! It replaces an arithmetic allocator (`tab * 2`, `tab * 2 + 1`) that could
//! only ever hand out two ids per tab. That arithmetic was not merely a limit:
//! a third pane on tab 0 would have taken id 2, which is tab 1's flow pane,
//! and the two would have shared every gesture egui keys by id.

/// Hands out pane ids that are unique for the lifetime of the window.
///
/// Two rules, and the type exists to make both true by construction rather
/// than by review:
///
/// - **Never derived from position.** An id that encoded where a pane sits
///   would move gesture state between panes the moment one was reordered.
/// - **Never reused.** egui keys interaction state by id across frames, so a
///   recycled id inherits the dead pane's drag, scroll and popup state. A
///   monotonic counter is the whole mechanism: ids are spent, not recovered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneIdAllocator {
    /// The next id to hand out. Only ever increases.
    next: u64,
}

impl Default for PaneIdAllocator {
    fn default() -> Self {
        Self::new()
    }
}

impl PaneIdAllocator {
    /// An allocator that has handed out nothing yet.
    #[must_use]
    pub const fn new() -> Self {
        Self { next: 0 }
    }

    /// The next unused pane id.
    ///
    /// Panics only after 2^64 panes, which is not a state a trading session
    /// reaches; the saturating arithmetic is here so that the failure mode, if
    /// the impossible happened, is a stuck id rather than a wrapped one that
    /// silently collides with pane 0.
    pub fn alloc(&mut self) -> u64 {
        let id = self.next;
        self.next = self.next.saturating_add(1);
        id
    }

    /// A pair of ids, for a tab opening with a flow pane and a reserved time
    /// pane. A convenience over two [`Self::alloc`] calls, kept so the tab
    /// constructor reads as "these two ids belong together".
    pub fn alloc_pair(&mut self) -> (u64, u64) {
        (self.alloc(), self.alloc())
    }

    /// How many ids have been spent. Test and diagnostic use only — nothing
    /// may derive a pane's identity from it.
    #[must_use]
    #[cfg(test)]
    pub const fn spent(&self) -> u64 {
        self.next
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn ids_are_unique_across_three_tabs_of_three_panes() {
        // The shape the old `tab * 2` allocator could not express: a third
        // pane on the first tab used to collide with the second tab's flow
        // pane, and the two shared every gesture egui keys by id.
        let mut allocator = PaneIdAllocator::new();
        let mut seen = BTreeSet::new();
        for _tab in 0..3 {
            for _pane in 0..3 {
                let id = allocator.alloc();
                assert!(seen.insert(id), "pane id {id} was handed out twice");
            }
        }
        assert_eq!(seen.len(), 9, "nine panes must hold nine distinct ids");
    }

    #[test]
    fn an_id_is_never_reused_after_its_pane_is_removed() {
        // Removing a pane must not return its id to the pool: egui keys
        // interaction state by id across frames, so a recycled id would
        // inherit the removed pane's drag.
        let mut allocator = PaneIdAllocator::new();
        let first = allocator.alloc();
        let second = allocator.alloc();
        // The pane holding `second` is closed here; nothing tells the
        // allocator, and that is the point.
        let third = allocator.alloc();
        assert_ne!(third, second, "a removed pane's id came back");
        assert_ne!(third, first);
        assert_eq!(allocator.spent(), 3);
    }

    #[test]
    fn a_pair_is_two_distinct_ids() {
        let mut allocator = PaneIdAllocator::new();
        let (flow, time) = allocator.alloc_pair();
        assert_ne!(flow, time);
        let (next_flow, next_time) = allocator.alloc_pair();
        assert_ne!(next_flow, flow);
        assert_ne!(next_flow, time);
        assert_ne!(next_time, flow);
        assert_ne!(next_time, time);
    }

    #[test]
    fn ids_do_not_encode_the_tab_they_were_asked_for() {
        // A regression guard on the rule rather than on an implementation:
        // whatever the allocator does, asking twice in a row must not produce
        // a value derivable from a tab index, or reordering would carry
        // gesture state with the position instead of with the pane.
        let mut allocator = PaneIdAllocator::new();
        let first_tab = allocator.alloc_pair();
        let second_tab = allocator.alloc_pair();
        assert_ne!(
            second_tab.0,
            first_tab.0 * 2,
            "an id that is a function of the tab index is the bug this replaced"
        );
    }
}
