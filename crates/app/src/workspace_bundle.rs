//! One file that holds the whole cockpit — the thing a trader can copy, keep
//! and carry to another machine.
//!
//! [`crate::store_home`] fixed *where* the cockpit lives, so it stops
//! disappearing between launches. This module answers the other half: a
//! trader who has arranged a screen wants to keep that arrangement somewhere
//! they can see — a file in Documents, next to everything else they own —
//! and to come back to it later, or on another computer.
//!
//! **Why a bundle and not the workspace file.** `ui-state.toml` remembers the
//! tabs and the chrome, and the named arrangements inside it remember those
//! too. But which indicators are on and how they are tuned, the chart layers,
//! the drawing colours, the footprint and the hand-added symbols each live in
//! a store of their own. Reopening a named arrangement therefore brought back
//! *part* of a cockpit, and — worse — gave the trader no way to see which
//! part was missing. A bundle is every one of those stores under one roof:
//! what you saved is what comes back.
//!
//! **Sections are the stores' own files, verbatim.** Each section is the
//! parsed content of one store's file under that store's key. No section
//! knows it is in a bundle, and adding a ninth store adds no code here — it
//! adds one entry to [`COCKPIT_STORES`]. That is what keeps this module from
//! becoming a second, drifting description of everything the app remembers.
//!
//! **Importing is all-or-nothing.** Every section is parsed with its real
//! type, and every replacement file is written, *before any store is
//! replaced* ([`apply`]). A bundle that fails anywhere is refused whole, with
//! the reason — half a cockpit is worse than none, because the trader cannot
//! see the half that is missing. It is the same rule
//! [`crate::ui_state::load`] already applies to a single store, applied
//! across all of them. The honest limit: the final phase is a sequence of
//! renames, atomic one at a time but not as a group, so a failure *there* is
//! reported by name rather than hidden.
//!
//! **A bundle carries a cockpit, not a machine.** Keys that describe this
//! installation — the recent files, the named bookmarks, the replay folder —
//! are stripped on capture and kept on apply
//! ([`crate::store_home::CockpitStore::local_keys`]), so opening a
//! colleague's workspace never costs the trader their own.
//!
//! The paper sidecar shares the durable home but is deliberately not a
//! section here: see [`crate::store_home::COCKPIT_STORES`].

#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;

#[cfg(test)]
use crate::store_home::CockpitStore;
pub(crate) use quantick_stores::bundle::*;
#[cfg(test)]
use quantick_workspace::bundle::FORMAT_VERSION;

/// The folder exports land in by default: `Documents/Quantick/workspaces`,
/// beside the journal. `None` when the platform reports no documents folder,
/// in which case the picker simply opens wherever it likes.
pub(crate) fn default_dir() -> Option<PathBuf> {
    crate::store_home::home().map(|home| home.join(BUNDLE_DIR))
}

#[cfg(test)]
#[path = "workspace_bundle/tests/baseline.rs"]
mod baseline_tests;

#[cfg(test)]
mod recovery_tests;

#[cfg(test)]
mod tests;
