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
//!
//! # Why the documents folder is not always the answer
//!
//! One session day of B3 tape is ~60 MB and a trader keeps months of them.
//! On a machine where the documents folder is inside OneDrive — which is the
//! Windows default once the account is signed in — that puts gigabytes of
//! recordings into a sync client: an upload per download, cloud quota spent on
//! data the venue can re-serve, and, once Files On-Demand tiers an old file
//! out, a replay that stalls fetching its own tape back. So tape lands beside
//! the documents shelf rather than inside it, under the user's own home
//! folder, which is just as easy to find and belongs to this machine. Small
//! things quantick keeps — the journal, the broker clock — stay in documents,
//! where syncing them is a feature.

use std::path::PathBuf;

/// Overrides the replay folder for one run — the autostart family's explicit
/// ask, and the hook the validation harness drives.
pub(crate) const REPLAY_DIR_ENV: &str = "QUANTICK_REPLAY_DIR";
/// The environment variable OneDrive sets to its own root on Windows. Present
/// only when the sync client is actually set up for this account, which is the
/// question being asked — the folder's *name* is not evidence of anything.
const SYNCED_ROOT_ENV: &str = "OneDrive";

/// The folder under quantick's documents shelf. The shelf itself is named once,
/// in [`crate::paper_home`], so everything quantick keeps for a trader is under
/// one roof and renaming that roof stays one edit.
const REPLAY_DIR: &str = "replay";

/// The default home given a shelf — and a cwd-relative `replay` when the
/// platform reports no folder to hang one off (headless CI, bare setups).
///
/// Inventing a path the trader cannot find would be worse than the honest
/// relative one, which is the same call [`crate::paper_home`] makes.
pub(crate) fn default_dir(shelf: Option<PathBuf>) -> PathBuf {
    shelf.map_or_else(|| PathBuf::from(REPLAY_DIR), |shelf| shelf.join(REPLAY_DIR))
}

/// The shelf recordings hang off: the documents one, unless it sits inside a
/// synced folder — then the user's home folder, which does not sync.
///
/// See the module docs for why gigabytes of tape must not land in OneDrive.
/// When the documents folder is synced but the platform reports no home
/// folder, documents still wins: a real path the trader can open beats a
/// guess, and the sync client is a slow problem rather than a broken one.
fn tape_shelf(
    documents: Option<PathBuf>,
    synced_root: Option<PathBuf>,
    home: Option<PathBuf>,
) -> Option<PathBuf> {
    let documents = documents?;
    let synced = synced_root.is_some_and(|root| documents.starts_with(root));
    let base = if synced {
        home.or(Some(documents))
    } else {
        Some(documents)
    };
    base.map(|base| base.join(crate::paper_home::DOCUMENTS_SHELF))
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
        tape_shelf(
            crate::paper_home::documents_dir(),
            // The sync client states its own root; quantick does not go
            // looking for one by name, which would call any folder with
            // "OneDrive" in its path synced.
            std::env::var_os(SYNCED_ROOT_ENV).map(PathBuf::from),
            dirs::home_dir(),
        ),
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

    /// Tape is big and keeps coming. A synced documents folder would upload
    /// every download and can tier an old session out to the cloud, which is a
    /// replay that stalls fetching its own tape back.
    #[test]
    fn a_synced_documents_folder_sends_tape_to_the_home_folder_instead() {
        assert_eq!(
            tape_shelf(
                Some(PathBuf::from("/users/me/OneDrive/Documents")),
                Some(PathBuf::from("/users/me/OneDrive")),
                Some(PathBuf::from("/users/me")),
            ),
            Some(PathBuf::from("/users/me/Quantick")),
        );
    }

    /// An unsynced documents folder is exactly where a trader looks first.
    #[test]
    fn an_unsynced_documents_folder_keeps_the_recordings() {
        assert_eq!(
            tape_shelf(
                Some(PathBuf::from("/users/me/Documents")),
                Some(PathBuf::from("/users/me/OneDrive")),
                Some(PathBuf::from("/users/me")),
            ),
            Some(PathBuf::from("/users/me/Documents/Quantick")),
        );
    }

    /// No sync client at all: nothing to avoid.
    #[test]
    fn no_sync_client_keeps_the_documents_folder() {
        assert_eq!(
            tape_shelf(Some(PathBuf::from("/users/me/Documents")), None, None),
            Some(PathBuf::from("/users/me/Documents/Quantick")),
        );
    }

    /// Synced documents and no home folder to fall back to: a real path the
    /// trader can open beats a guess.
    #[test]
    fn a_synced_folder_with_nowhere_else_to_go_still_answers() {
        assert_eq!(
            tape_shelf(
                Some(PathBuf::from("/users/me/OneDrive/Documents")),
                Some(PathBuf::from("/users/me/OneDrive")),
                None,
            ),
            Some(PathBuf::from("/users/me/OneDrive/Documents/Quantick")),
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
