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
    /// ([`crate::ui_state::Workspace::recent_workspaces`]). They are stripped
    /// on capture and preserved on apply.
    pub local_keys: &'static [&'static str],
}

/// Every store that keeps something between launches, in the order a bundle
/// writes them.
///
/// Two questions, deliberately separate. *Does it belong in the durable
/// home?* — yes for all of these, because losing any of them to a launch
/// directory is the one bug this module exists to end. *Does it travel in a
/// workspace bundle?* — [`CockpitStore::in_bundle`], and the paper sidecar
/// answers no: it records a simulated account and where its journal lives,
/// which are a result and a machine fact, not an arrangement of the screen.
/// The journal *folder* was already rescued by [`crate::paper_home`]; the
/// file recording which folder the trader picked was not, and that is why
/// `paper_state` is here.
pub(crate) const COCKPIT_STORES: &[CockpitStore] = &[
    CockpitStore {
        key: "ui_state",
        env: crate::ui_state::UI_STATE_ENV,
        file: crate::ui_state::UI_STATE_FILE,
        validate: crate::ui_state::validate,
        path: crate::ui_state::default_path,
        in_bundle: true,
        local_keys: crate::ui_state::LOCAL_KEYS,
    },
    CockpitStore {
        key: "indicators",
        env: crate::indicators::state_file::STATE_ENV,
        file: crate::indicators::state_file::STATE_FILE,
        validate: crate::indicators::state_file::validate,
        path: crate::indicators::state_file::default_path,
        in_bundle: true,
        local_keys: &[],
    },
    CockpitStore {
        key: "layouts",
        env: crate::layouts::LAYOUTS_ENV,
        file: crate::layouts::LAYOUTS_FILE,
        validate: crate::layouts::validate,
        path: crate::layouts::default_path,
        in_bundle: true,
        local_keys: &[],
    },
    CockpitStore {
        key: "indicator_presets",
        env: crate::indicators::preset_file::PRESETS_ENV,
        file: crate::indicators::preset_file::PRESETS_FILE,
        validate: crate::indicators::preset_file::validate,
        path: crate::indicators::preset_file::default_path,
        in_bundle: true,
        local_keys: &[],
    },
    CockpitStore {
        key: "chart_layers",
        env: crate::chart_layers::LAYERS_ENV,
        file: crate::chart_layers::LAYERS_FILE,
        validate: crate::chart_layers::validate,
        path: crate::chart_layers::default_path,
        in_bundle: true,
        local_keys: &[],
    },
    CockpitStore {
        key: "drawing_presets",
        env: crate::drawings::presets::PRESETS_ENV,
        file: crate::drawings::presets::PRESETS_FILE,
        validate: crate::drawings::presets::validate,
        path: crate::drawings::presets::PresetStore::default_path,
        in_bundle: true,
        local_keys: &[],
    },
    CockpitStore {
        key: "footprint_settings",
        env: crate::footprint_config::SETTINGS_ENV,
        file: crate::footprint_config::SETTINGS_FILE,
        validate: crate::footprint_config::validate_settings,
        path: crate::footprint_config::settings_path,
        in_bundle: true,
        local_keys: &[],
    },
    CockpitStore {
        key: "footprint_presets",
        env: crate::footprint_presets::PRESETS_ENV,
        file: crate::footprint_presets::PRESETS_FILE,
        validate: crate::footprint_presets::validate,
        path: crate::footprint_presets::default_path,
        in_bundle: true,
        local_keys: &[],
    },
    CockpitStore {
        key: "paper_state",
        env: crate::paper_state::STATE_ENV,
        file: crate::paper_state::STATE_FILE,
        validate: crate::paper_state::validate,
        path: crate::paper_state::default_path,
        in_bundle: false,
        local_keys: &[],
    },
    CockpitStore {
        key: "symbols",
        env: crate::symbols_file::SYMBOLS_ENV,
        file: crate::symbols_file::SYMBOLS_FILE,
        validate: crate::symbols_file::validate,
        path: crate::symbols_file::default_path,
        in_bundle: true,
        local_keys: &[],
    },
];

/// The file that marks the home as already consolidated. It lives in the home
/// itself so the one-time rescue stays one-time from every launch directory —
/// a flag written beside the launch directory would not.
const CONSOLIDATED_MARKER: &str = ".cockpit-consolidated";

