//! Platform and captured environment inputs for the headless cockpit home.
pub(crate) use quantick_stores::home::*;
#[cfg(test)]
pub(crate) use quantick_workspace::write_refusal::refuse_writes_on_this_thread;
pub(crate) use quantick_workspace::write_refusal::{
    WritesRefused, guard_write, refuse_writes, writes_refused,
};
use std::path::PathBuf;

static HOME: quantick_ui_model::store_home::StoreHome =
    quantick_ui_model::store_home::StoreHome::new();
pub(crate) const COCKPIT_STORES: &[CockpitStore] =
    &quantick_ui_model::store_home::cockpit_stores([
        crate::app::alerts_runtime::path,
        crate::theme::store::path,
        crate::ui_state::default_path,
        crate::indicators::state_file::default_path,
        crate::layouts::default_path,
        crate::layouts::workspaces_path,
        crate::layouts::market_settings_path,
        crate::indicators::preset_file::default_path,
        crate::bubble_presets::assets_path,
        crate::chart_layers::default_path,
        crate::drawings::presets::default_path,
        crate::footprint_config::settings_path,
        crate::footprint_presets::default_path,
        crate::candle_presets::default_path,
        crate::paper_state::default_path,
        crate::symbols_file::default_path,
    ]);

fn captured(name: &str) -> Option<std::ffi::OsString> {
    #[cfg(any(feature = "scenario-harness", test))]
    {
        crate::hooks::captured::var(name).map(Into::into)
    }
    #[cfg(not(any(feature = "scenario-harness", test)))]
    {
        let _ = name;
        None
    }
}

pub(crate) fn home() -> Option<PathBuf> {
    HOME.home(crate::paper_home::shelf_dir)
}
pub(crate) fn resolve(file: &str) -> PathBuf {
    HOME.resolve(file, crate::paper_home::shelf_dir, captured)
}
pub(crate) fn consolidate_once() -> Option<RescueSummary> {
    HOME.consolidate_once(
        cfg!(test),
        crate::paper_home::shelf_dir,
        COCKPIT_STORES,
        captured,
        crate::launch::operator_paths().theme.is_some(),
    )
}
pub(crate) fn rescue_notice() -> Option<String> {
    HOME.rescue_notice()
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

#[cfg(any(feature = "scenario-harness", test))]
crate::hooks::declare_hooks![
    "QUANTICK_BUBBLE_ASSETS",
    "QUANTICK_CANDLE_PRESETS",
    "QUANTICK_CHART_LAYERS",
    "QUANTICK_DRAWING_PRESETS",
    "QUANTICK_FOOTPRINT_SETTINGS",
    "QUANTICK_FOOTPRINT_PRESETS",
    "QUANTICK_INDICATOR_PRESETS",
    "QUANTICK_INDICATORS_STATE",
    "QUANTICK_INSTRUMENTS",
    "QUANTICK_LAYOUTS",
    "QUANTICK_MARKET_SETTINGS",
    "QUANTICK_PAPER_STATE",
    "QUANTICK_STRATEGY_PRESETS",
    "QUANTICK_SYMBOLS",
    "QUANTICK_UI_STATE"
];
