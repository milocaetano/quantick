//! Resolved layout document storage and commit policy, independent of presentation.
use crate::{
    layout_commit::{LayoutCommitPolicy, LayoutSave},
    layout_document::LayoutBook,
    session::LayoutSession,
};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// The layout book, and the one rule that decides when it reaches disk.
///
/// The pure commit policy owns the dirty/timestamp/blocked transition. This
/// handle associates its effects with the already resolved layout path.
pub struct LayoutStore {
    /// The workspace's layouts: the strip's tabs, their indicator sets and
    /// their per-market drawings. See [`crate::layout_document`].
    session: LayoutSession,
    /// Where the layouts persist. Handed in, never resolved here.
    path: PathBuf,
    policy: LayoutCommitPolicy,
}

impl LayoutStore {
    /// A store over a book already loaded, at a path already resolved.
    ///
    /// `blocked` comes from the load: it is the "could not read it and could
    /// not set it aside" answer, which only the loader can give.
    pub fn new(book: LayoutBook, path: PathBuf, blocked: bool) -> Self {
        Self {
            session: LayoutSession::new(book),
            path,
            policy: LayoutCommitPolicy::new(blocked),
        }
    }

    /// The book, for the strip, the menu and the control plane to read.
    pub fn book(&self) -> &LayoutBook {
        self.session.book()
    }

    pub fn session(&self) -> &LayoutSession {
        &self.session
    }
    pub fn session_mut(&mut self) -> &mut LayoutSession {
        &mut self.session
    }

    /// Redirect a fixture without exposing the document or commit policy.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_path(&mut self, path: PathBuf) {
        self.path = path;
    }

    /// Where the layouts persist.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Note that the file could not be read at launch, so this session's book
    /// must not replace it.
    pub fn set_blocked(&mut self, blocked: bool) {
        self.policy.set_blocked(blocked);
    }

    /// Replace the book wholesale — a workspace bundle landing, or a reset.
    ///
    /// Deliberately silent about the flags. The caller has work to do between
    /// putting the book in place and knowing what it owes the file (it seeds
    /// panes, which marks changes of its own), so the pending state is stated
    /// afterwards by [`Self::settle`] rather than guessed at here.
    pub fn set_book(&mut self, book: LayoutBook) {
        self.session.replace_book(book);
    }

    pub fn settle(&mut self, changed: bool, now: Instant) {
        self.policy.settle(changed, now);
    }
    pub fn mark_changed(&mut self, now: Instant) {
        self.policy.mark_changed(now);
    }
    pub fn take_save(&mut self, now: Instant) -> LayoutSave {
        self.policy.take_save(now)
    }
    pub fn take_flush(&mut self) -> LayoutSave {
        self.policy.take_flush()
    }
    pub fn is_dirty(&self) -> bool {
        self.policy.is_dirty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn blocked_and_debounced_documents_keep_their_once_only_commit_policy() {
        let now = Instant::now();
        let mut store =
            LayoutStore::new(LayoutBook::default(), PathBuf::from("fixture.toml"), true);
        store.mark_changed(now);
        assert_eq!(store.take_flush(), LayoutSave::Blocked);
        assert_eq!(store.take_flush(), LayoutSave::Wait);
        store.set_blocked(false);
        store.mark_changed(now);
        assert_eq!(store.take_save(now), LayoutSave::Wait);
        assert_eq!(store.take_flush(), LayoutSave::Write);
        assert_eq!(store.take_flush(), LayoutSave::Wait);
    }
}
