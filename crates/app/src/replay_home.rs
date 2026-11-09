//! This run's replay folder: the environment's half of `quantick_replay::home`.

use std::path::PathBuf;

/// The replay folder for this run, as text for the browser's field.
///
/// The env var wins *for this run only*. What the trader chose stays what they
/// chose: a QA or autostart run pointed at a scratch folder must not come back
/// as a permanent pick, which is the same line [`crate::paper_home`] draws
/// around its own per-run override.
#[must_use]
pub(crate) fn resolve(stored: Option<&str>) -> String {
    quantick_replay::home::resolve_with(
        // `QUANTICK_REPLAY_DIR`, read by the launch root.
        crate::launch::operator_paths().replay_dir.as_deref(),
        stored,
        quantick_replay::home::tape_shelf(
            crate::paper_home::DOCUMENTS_SHELF,
            crate::paper_home::documents_dir(),
            // The sync client states its own root; quantick does not go
            // looking for one by name, which would call any folder with
            // "OneDrive" in its path synced.
            std::env::var_os(quantick_replay::home::SYNCED_ROOT_ENV).map(PathBuf::from),
            dirs::home_dir(),
        ),
    )
}
