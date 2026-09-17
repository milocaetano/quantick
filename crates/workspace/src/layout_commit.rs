//! Deterministic layout write decisions. The shell supplies time and executes effects.
use std::time::{Duration, Instant};

pub const LAYOUTS_SAVE_DEBOUNCE: Duration = Duration::from_millis(1_000);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutSave {
    /// Nothing to write, or the debounce window has not elapsed. The pending
    /// change, if there is one, is left pending.
    Wait,
    /// Write the book to the layouts path. The change is consumed.
    Write,
    /// There is a change, and this session may not write the file. The change
    /// is consumed and the caller reports it; see [`LayoutCommitPolicy::set_blocked`].
    Blocked,
}

pub struct LayoutCommitPolicy {
    dirty: bool,
    last_change: Option<Instant>,
    blocked: bool,
}

impl LayoutCommitPolicy {
    pub fn new(blocked: bool) -> Self {
        Self {
            dirty: false,
            last_change: None,
            blocked,
        }
    }
    pub fn set_blocked(&mut self, blocked: bool) {
        self.blocked = blocked;
    }
    /// State outright what the book owes the file, overriding anything marked
    /// while it was being put in place.
    ///
    /// The screen is the file's after an import; nothing has changed since —
    /// unless the book was made from an imported indicator set, which the file
    /// does not hold yet. Either way this is the last word, which is why it
    /// clears as readily as it sets.
    ///
    /// **Call it after the caller has finished seeding**, which is where the
    /// two assignments it replaces stood. `seed_new_panes` marks no change
    /// today, so the order is not observable and no test pins it — which is
    /// precisely why it is written here. A later reading of "these are just
    /// two flags" moves the call above the seeding; the day seeding does mark
    /// a change, an import begins writing the file back over itself and
    /// nothing fails to say so.
    pub fn settle(&mut self, changed: bool, now: Instant) {
        self.dirty = changed;
        self.last_change = changed.then_some(now);
    }

    /// Record that the book changed, and when.
    ///
    /// The flag and the clock move together or not at all. That is the whole
    /// point of this type.
    pub fn mark_changed(&mut self, now: Instant) {
        self.dirty = true;
        self.last_change = Some(now);
    }

    /// Whether an edit is waiting for the debounce.
    ///
    /// A diagnostic observation for consumers. Only [`Self::take_save`] and
    /// [`Self::take_flush`] authorize and consume a write.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// The frame's question: is there a change, and has it settled?
    ///
    /// `Wait` leaves the change pending. `Write` and `Blocked` both consume
    /// it, which is what the pre-existing `save_layouts_now` did — it cleared
    /// both flags before it checked whether it was allowed to write.
    pub fn take_save(&mut self, now: Instant) -> LayoutSave {
        let settled = self
            .last_change
            .is_some_and(|changed| now.duration_since(changed) >= LAYOUTS_SAVE_DEBOUNCE);
        if !settled {
            return LayoutSave::Wait;
        }
        self.take()
    }

    /// The way out on exit, and the moment before a bundle export reads the
    /// file: write now, whatever the debounce says.
    pub fn take_flush(&mut self) -> LayoutSave {
        self.take()
    }

    /// The half both questions share: consume a pending change and say where
    /// it goes. Split out so `Blocked` can never disagree with `Write` about
    /// what was consumed.
    fn take(&mut self) -> LayoutSave {
        if !self.dirty {
            return LayoutSave::Wait;
        }
        self.dirty = false;
        self.last_change = None;
        if self.blocked {
            LayoutSave::Blocked
        } else {
            LayoutSave::Write
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// A store with a book, at a path no test ever writes to, blocked or not.
    fn store(blocked: bool) -> LayoutCommitPolicy {
        LayoutCommitPolicy::new(blocked)
    }

    #[test]
    fn a_change_inside_the_debounce_window_is_not_yet_asked_for() {
        let mut layouts = store(false);
        let changed = Instant::now();
        layouts.mark_changed(changed);
        assert_eq!(
            layouts.take_save(changed + LAYOUTS_SAVE_DEBOUNCE - Duration::from_millis(1)),
            LayoutSave::Wait,
            "a change one millisecond short of the window must not reach the file"
        );
        assert!(
            layouts.is_dirty(),
            "a change the debounce held back is still pending, not consumed"
        );
    }

    #[test]
    fn a_change_that_has_settled_is_asked_for_once() {
        let mut layouts = store(false);
        let changed = Instant::now();
        layouts.mark_changed(changed);
        assert_eq!(
            layouts.take_save(changed + LAYOUTS_SAVE_DEBOUNCE),
            LayoutSave::Write,
            "the window's own edge releases the change"
        );
        assert_eq!(
            layouts.take_save(changed + LAYOUTS_SAVE_DEBOUNCE),
            LayoutSave::Wait,
            "a change that reached the file is not written a second time"
        );
    }

    #[test]
    fn a_blocked_store_never_asks_to_write() {
        let mut layouts = store(true);
        let changed = Instant::now();
        layouts.mark_changed(changed);
        assert_eq!(
            layouts.take_save(changed + LAYOUTS_SAVE_DEBOUNCE),
            LayoutSave::Blocked,
            "a session that could not read the file must not write over it"
        );
        layouts.mark_changed(changed);
        assert_eq!(
            layouts.take_flush(),
            LayoutSave::Blocked,
            "not even the exit flush, which ignores the debounce, may write it"
        );
    }

    #[test]
    fn the_exit_flush_ignores_the_debounce_but_not_the_absence_of_a_change() {
        let mut layouts = store(false);
        assert_eq!(
            layouts.take_flush(),
            LayoutSave::Wait,
            "nothing changed, so exiting writes nothing"
        );
        layouts.mark_changed(Instant::now());
        assert_eq!(
            layouts.take_flush(),
            LayoutSave::Write,
            "a change still inside the window is written on the way out, not lost"
        );
    }

    #[test]
    fn marking_a_change_moves_the_flag_and_the_clock_together() {
        let mut layouts = store(false);
        assert!(!layouts.is_dirty());
        let changed = Instant::now();
        layouts.mark_changed(changed);
        assert!(layouts.is_dirty());
        // The clock is not readable from outside; that it was stamped is
        // proven by the debounce releasing on time rather than never.
        assert_eq!(
            layouts.take_save(changed + LAYOUTS_SAVE_DEBOUNCE),
            LayoutSave::Write,
            "a change recorded without its timestamp would never settle"
        );
    }

    #[test]
    fn settling_a_replaced_book_overrides_what_landing_it_marked() {
        let mut layouts = store(false);
        let now = Instant::now();
        // Putting the book in place seeds panes, and seeding marks changes.
        layouts.mark_changed(now);
        layouts.settle(false, now);
        assert_eq!(
            layouts.take_save(now + LAYOUTS_SAVE_DEBOUNCE),
            LayoutSave::Wait,
            "an import that changed nothing must not write the file back, whatever seeding its panes marked on the way"
        );
        layouts.settle(true, now);
        assert_eq!(
            layouts.take_save(now + LAYOUTS_SAVE_DEBOUNCE),
            LayoutSave::Write,
            "an import that migrated something owes the file a write"
        );
    }
}
