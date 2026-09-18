//! Where the paper-trading journal lives by default, and the consolidation
//! that rescues history from the cwd-relative era.
//!
//! Before this module every quantick path was cwd-relative — launching the
//! app from a different folder silently switched the journal, and even the
//! sidecar remembering the user's picked folder. Trades were never deleted,
//! but they were scattered; that is the "my old trades are gone" report.
//! The durable home is the user's documents folder
//! (`Documents/Quantick/paper-trades`), the one path that does not care
//! where the app was launched from.
//!
//! Resolution keeps its spirit — an explicit ask always wins:
//! `QUANTICK_TRADES_DIR` (one run) > the folder picked in-app
//! (`paper-state.toml`) > `[paper] trades_dir` in the config > the
//! documents home. Exactly once — recorded by a `.consolidated` marker in
//! the home itself, so the one-time-ness holds from every launch
//! directory — startup also *consolidates*: copy — never move, never
//! delete — what the reachable legacy locations still hold into the home,
//! retiring the pre-rescue pick. From then on a pick made in-app is a
//! standing choice again and survives every restart. The copy skips
//! byte-identical files, so the ongoing sweep of the cwd-relative legacy
//! folder stays safe to repeat.

pub(crate) use quantick_paper::home::*;

use std::path::{Path, PathBuf};

/// Overrides the history folder for one run — the autostart family's
/// explicit ask; nothing is consolidated or cleared under it.
pub(crate) const TRADES_DIR_ENV: &str = "QUANTICK_TRADES_DIR";

/// That shelf as a path, when the platform reports a documents folder.
///
/// The one place the shelf is named. Everything quantick keeps for a trader —
/// the journal, the replay folder, the broker clock — hangs off this, so
/// renaming it is one edit rather than a hunt through three modules.
pub(crate) fn shelf_dir() -> Option<PathBuf> {
    documents_dir().map(|documents| documents.join(DOCUMENTS_SHELF))
}

/// The user's documents folder, when the platform knows one.
pub(crate) fn documents_dir() -> Option<PathBuf> {
    dirs::document_dir()
}

/// The journal folder for this run, without consolidation — the resolution
/// order from the module doc, for hosts that only need an answer.
pub(crate) fn resolve(configured: Option<&str>, stored: Option<&str>) -> PathBuf {
    std::env::var_os(TRADES_DIR_ENV).map_or_else(
        || chosen(configured, stored, documents_dir()),
        PathBuf::from,
    )
}

/// Resolve the journal home for this run and, exactly once, consolidate
/// the reachable legacy locations into it. The one-time-ness lives as a
/// marker file in the home itself (cwd-independent, unlike this sidecar);
/// after the rescue has run, a stored in-app pick is a live choice again
/// and wins, exactly as the picker's tooltip promises.
pub(crate) fn startup_home(
    configured: Option<&str>,
    stored: Option<&str>,
    state_path: &Path,
) -> (PathBuf, Option<ImportSummary>) {
    if cfg!(test) {
        // Tests must never scan, consolidate into, or journal under a
        // real documents folder — the same scratch discipline
        // `PaperTrading::new` applies. Per test thread rather than per
        // process, which is strictly more isolation than the shared folder
        // this replaced, and is what lets the directory be removed when the
        // thread ends instead of accumulating one per run for ever.
        return (crate::scratch::thread_dir("paper-home"), None);
    }
    if let Some(env) = std::env::var_os(TRADES_DIR_ENV) {
        // An explicit per-run ask — a QA or autostart run must never
        // touch, or even scan, the real home.
        return (PathBuf::from(env), None);
    }
    resolve_startup_home(
        configured,
        stored,
        state_path,
        documents_dir(),
        Path::new(LEGACY_TRADES_DIR),
    )
}

crate::hooks::declare_hooks!["QUANTICK_TRADES_DIR"];
