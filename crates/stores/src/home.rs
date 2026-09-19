//! Where the cockpit's stores live, and the one-time rescue of the
//! cwd-relative era: the store row a window registers, the resolution of a
//! store's file inside a home, and the copy pass that brings a launch
//! directory's files into the durable home once.
//!
//! The window supplies the home, the launch directory, its store table and
//! the "is this store overridden?" question; nothing here reads the
//! environment or asks the platform for a folder.

use std::path::{Path, PathBuf};

/// One store that remembers part of the cockpit.
///
/// The registry below is the port: a ninth store is one entry here plus the
/// one line in its own module that resolves its file. Nothing else in the
/// app learns a new name — the consolidation sweep and the workspace bundle
/// both read this list rather than keeping their own.
pub struct CockpitStore {
    /// The section this store occupies in a workspace bundle, and the label
    /// a log line uses. Stable across renames of the file itself.
    pub key: &'static str,
    /// The file's name, in the durable home and in the legacy launch
    /// directory alike. One name, so the rescue is a copy and not a mapping.
    pub file: &'static str,
    /// Where this store's file actually is for this run — the module's own
    /// `default_path`, not a second copy of its resolution.
    ///
    /// A field rather than `resolve(env, file)` because each module decides
    /// its own answer, and under test that answer is a scratch file. Reading
    /// the trader's real cockpit from a test — or worse, writing it — is the
    /// failure this closes.
    pub path: fn() -> PathBuf,
    /// Parse this store's file with its real type, reporting why it is not
    /// one. The gate that lets a bundle be checked whole before any of it is
    /// written — see the workspace bundle.
    pub validate: fn(&str) -> Result<(), String>,
    /// Whether this store travels in a workspace bundle.
    ///
    /// Every store here shares the durable home, because losing any of them
    /// to a launch directory is the same bug. Not every one is part of an
    /// *arrangement* a trader would hand to someone else: the paper sidecar
    /// records a simulated account and the folder its journal lives in, which
    /// are results and machine facts, not a screen.
    pub in_bundle: bool,
    /// Top-level keys that describe *this installation* rather than the
    /// arrangement, and so never travel in a bundle.
    ///
    /// A workspace file carries a cockpit, not a machine. Without this, a
    /// bundle from a colleague would overwrite the recent-files list, the
    /// named bookmarks and the replay folder of whoever opened it — the very
    /// thing those fields' own doc comments say must not happen
    /// (the workspace's recent list). They are stripped
    /// on capture and preserved on apply.
    pub local_keys: &'static [&'static str],
}

impl quantick_workspace::bundle::BundleStore for CockpitStore {
    fn key(&self) -> &str {
        self.key
    }
    fn included(&self) -> bool {
        self.in_bundle
    }
    fn local_keys(&self) -> &[&str] {
        self.local_keys
    }
    fn validate_text(&self, text: &str) -> Result<(), quantick_workspace::bundle::SectionError> {
        (self.validate)(text).map_err(quantick_workspace::bundle::SectionError::Malformed)
    }
}

/// The file that marks the home as already consolidated. It lives in the home
/// itself so the one-time rescue stays one-time from every launch directory —
/// a flag written beside the launch directory would not.
pub const CONSOLIDATED_MARKER: &str = ".cockpit-consolidated";

/// The window's `resolve` with its home injected, so the decision is testable without a
/// real documents folder.
pub fn resolve_in(home: Option<PathBuf>, file: &str) -> PathBuf {
    home.map_or_else(|| PathBuf::from(file), |home| home.join(file))
}

/// What one consolidation pass did. Copies only — the launch directory keeps
/// every file, which is what makes running the pass again safe.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct RescueSummary {
    /// Files copied into the home because it did not have them yet.
    pub copied: usize,
    /// Files the home already had — left exactly as they are.
    pub kept: usize,
    /// Copies that failed with an I/O error (logged, source left in place).
    pub failed: usize,
}

