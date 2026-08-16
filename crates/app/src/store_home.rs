//! Where the cockpit's stores live, and the one-time rescue of the
//! cwd-relative era.
//!
//! Every store that remembers a piece of the trader's cockpit — the tabs and
//! the chrome around them, which indicators are on and how they are tuned,
//! the chart layers, the drawing colours, the footprint, the symbols added by
//! hand — used to resolve its file against the *working directory*:
//! `PathBuf::from("ui-state.toml")`. Launching quantick from a different
//! folder therefore opened a different cockpit, and every one of them was
//! empty. Nothing was ever deleted; the arrangement was simply somewhere the
//! next launch did not look. That is the "it doesn't remember anything
//! anymore" report, and it is the same bug [`crate::paper_home`] already
//! named and fixed for the paper-trading journal — the cockpit was left
//! behind.
//!
//! The durable home is the shelf that module already owns
//! (`Documents/Quantick`), so one folder holds everything quantick keeps for
//! a trader. Resolution keeps the spirit an explicit ask always wins:
//! `QUANTICK_*` for this store (one run) > the durable home > the
//! cwd-relative name, which survives as the honest answer when the platform
//! reports no documents folder (headless CI, bare setups). Inventing a home
//! the user cannot find would be worse than the old behaviour.
//!
//! Exactly once — recorded by a marker in the home itself, so the
//! one-time-ness holds from every launch directory — startup also
//! *consolidates*: copy, never move, never delete, whatever the launch
//! directory still holds into a home file that does not exist yet. A home
//! file that does exist is never overwritten: after the first rescue the home
//! is the truth, and a stale copy in some old checkout must not be able to
//! reach back and replace it.

use std::path::{Path, PathBuf};

/// One store that remembers part of the cockpit.
///
/// The registry below is the port: a ninth store is one entry here plus the
/// one line in its own module that calls [`resolve`]. Nothing else in the
/// app learns a new name — the consolidation sweep and the workspace bundle
/// both read this list rather than keeping their own.
pub(crate) struct CockpitStore {
    /// The section this store occupies in a workspace bundle, and the label
    /// a log line uses. Stable across renames of the file itself.
    pub key: &'static str,
    /// Environment override for this store's location — an explicit ask,
    /// honoured for one run and never consolidated under.
    pub env: &'static str,
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
    /// written — see [`crate::workspace_bundle`].
    pub validate: fn(&str) -> Result<(), String>,
}

/// Every store that holds a piece of the cockpit, in the order a bundle
/// writes them.
///
/// Paper trading is deliberately absent: [`crate::paper_home`] already gave
/// the journal a durable home of its own, and a simulated position is a
/// *result*, not an arrangement of the screen — restoring a saved cockpit
/// must not rewrite the trader's account.
pub(crate) const COCKPIT_STORES: &[CockpitStore] = &[
    CockpitStore {
        key: "ui_state",
        env: crate::ui_state::UI_STATE_ENV,
        file: crate::ui_state::UI_STATE_FILE,
        validate: crate::ui_state::validate,
        path: crate::ui_state::default_path,
    },
    CockpitStore {
        key: "indicators",
        env: crate::indicators::state_file::STATE_ENV,
        file: crate::indicators::state_file::STATE_FILE,
        validate: crate::indicators::state_file::validate,
        path: crate::indicators::state_file::default_path,
    },
    CockpitStore {
        key: "indicator_presets",
        env: crate::indicators::preset_file::PRESETS_ENV,
        file: crate::indicators::preset_file::PRESETS_FILE,
        validate: crate::indicators::preset_file::validate,
        path: crate::indicators::preset_file::default_path,
    },
    CockpitStore {
        key: "chart_layers",
        env: crate::chart_layers::LAYERS_ENV,
        file: crate::chart_layers::LAYERS_FILE,
        validate: crate::chart_layers::validate,
        path: crate::chart_layers::default_path,
    },
    CockpitStore {
        key: "drawing_presets",
        env: crate::drawings::presets::PRESETS_ENV,
        file: crate::drawings::presets::PRESETS_FILE,
        validate: crate::drawings::presets::validate,
        path: crate::drawings::presets::PresetStore::default_path,
    },
    CockpitStore {
        key: "footprint_settings",
        env: crate::footprint_config::SETTINGS_ENV,
        file: crate::footprint_config::SETTINGS_FILE,
        validate: crate::footprint_config::validate_settings,
        path: crate::footprint_config::settings_path,
    },
    CockpitStore {
        key: "footprint_presets",
        env: crate::footprint_presets::PRESETS_ENV,
        file: crate::footprint_presets::PRESETS_FILE,
        validate: crate::footprint_presets::validate,
        path: crate::footprint_presets::default_path,
    },
    CockpitStore {
        key: "symbols",
        env: crate::symbols_file::SYMBOLS_ENV,
        file: crate::symbols_file::SYMBOLS_FILE,
        validate: crate::symbols_file::validate,
        path: crate::symbols_file::default_path,
    },
];

