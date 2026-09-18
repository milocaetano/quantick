//! Deal recording — the venue's session deal counter, kept and written down.
//!
//! MetaTrader folds several exchange deals into one tick and keeps no count
//! per tick; the session's running total is the only deal count it has, and
//! it exists only while the session is on. So a chart that cuts bars every
//! N deals (the `trades` kind) can be rebuilt for a day only from what
//! *someone wrote down while it happened*. This module is that someone:
//! it holds the readings a feed delivers, appends them to one file per
//! symbol and day, and reads such a file back so the day reopens as the
//! same chart — after a restart, or a week later.
//!
//! Recording belongs to the **asset**, never to the pane. Switching the pane
//! from `trades` to `tick` changes what is drawn and nothing else; the
//! recorder keeps writing, and the readings keep being retained by every
//! pane on the tab, so switching back rebuilds from the whole series.
//!
//! Three states a trader has to be able to tell apart, and this module
//! names them so every surface says the same word:
//!
//! - [`RecState::Recording`] — the market is being written down now.
//! - [`RecState::Recorded`] — what is on screen came from a file; nothing is
//!   being written.
//! - no deal count — prints the rule cannot place, before the first reading
//!   or on a day nobody recorded. Reported as a number, never as a bar.
//!
//! # The file
//!
//! `<dir>/<SYMBOL>/<YYYY-MM-DD>.deals`, text, append-only:
//!
//! ```text
//! # quantick-deals v1 symbol=WINV26 day=2026-09-03 tz_minutes=-180
//! 1788436967023 1990
//! +20 +13
//! ```
//!
//! The first data line is absolute, every later one a delta from the line
//! before — a poll every 20 ms all day is a million lines, and deltas keep
//! that at a few megabytes rather than forty. The day is the one the first
//! reading fell on in the display timezone; a reading that falls on the next
//! day rotates to the next file. The directory is `QUANTICK_DEALS_DIR`, then
//! the `[deals] dir` config key, then `deals/` in the cockpit home
//! (`Documents/Quantick`).

use std::path::PathBuf;

pub(crate) use quantick_replay::recorder::*;

/// One-run override of the recording directory.
pub const DEALS_DIR_ENV: &str = "QUANTICK_DEALS_DIR";

/// The directory under the cockpit home when nothing overrides it.
pub const DEALS_DIR: &str = "deals";

/// Scripted REC state: `QUANTICK_DEAL_RECORDING=on|off|menu`. `on`/`off`
/// override the default the tab would otherwise open with; `menu` opens the
/// REC popover on the first frame, for a capture.
pub const RECORDING_HOOK_ENV: &str = "QUANTICK_DEAL_RECORDING";

/// Where recordings go this run.
///
/// The one-run override first, then the config, then the cockpit home the
/// other stores live in, then the cwd-relative name for a run with no home.
#[must_use]
pub fn resolve_dir(configured: Option<&str>) -> PathBuf {
    if cfg!(test) {
        // Never the trader's documents from a test, like every other store.
        return crate::store_home::test_path(DEALS_DIR);
    }
    if let Some(explicit) = std::env::var(DEALS_DIR_ENV)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
    {
        return PathBuf::from(explicit);
    }
    if let Some(dir) = configured.map(str::trim).filter(|dir| !dir.is_empty()) {
        return PathBuf::from(dir);
    }
    crate::store_home::home().map_or_else(|| PathBuf::from(DEALS_DIR), |home| home.join(DEALS_DIR))
}

crate::hooks::declare_hooks!["QUANTICK_DEALS_DIR", "QUANTICK_DEAL_RECORDING"];