impl RescueSummary {
    /// Whether this pass may stamp the home as consolidated.
    ///
    /// A pass that copied nothing may *not*: it saw one launch directory, and
    /// the trader's real cockpit is very likely in a different one. Stamping
    /// on an empty pass is how the rescue would disable itself before ever
    /// reaching the folder that mattered — a trader who happens to open the
    /// app once from a desktop shortcut would lose their arrangement
    /// permanently, which is the exact failure this module exists to end.
    /// A pass that failed may not either: it must run again rather than
    /// declare the rescue done.
    ///
    /// The cost of not stamping is one `exists` per store on later launches
    /// (microseconds), against losing a cockpit — so the bar is deliberately
    /// this high.
    pub const fn rescued_something(&self) -> bool {
        self.copied > 0 && self.failed == 0
    }
}

/// The window's `consolidate_once` with its home, legacy directory and the
/// "is this store overridden?" question injected — so the whole decision tree
/// is testable against scratch folders without a test having to set a process
/// -wide environment variable that its neighbours would see.
pub fn rescue_into(
    home: &Path,
    legacy: &Path,
    stores: &[CockpitStore],
    overridden: &dyn Fn(&str) -> bool,
) -> Option<RescueSummary> {
    // Before the marker check, not after: a trader who deleted the folder
    // still has a marker-less home *and* one that no store could write to,
    // and the cheap `create_dir_all` on an existing folder is the price of
    // the app never silently failing to save again.
    if let Err(error) = std::fs::create_dir_all(home) {
        tracing::warn!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "COCKPIT_HOME_UNAVAILABLE",
            path = %home.display(),
            %error,
            action = "keeping_launch_directory",
            "could not create the cockpit home"
        );
        return None;
    }
    let marker = home.join(CONSOLIDATED_MARKER);
    if marker.exists() {
        return None;
    }
    // A pass that copies nothing must not reach the marker: see
    // `RescueSummary::rescued_something`.
    let mut summary = RescueSummary::default();
    for store in stores {
        // A store pointed somewhere by its own environment variable is not
        // part of this installation's cockpit — a QA or autostart run must
        // not have its scratch file copied into the trader's home.
        if overridden(store.file) {
            continue;
        }
        let source = legacy.join(store.file);
        let dest = home.join(store.file);
        if dest.exists() {
            // The home is the truth once it has an answer. A stale copy left
            // in some old checkout must never reach back and replace it.
            summary.kept += 1;
            continue;
        }
        if !source.exists() {
            continue;
        }
        match std::fs::copy(&source, &dest) {
            Ok(_) => {
                summary.copied += 1;
                tracing::info!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "COCKPIT_RESCUED",
                    store = store.key,
                    from = %source.display(),
                    to = %dest.display(),
                    action = "copied_to_home",
                    "brought a cockpit store into the durable home"
                );
            }
            Err(error) => {
                summary.failed += 1;
                tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "COCKPIT_RESCUE_FAILED",
                    store = store.key,
                    from = %source.display(),
                    %error,
                    action = "left_in_place",
                    "could not copy a cockpit store into the durable home"
                );
            }
        }
    }
    if summary.rescued_something() {
        write_marker(&marker);
    }
    Some(summary)
}

/// What the trader is told after a rescue, or `None` when nothing moved.
///
/// A silent rescue would look like the app relocated their settings behind
/// their back — and worse, they would not know the folder to back up. Says
/// "copies" out loud and names the folder, the way
/// the paper home's import toast does for the journal: one app, one
/// way of reporting a one-time import.
pub fn rescue_toast(summary: &RescueSummary, home: &Path) -> Option<String> {
    if summary.copied == 0 {
        return None;
    }
    Some(format!(
        "Your saved settings now live in {} — {} file(s) copied there, originals untouched",
        home.display(),
        summary.copied
    ))
}

