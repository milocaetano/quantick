//! Window-level wiring for deal recorders.

use quantick_control_handlers::deal_recorder::DealRecordingPort;

use crate::config::AppConfig;
use crate::deal_recording::{self, DealRecorder};

use super::ArrangementHost;
use super::chrome::ChromeState;
use super::desk_wiring::DeskAdapter;
use super::workspace_runtime::WorkspaceRuntime;

fn recorder_for(
    chrome: &ChromeState,
    config: &AppConfig,
    (feed_id, symbol): (&str, &str),
    day_cache: deal_recording::DayCache,
) -> DealRecorder {
    #[cfg(any(feature = "scenario-harness", test))]
    let scripted = chrome.harness.deal_recording_default();
    #[cfg(not(any(feature = "scenario-harness", test)))]
    let scripted = None;
    let default_on = config.records_by_default(scripted.or(chrome.record_deals), feed_id);
    let dir = deal_recording::resolve_dir(config.deals_dir());
    DealRecorder::with_cache(symbol, dir, default_on, day_cache).for_feed(feed_id)
}

/// Ensure every tab owns a recorder for its current feed and symbol.
pub(super) fn ensure(runtime: &mut WorkspaceRuntime, chrome: &ChromeState, config: &AppConfig) {
    let tz_minutes = runtime.tz.minutes();
    for index in 0..runtime.tabs.len() {
        let tab = runtime.tabs.runtime_mut(index);
        tab.deal_recorder.set_timezone(tz_minutes);
        if tab.deal_recorder.is_for(&tab.active.0, &tab.active.1) {
            continue;
        }
        tab.deal_recorder.stop();
        let day_cache = tab.deal_recorder.take_day_cache();
        let market = (tab.active.0.as_str(), tab.active.1.as_str());
        let mut recorder = recorder_for(chrome, config, market, day_cache);
        recorder.set_timezone(tz_minutes);
        recorder.set_available(tab.link.feed_capabilities.borrow().deal_counter);
        tab.deal_recorder = recorder;
    }
}

/// Save the default and apply it to undecided recorders.
pub(crate) fn set_default(saved: &mut Option<bool>, tabs: &mut ArrangementHost, on: bool) {
    *saved = Some(on);
    for tab in tabs.iter_mut() {
        tab.deal_recorder.set_default(on);
    }
}

/// `feed.deal_recording.set` reaches each tab's own recorder.
impl DealRecordingPort for DeskAdapter<'_> {
    fn deal_recorder(
        &mut self,
        index: usize,
    ) -> Option<&mut dyn quantick_control_handlers::deal_recorder::DealRecorder> {
        Some(self.runtime.tabs.get_mut(index)?)
    }
}
