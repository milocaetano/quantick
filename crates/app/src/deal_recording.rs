//! Deal recording, as [`quantick_replay::recorder`] keeps it (the file, the
//! states, why it belongs to the asset), and where its files go this run.

use std::path::PathBuf;

pub(crate) use quantick_replay::recorder::*;

/// The directory under the cockpit home when nothing overrides it.
pub const DEALS_DIR: &str = "deals";

/// Scripted REC state: `QUANTICK_DEAL_RECORDING=on|off|menu`. `on`/`off`
/// override the default the tab would otherwise open with; `menu` opens the
/// REC popover on the first frame, for a capture.
#[cfg(any(feature = "scenario-harness", test))]
pub const RECORDING_HOOK_ENV: &str = "QUANTICK_DEAL_RECORDING";

/// Where recordings go this run.
///
/// The one-run override (`QUANTICK_DEALS_DIR`, read by the launch root)
/// first, then the config, then the cockpit home the other stores live in,
/// then the cwd-relative name for a run with no home.
#[must_use]
pub fn resolve_dir(configured: Option<&str>) -> PathBuf {
    if cfg!(test) {
        // Never the trader's documents from a test, like every other store.
        return crate::store_home::test_path(DEALS_DIR);
    }
    if let Some(explicit) = crate::launch::operator_paths()
        .deals_dir
        .as_deref()
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

#[cfg(any(feature = "scenario-harness", test))]
crate::hooks::declare_hooks!["QUANTICK_DEAL_RECORDING"];
