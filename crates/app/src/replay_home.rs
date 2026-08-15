//! Where recorded sessions live by default, and how that choice survives a
//! restart.
//!
//! Market Replay used to know one folder: whatever `QUANTICK_REPLAY_DIR` said,
//! and an empty string when it said nothing. Nothing was ever written down, so
//! every launch opened the browser on nowhere — the recordings were on disk,
//! intact, and the app simply did not look. A trader who downloads a week of
//! tape then finds an empty list the next morning does not conclude "the folder
//! field is blank"; they conclude replay is broken.
//!
//! Resolution follows [`crate::paper_home`]'s order, for the same reasons —
//! an explicit ask wins, a standing choice outlives it, and the fallback is a
//! place the trader can find in their own file manager:
//!
//! `QUANTICK_REPLAY_DIR` (one run) > the folder picked in-app (the workspace
//! file) > `Documents/Quantick/replay`.
//!
//! The home is never created here. A folder that does not exist yet scans to
//! "no sessions", which is the truth, and the first download makes it — so a
//! fresh install neither lies about what it holds nor litters the documents
//! folder before the trader has asked for anything.

use std::path::PathBuf;

/// Overrides the replay folder for one run — the autostart family's explicit
/// ask, and the hook the validation harness drives.
pub(crate) const REPLAY_DIR_ENV: &str = "QUANTICK_REPLAY_DIR";
/// The folder under quantick's documents shelf. The shelf itself is named once,
/// in [`crate::paper_home`], so everything quantick keeps for a trader is under
/// one roof and renaming that roof stays one edit.
const REPLAY_DIR: &str = "replay";

/// The default home given a documents folder — and a cwd-relative `replay`
/// when the platform reports none (headless CI, bare setups).
///
/// Inventing a path the trader cannot find would be worse than the honest
/// relative one, which is the same call [`crate::paper_home`] makes.
pub(crate) fn default_dir(shelf: Option<PathBuf>) -> PathBuf {
    shelf.map_or_else(|| PathBuf::from(REPLAY_DIR), |shelf| shelf.join(REPLAY_DIR))
}

/// The folder before the environment has its say: the trader's own stored pick
/// when there is one, else the documents home.
///
/// A stored pick that is blank is treated as no pick. An empty string is what
/// the old field held when nothing had been chosen, and honouring it would
/// resolve to the process's working directory — the bug this module exists to
/// end, preserved in a file.
fn chosen(stored: Option<&str>, shelf: Option<PathBuf>) -> PathBuf {
    stored
        .map(str::trim)
        .filter(|stored| !stored.is_empty())
        .map_or_else(|| default_dir(shelf), PathBuf::from)
}

/// The replay folder for this run, given every input — the form the tests
/// drive, because reading the environment inside a constructor makes the
/// answer depend on the machine the suite runs on.
fn resolve_with(from_env: Option<&str>, stored: Option<&str>, shelf: Option<PathBuf>) -> String {
    let path = from_env
        .map(str::trim)
        .filter(|from_env| !from_env.is_empty())
        .map_or_else(|| chosen(stored, shelf), PathBuf::from);
    path.display().to_string()
}

/// The replay folder for this run, as text for the browser's field.
///
/// The env var wins *for this run only*. What the trader chose stays what they
/// chose: a QA or autostart run pointed at a scratch folder must not come back
/// as a permanent pick, which is the same line [`crate::paper_home`] draws
/// around its own per-run override.
#[must_use]
pub(crate) fn resolve(stored: Option<&str>) -> String {
    resolve_with(
        std::env::var(REPLAY_DIR_ENV).ok().as_deref(),
        stored,
        crate::paper_home::shelf_dir(),
    )
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn a_stored_pick_wins_over_the_documents_home() {
        assert_eq!(
            chosen(Some("D:/tape"), Some(PathBuf::from("/docs/Quantick"))),
            PathBuf::from("D:/tape"),
        );
    }

    #[test]
    fn no_pick_lands_under_the_documents_shelf() {
        assert_eq!(
            chosen(None, Some(PathBuf::from("/docs/Quantick"))),
            Path::new("/docs/Quantick").join("replay"),
        );
    }

    /// The old empty-string field must not resolve to the working directory:
    /// that is how a download landed beside the executable and vanished from
    /// the list that was supposed to show it.
    #[test]
    fn a_blank_stored_pick_is_no_pick() {
        assert_eq!(
            chosen(Some("   "), Some(PathBuf::from("/docs/Quantick"))),
            Path::new("/docs/Quantick").join("replay"),
        );
    }

    #[test]
    fn no_documents_folder_falls_back_to_the_relative_folder() {
        assert_eq!(chosen(None, None), PathBuf::from("replay"));
    }

    /// The hook is for one run. It decides where *this* run looks and nothing
    /// else — a QA run must not be able to redefine the trader's folder.
    #[test]
    fn the_hook_wins_this_run_without_touching_the_stored_pick() {
        assert_eq!(
            resolve_with(
                Some("E:/qa"),
                Some("D:/tape"),
                Some(PathBuf::from("/docs/Quantick"))
            ),
            "E:/qa",
        );
    }

    /// An empty hook is not a hook. `QUANTICK_REPLAY_DIR=` used to resolve to
    /// the working directory, which is how a download landed beside the
    /// executable.
    #[test]
    fn an_empty_hook_falls_through_to_the_pick() {
        assert_eq!(
            resolve_with(
                Some("  "),
                Some("D:/tape"),
                Some(PathBuf::from("/docs/Quantick"))
            ),
            "D:/tape",
        );
    }
}
