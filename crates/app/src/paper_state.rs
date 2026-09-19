//! The paper-trading sidecar: in-app choices that must survive a restart.
//!
//! One value set, one home: the shipped base stays in the config
//! (`[paper] trades_dir`), environment variables stay the explicit
//! per-run overrides, and this sidecar records what the user changed
//! *in-app* — the added-symbols pattern: an in-app edit must survive a
//! restart without the app rewriting the user's hand-commented
//! `quantick.toml` (a TOML writer would strip its comments, which is
//! exactly what the config rules forbid). Today that is the journal
//! folder picked with the panel's button and the cmd-trading settings.
//!
//! Same store discipline as the chart layers: a versioned TOML next to the
//! config (override with `QUANTICK_PAPER_STATE`), read once at startup,
//! written when a choice changes, temp-file-and-rename so a crash
//! mid-write cannot leave half a file behind. Anything unreadable is
//! ignored entirely.

use std::path::PathBuf;

pub(crate) use quantick_paper::state::*;

/// The file's name inside the durable cockpit home. See [`crate::store_home`].
pub(crate) const STATE_FILE: &str = "paper-state.toml";

/// The paper-state file the app opens with and writes back to. Under test
/// it is a scratch file of its own per process, for the same reason the
/// chart layers do it: tests must not restore one another's choice, nor
/// rewrite the repo's copy.
#[must_use]
pub(crate) fn default_path() -> PathBuf {
    if cfg!(test) {
        return scratch_path();
    }
    crate::store_home::resolve(STATE_FILE)
}

/// A store of its own, for tests. See [`default_path`].
fn scratch_path() -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    // Inside the thread's own directory rather than beside it: the file is
    // then removed with the directory when the test's thread ends, and the
    // counter only has to separate this thread's files from each other.
    crate::scratch::thread_dir("paper-state")
        .join(format!("{}.toml", NEXT.fetch_add(1, Ordering::Relaxed)))
}
