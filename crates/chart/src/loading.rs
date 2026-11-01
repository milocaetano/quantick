//! The waits a tab can be on, counted.
//!
//! Every operation that can take noticeable time registers itself here under
//! its [`LoadingTask`]; [`crate::connection_state`] decides from these counts
//! which one the pane header names, and when.

/// Something slow the interface may be waiting on. One variant per kind of
/// wait; the label is what the user reads in the header's slot.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LoadingTask {
    /// The initial backfill, an on-demand "load older" request, or the refill
    /// after a source reset.
    History,
    /// Bars are being rebuilt from the retained trades after a bar-type or
    /// parameter change.
    BarRebuild,
    /// Order-book capture is connecting, buffering, fetching its snapshot or
    /// resyncing after a gap.
    BookSync,
    /// A recorded session is being parsed on a worker thread.
    ReplaySession,
    /// Venue candle history is on its way for a time pane.
    VenueHistory,
    /// Older prints are being cut into a coherent replacement series.
    HistoryRebuild,
}

impl LoadingTask {
    /// Every task, in declaration order.
    pub const ALL: [Self; 6] = [
        Self::History,
        Self::BarRebuild,
        Self::BookSync,
        Self::ReplaySession,
        Self::VenueHistory,
        Self::HistoryRebuild,
    ];

    /// What the slot says.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            // Network and recut are phases of one wait, with one stable word.
            Self::History | Self::HistoryRebuild => "loading history",
            Self::BarRebuild => "rebuilding bars",
            Self::BookSync => "syncing order book",
            Self::ReplaySession => "loading replay session",
            Self::VenueHistory => "loading venue history",
        }
    }

    /// This task's position in [`Self::ALL`] — the tracker's array index.
    /// Declaration order *is* the index, so adding a variant cannot drift.
    #[must_use]
    pub fn index(self) -> usize {
        self as usize
    }
}

/// How many operations of each kind are currently in flight.
///
/// Counted rather than boolean because several requests of one kind can
/// overlap (the feed command channel queues history loads) and the first
/// reply must not hide the indicator while others are still out. Waits owned
/// by someone else's state machine are mirrored level-style through
/// [`Self::set_active`] instead.
#[derive(Default)]
pub struct LoadingTracker {
    counts: [usize; LoadingTask::ALL.len()],
}

impl LoadingTracker {
    /// A tracker with nothing in flight.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// One more operation of this kind is in flight.
    pub fn begin(&mut self, task: LoadingTask) {
        let count = &mut self.counts[task.index()];
        *count = count.saturating_add(1);
    }

    /// One operation of this kind was answered. Saturates at zero so a stray
    /// reply can never underflow into "loading forever".
    pub fn end(&mut self, task: LoadingTask) {
        let count = &mut self.counts[task.index()];
        *count = count.saturating_sub(1);
    }

    /// Forget every operation of this kind and leave exactly one in flight —
    /// what a respawned or reset source needs, whose earlier requests will
    /// never be answered.
    pub fn restart(&mut self, task: LoadingTask) {
        self.counts[task.index()] = 1;
    }

    /// Level-triggered form for waits mirrored from state owned elsewhere:
    /// active or not, with no count to balance.
    pub fn set_active(&mut self, task: LoadingTask, active: bool) {
        self.counts[task.index()] = usize::from(active);
    }

    /// How many operations of this kind are in flight.
    #[must_use]
    pub fn count(&self, task: LoadingTask) -> usize {
        self.counts[task.index()]
    }

    /// Whether at least one operation of this kind is in flight.
    #[must_use]
    pub fn is_active(&self, task: LoadingTask) -> bool {
        self.count(task) > 0
    }

    /// Whether anything at all is in flight.
    #[must_use]
    pub fn any_active(&self) -> bool {
        self.counts.iter().any(|&count| count > 0)
    }

    /// The active tasks, in declaration order.
    pub fn active(&self) -> impl Iterator<Item = LoadingTask> + '_ {
        LoadingTask::ALL
            .into_iter()
            .filter(|task| self.is_active(*task))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_overlapping_operations_and_drains_to_zero() {
        let mut tracker = LoadingTracker::new();
        assert!(!tracker.any_active());

        tracker.begin(LoadingTask::History);
        tracker.begin(LoadingTask::History);
        assert_eq!(tracker.count(LoadingTask::History), 2);
        assert!(tracker.is_active(LoadingTask::History));

        tracker.end(LoadingTask::History);
        assert!(
            tracker.is_active(LoadingTask::History),
            "one reply, one load"
        );
        tracker.end(LoadingTask::History);
        assert!(!tracker.any_active());
    }

    #[test]
    fn ending_more_than_began_saturates_at_zero() {
        let mut tracker = LoadingTracker::new();
        tracker.end(LoadingTask::History);
        assert_eq!(tracker.count(LoadingTask::History), 0);
    }

    #[test]
    fn restart_leaves_exactly_one_in_flight() {
        let mut tracker = LoadingTracker::new();
        tracker.begin(LoadingTask::History);
        tracker.begin(LoadingTask::History);
        tracker.begin(LoadingTask::History);
        tracker.restart(LoadingTask::History);
        assert_eq!(tracker.count(LoadingTask::History), 1);
    }

    #[test]
    fn set_active_is_level_triggered() {
        let mut tracker = LoadingTracker::new();
        tracker.set_active(LoadingTask::BookSync, true);
        tracker.set_active(LoadingTask::BookSync, true);
        assert_eq!(tracker.count(LoadingTask::BookSync), 1, "no accumulation");
        tracker.set_active(LoadingTask::BookSync, false);
        assert!(!tracker.is_active(LoadingTask::BookSync));
    }

    #[test]
    fn tasks_are_independent() {
        let mut tracker = LoadingTracker::new();
        tracker.begin(LoadingTask::History);
        tracker.set_active(LoadingTask::ReplaySession, true);
        tracker.end(LoadingTask::History);
        assert!(!tracker.is_active(LoadingTask::History));
        assert!(tracker.is_active(LoadingTask::ReplaySession));
    }

    #[test]
    fn every_task_indexes_its_own_slot_and_has_a_label() {
        for (position, task) in LoadingTask::ALL.into_iter().enumerate() {
            assert_eq!(task.index(), position, "ALL and index() must agree");
            assert!(!task.label().is_empty());
        }
    }
}