/// Stamp the home as consolidated. A failed write only means the rescue
/// re-runs next launch — it copies nothing it already copied, so that is
/// safe.
fn write_marker(marker: &Path) {
    if let Err(error) = std::fs::write(
        marker,
        "quantick wrote this after bringing the cockpit stores it found in a launch \
         directory into this folder; deleting it re-runs that one-time import.\n",
    ) {
        tracing::warn!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "COCKPIT_MARKER_FAILED",
            path = %marker.display(),
            %error,
            action = "rescue_reruns_next_launch",
            "could not stamp the cockpit home as consolidated"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> crate::scratch::ScratchDir {
        crate::scratch::ScratchDir::new(name)
    }

    fn no_path() -> PathBuf {
        PathBuf::new()
    }

    fn any_text(_: &str) -> Result<(), String> {
        Ok(())
    }

    /// Two stores with the window's own file names, so the mechanism is
    /// proven here and the window's table only has to be well formed.
    const STORES: &[CockpitStore] = &[
        CockpitStore {
            key: "ui_state",
            file: "ui-state.toml",
            path: no_path,
            validate: any_text,
            in_bundle: true,
            local_keys: &[],
        },
        CockpitStore {
            key: "chart_layers",
            file: "chart-layers.toml",
            path: no_path,
            validate: any_text,
            in_bundle: true,
            local_keys: &[],
        },
    ];

    /// The whole point of the module: the answer does not depend on where the
    /// app was launched from.
    #[test]
    fn a_store_resolves_into_the_home_not_the_launch_directory() {
        let home = scratch("resolve");
        assert_eq!(
            resolve_in(Some(home.path().to_path_buf()), "ui-state.toml"),
            home.join("ui-state.toml")
        );
    }

    /// Honest, rather than inventing a folder the user cannot find.
    #[test]
    fn without_a_documents_folder_the_old_relative_name_stands() {
        assert_eq!(
            resolve_in(None, "ui-state.toml"),
            PathBuf::from("ui-state.toml")
        );
    }

    /// Nothing is overridden — the plain case for every rescue test below.
    fn nothing_overridden(_env: &str) -> bool {
        false
    }

    #[test]
    fn the_rescue_copies_the_launch_directory_into_an_empty_home() {
        let home = scratch("rescue-home");
        let legacy = scratch("rescue-legacy");
        std::fs::write(legacy.join("ui-state.toml"), "version = 1\ntabs = []\n").unwrap();
        std::fs::write(legacy.join("chart-layers.toml"), "version = 1\n").unwrap();
        let summary =
            rescue_into(&home, &legacy, STORES, &nothing_overridden).expect("the rescue runs");
        assert_eq!(summary.copied, 2, "both stores reach the home");
        assert_eq!(summary.failed, 0);
        assert!(home.join("ui-state.toml").exists());
        assert!(
            legacy.join("ui-state.toml").exists(),
            "a rescue copies and never moves"
        );
    }

    /// The one-time-ness lives in the home, so it holds from every launch
    /// directory rather than from the one that happened to run first.
    #[test]
    fn the_rescue_runs_once_however_the_app_is_launched() {
        let home = scratch("rescue-once-home");
        let legacy = scratch("rescue-once-legacy");
        std::fs::write(legacy.join("ui-state.toml"), "version = 1\ntabs = []\n").unwrap();
        assert!(
            rescue_into(&home, &legacy, STORES, &nothing_overridden).is_some(),
            "the first pass runs"
        );
        assert!(
            rescue_into(
                &home,
                Path::new("some/other/checkout"),
                STORES,
                &nothing_overridden
            )
            .is_none(),
            "a second launch from anywhere else finds the marker"
        );
    }

    /// The failure that would make this module worse than the bug it fixes.
    ///
    /// A trader whose cockpit lives in one checkout opens the app once from
    /// somewhere else — a desktop shortcut, another worktree. That pass finds
    /// nothing. If it stamped the home anyway, the rescue would be over
    /// before it ever saw the folder that mattered, and the arrangement would
    /// be lost for good.
    #[test]
    fn a_pass_that_found_nothing_does_not_end_the_rescue() {
        let home = scratch("rescue-empty-home");
        let empty = scratch("rescue-empty-launch");
        let real = scratch("rescue-real-cockpit");
        std::fs::write(real.join("ui-state.toml"), "version = 1\ntabs = []\n").unwrap();

        let first = rescue_into(&home, &empty, STORES, &nothing_overridden).expect("the pass runs");
        assert_eq!(first.copied, 0, "that folder held no cockpit");
        assert!(
            !home.join(CONSOLIDATED_MARKER).exists(),
            "and an empty pass must not declare the rescue done"
        );

        let second =
            rescue_into(&home, &real, STORES, &nothing_overridden).expect("the next launch tries");
        assert_eq!(second.copied, 1, "the real cockpit is still rescued");
        assert!(
            home.join(CONSOLIDATED_MARKER).exists(),
            "and now the rescue is over"
        );
    }

    /// A failed copy must not end the rescue either.
    #[test]
    fn a_pass_that_failed_does_not_end_the_rescue() {
        let home = scratch("rescue-failed-home");
        let legacy = scratch("rescue-failed-legacy");
        std::fs::write(legacy.join("ui-state.toml"), "version = 1\ntabs = []\n").unwrap();
        // A directory where the copy expects to write a file: the rename
        // fails, which is the shape of any I/O refusal here.
        std::fs::create_dir_all(home.join("ui-state.toml")).unwrap();
        let summary =
            rescue_into(&home, &legacy, STORES, &nothing_overridden).expect("the pass runs");
        assert_eq!(summary.copied, 0);
        assert!(
            !home.join(CONSOLIDATED_MARKER).exists(),
            "a pass that could not finish runs again next launch"
        );
    }

    /// After the first rescue the home is the truth. A cockpit left in an old
    /// checkout must not be able to reach back and overwrite it.
    #[test]
    fn a_home_that_already_has_the_store_is_never_overwritten() {
        let home = scratch("rescue-keep-home");
        let legacy = scratch("rescue-keep-legacy");
        std::fs::write(
            home.join("ui-state.toml"),
            "version = 1\n# the home's own\n",
        )
        .unwrap();
        std::fs::write(
            legacy.join("ui-state.toml"),
            "version = 1\n# the stale one\n",
        )
        .unwrap();
        let summary =
            rescue_into(&home, &legacy, STORES, &nothing_overridden).expect("the rescue runs");
        assert_eq!(summary.kept, 1);
        assert_eq!(summary.copied, 0);
        assert!(
            std::fs::read_to_string(home.join("ui-state.toml"))
                .unwrap()
                .contains("the home's own"),
            "the home keeps its answer"
        );
    }

    /// Running the pass again after a partial failure has to be safe, which
    /// is exactly what copy-never-move plus never-overwrite buys.
    #[test]
    fn a_second_pass_after_a_cleared_marker_changes_nothing() {
        let home = scratch("rescue-idempotent-home");
        let legacy = scratch("rescue-idempotent-legacy");
        std::fs::write(legacy.join("ui-state.toml"), "version = 1\ntabs = []\n").unwrap();
        let first = rescue_into(&home, &legacy, STORES, &nothing_overridden).expect("first");
        std::fs::remove_file(home.join(CONSOLIDATED_MARKER)).unwrap();
        let second = rescue_into(&home, &legacy, STORES, &nothing_overridden).expect("second");
        assert_eq!(first.copied, 1);
        assert_eq!(second.copied, 0, "nothing is copied twice");
        assert_eq!(second.kept, 1);
    }

    /// A scratch file belonging to a QA run is not this installation's
    /// cockpit and must not be copied into the trader's home.
    #[test]
    fn a_store_under_an_environment_override_is_left_out_of_the_rescue() {
        let home = scratch("rescue-env-home");
        let legacy = scratch("rescue-env-legacy");
        std::fs::write(legacy.join("ui-state.toml"), "version = 1\ntabs = []\n").unwrap();
        let summary = rescue_into(&home, &legacy, STORES, &|file| file == "ui-state.toml")
            .expect("the rescue runs");
        assert_eq!(summary.copied, 0, "the overridden store is skipped");
        assert!(!home.join("ui-state.toml").exists());
    }
}