/// The durable home for cockpit stores: the shelf the journal hangs off, but
/// only once it is a folder that can actually be written to.
///
/// Resolved once per process. A path the platform names but the app cannot
/// create — an unmounted roaming profile, a full disk, a permissions
/// problem — is *not* a home: returning it anyway would send every store to a
/// folder that does not exist, and the trader would get "could not be saved"
/// on every save for the rest of the session while the log claimed the launch
/// directory had been kept. Falling back to the cwd-relative name is the old
/// behaviour, which at least works.
///
/// Caching also pays for itself: `dirs::document_dir` is a `SHGetKnownFolderPath`
/// call on Windows and eight stores ask for it at startup.
pub(crate) fn home() -> Option<PathBuf> {
    static HOME: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    HOME.get_or_init(|| {
        let shelf = crate::paper_home::shelf_dir()?;
        match std::fs::create_dir_all(&shelf) {
            Ok(()) => Some(shelf),
            Err(error) => {
                tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "COCKPIT_HOME_UNAVAILABLE",
                    path = %shelf.display(),
                    %error,
                    action = "keeping_launch_directory",
                    "the documents folder cannot hold the cockpit; falling back to the launch \
                     directory"
                );
                None
            }
        }
    })
    .clone()
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

thread_local! {
    /// Which scratch home this thread's stores resolve to. See [`test_path`].
    static TEST_HOME_EPOCH: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Give the app a test is about to build a scratch home of its own.
///
/// Called by the test harness's app constructor. Without it, two apps built
/// on one thread would share a cockpit and the second would open on the
/// first's arrangement — an order-dependent failure that passes in parallel
/// CI and appears the moment someone serializes the run to debug something
/// else.
#[cfg(test)]
pub(crate) fn next_test_home() {
    TEST_HOME_EPOCH.with(|epoch| epoch.set(epoch.get() + 1));
}

/// A scratch home for stores built by a test, per thread and per epoch.
///
/// Stable within a process and distinct per file, so a `default_path()` a
/// test calls twice answers twice the same — and no test can reach the real
/// documents folder.
pub(crate) fn test_path(file: &str) -> PathBuf {
    // Two requirements pull against each other. A path resolved twice inside
    // one test must answer twice the same, or the bundle would write to a
    // different file than the app is reading. And two tests must never share
    // a cockpit, or one restores the other's — which the per-call counters
    // this replaced did give, and `--test-threads=1` would otherwise take
    // away, since libtest then runs every test on the same thread.
    //
    // So: stable per (run, thread, epoch), where the epoch is bumped by
    // `next_test_home` when a test builds an app.
    // `thread_dir` supplies the rest: a token no other run can reproduce, the
    // thread, creation, and removal when the test's thread ends. The epoch is
    // this module's own contribution to the label.
    let home = crate::scratch::thread_dir(&format!(
        "home-{}",
        TEST_HOME_EPOCH.with(std::cell::Cell::get)
    ));
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
    const fn rescued_something(&self) -> bool {
        self.copied > 0 && self.failed == 0
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
    let summary = rescue_into(&home, Path::new("."), &|env| {
        std::env::var_os(env).is_some()
    })?;
    let _ = RESCUE_NOTICE.set(rescue_toast(&summary, &home));
    Some(summary)
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
    // A pass that copies nothing must not reach the marker: see
    // `RescueSummary::rescued_something`.
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
    if summary.rescued_something() {
        write_marker(&marker);
    }
    Some(summary)
}

/// What the startup rescue has to say, waiting for a window to say it in.
///
/// The rescue runs in `main`, before any store is read and therefore before
/// the app exists; the toast belongs to the app. One slot, written once by
/// [`consolidate_once`] and read once by the window — rather than threading a
/// message through a constructor that no other caller would ever pass.
static RESCUE_NOTICE: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();

/// The rescue's message, if this launch had one. Read by the window as it
/// opens; every later call answers the same, so it is safe to ask twice.
pub(crate) fn rescue_notice() -> Option<String> {
    RESCUE_NOTICE.get().cloned().flatten()
}

/// What the trader is told after a rescue, or `None` when nothing moved.
///
/// A silent rescue would look like the app relocated their settings behind
/// their back — and worse, they would not know the folder to back up. Says
/// "copies" out loud and names the folder, the way
/// [`crate::paper_home::import_toast`] does for the journal: one app, one
/// way of reporting a one-time import.
pub(crate) fn rescue_toast(summary: &RescueSummary, home: &Path) -> Option<String> {
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

        let first = rescue_into(&home, &empty, &nothing_overridden).expect("the pass runs");
        assert_eq!(first.copied, 0, "that folder held no cockpit");
        assert!(
            !home.join(CONSOLIDATED_MARKER).exists(),
            "and an empty pass must not declare the rescue done"
        );

        let second = rescue_into(&home, &real, &nothing_overridden).expect("the next launch tries");
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
        let summary = rescue_into(&home, &legacy, &nothing_overridden).expect("the pass runs");
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