/// The file that marks the home as already consolidated. It lives in the home
/// itself so the one-time rescue stays one-time from every launch directory —
/// a flag written beside the launch directory would not.
const CONSOLIDATED_MARKER: &str = ".cockpit-consolidated";

/// The durable home for cockpit stores, when the platform reports a documents
/// folder. The same shelf the journal hangs off, by design.
pub(crate) fn home() -> Option<PathBuf> {
    crate::paper_home::shelf_dir()
}

/// Where a store's file lives this run.
///
/// The whole resolution order in one place: the store's own environment
/// override, then the durable home, then the cwd-relative name the app used
/// before this module existed.
pub(crate) fn resolve(env: &str, file: &str) -> PathBuf {
    if let Some(explicit) = std::env::var_os(env) {
        return PathBuf::from(explicit);
    }
    resolve_in(home(), file)
}

/// [`resolve`] with its home injected, so the decision is testable without a
/// real documents folder.
fn resolve_in(home: Option<PathBuf>, file: &str) -> PathBuf {
    home.map_or_else(|| PathBuf::from(file), |home| home.join(file))
}

/// A per-process scratch home for stores built by a test.
///
/// Stable within a process and distinct per file, so a `default_path()` a
/// test calls twice answers twice the same — and no test can reach the real
/// documents folder.
pub(crate) fn test_path(file: &str) -> PathBuf {
    // Per thread as well as per process: the test harness gives each test its
    // own thread, so this is stable *within* a test — a path resolved twice
    // answers twice the same — while still isolating tests that build several
    // apps and would otherwise restore one another's cockpit.
    let home = std::env::temp_dir().join(format!(
        "quantick-test-home-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    // The real home is created by the startup rescue, which a test does not
    // run; without this every store's first save would fail on a missing
    // folder rather than on anything the test is about.
    let _ = std::fs::create_dir_all(&home);
    home.join(file)
}

/// What one consolidation pass did. Copies only — the launch directory keeps
/// every file, which is what makes running the pass again safe.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct RescueSummary {
    /// Files copied into the home because it did not have them yet.
    pub copied: usize,
    /// Files the home already had — left exactly as they are.
    pub kept: usize,
    /// Copies that failed with an I/O error (logged, source left in place).
    pub failed: usize,
}

impl RescueSummary {
    /// Whether this pass may stamp the home as consolidated. A pass that
    /// failed must run again rather than declare the rescue done.
    const fn clean(&self) -> bool {
        self.failed == 0
    }
}

/// Rescue the cockpit from the launch directory into the durable home, once.
///
/// Called before the app reads any store, so the first launch after this
/// change opens on the arrangement the trader last had rather than on an
/// empty screen. Returns `None` when there is nothing to do: no documents
/// folder, or the rescue already ran.
pub(crate) fn consolidate_once() -> Option<RescueSummary> {
    if cfg!(test) {
        // A test must never scan, copy into, or stamp a real documents
        // folder — the same scratch discipline `paper_home::startup_home`
        // applies.
        return None;
    }
    let home = home()?;
    rescue_into(&home, Path::new("."), &|env| {
        std::env::var_os(env).is_some()
    })
}

/// [`consolidate_once`] with its home, legacy directory and the
/// "is this store overridden?" question injected — so the whole decision tree
/// is testable against scratch folders without a test having to set a process
/// -wide environment variable that its neighbours would see.
fn rescue_into(
    home: &Path,
    legacy: &Path,
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
    let mut summary = RescueSummary::default();
    for store in COCKPIT_STORES {
        // A store pointed somewhere by its own environment variable is not
        // part of this installation's cockpit — a QA or autostart run must
        // not have its scratch file copied into the trader's home.
        if overridden(store.env) {
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
    if summary.clean() {
        write_marker(&marker);
    }
    Some(summary)
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

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "quantick-store-home-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch");
        dir
    }

    /// The whole point of the module: the answer does not depend on where the
    /// app was launched from.
    #[test]
    fn a_store_resolves_into_the_home_not_the_launch_directory() {
        let home = scratch("resolve");
        assert_eq!(
            resolve_in(Some(home.clone()), "ui-state.toml"),
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

    /// Every QA hook and autostart run leans on this: an explicit ask wins,
    /// so validation never reads or writes the trader's real cockpit.
    ///
    /// The one test here that touches the process environment, under a name
    /// no other store or test reads, so a parallel neighbour cannot see it.
    #[test]
    fn an_explicit_environment_ask_beats_the_home() {
        let key = "QUANTICK_TEST_STORE_HOME_ENV";
        // SAFETY: the name is unique to this test, so no concurrently
        // running test reads it; it is removed again before returning.
        unsafe { std::env::set_var(key, "D:/somewhere/else.toml") };
        let resolved = resolve(key, "ui-state.toml");
        unsafe { std::env::remove_var(key) };
        assert_eq!(resolved, PathBuf::from("D:/somewhere/else.toml"));
    }

    #[test]
    fn the_rescue_copies_the_launch_directory_into_an_empty_home() {
        let home = scratch("rescue-home");
        let legacy = scratch("rescue-legacy");
        std::fs::write(legacy.join("ui-state.toml"), "version = 1\ntabs = []\n").unwrap();
        std::fs::write(legacy.join("chart-layers.toml"), "version = 1\n").unwrap();
        let summary = rescue_into(&home, &legacy, &nothing_overridden).expect("the rescue runs");
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
            rescue_into(&home, &legacy, &nothing_overridden).is_some(),
            "the first pass runs"
        );
        assert!(
            rescue_into(&home, Path::new("some/other/checkout"), &nothing_overridden).is_none(),
            "a second launch from anywhere else finds the marker"
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
        let summary = rescue_into(&home, &legacy, &nothing_overridden).expect("the rescue runs");
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
        let first = rescue_into(&home, &legacy, &nothing_overridden).expect("first");
        std::fs::remove_file(home.join(CONSOLIDATED_MARKER)).unwrap();
        let second = rescue_into(&home, &legacy, &nothing_overridden).expect("second");
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
        let summary = rescue_into(&home, &legacy, &|env| env == crate::ui_state::UI_STATE_ENV)
            .expect("the rescue runs");
        assert_eq!(summary.copied, 0, "the overridden store is skipped");
        assert!(!home.join("ui-state.toml").exists());
    }

    /// Two stores may never share a file or a key: the bundle keys sections
    /// by one and the rescue copies by the other.
    #[test]
    fn every_store_is_named_once() {
        let mut keys: Vec<_> = COCKPIT_STORES.iter().map(|store| store.key).collect();
        let mut files: Vec<_> = COCKPIT_STORES.iter().map(|store| store.file).collect();
        let (before_keys, before_files) = (keys.len(), files.len());
        keys.sort_unstable();
        keys.dedup();
        files.sort_unstable();
        files.dedup();
        assert_eq!(keys.len(), before_keys, "two stores share a bundle key");
        assert_eq!(files.len(), before_files, "two stores share a file name");
    }
}
