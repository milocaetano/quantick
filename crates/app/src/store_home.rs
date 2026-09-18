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

pub(crate) use quantick_stores::home::*;

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
    let summary = rescue_into(&home, Path::new("."), COCKPIT_STORES, &|env| {
        std::env::var_os(env).is_some()
    })?;
    let _ = RESCUE_NOTICE.set(rescue_toast(&summary, &home));
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

#[cfg(test)]
mod tests {
    use super::*;

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
