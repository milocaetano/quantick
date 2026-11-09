//! Existing file-poll and worker-reload path; editor reload scheduling docks here.
use super::{TabSlot, arrangement_host::ArrangementHost, indicator_manager::IndicatorState};
use std::time::{Duration, Instant};

const SCRIPT_RELOAD_POLL_INTERVAL: Duration = Duration::from_millis(1_000);

impl IndicatorState {
    pub(super) fn log_reload(owner: TabSlot, name: &str) {
        tracing::info!(target: "quantick::app", schema_version = 1_u8,
            event_code = "INDICATOR_SCRIPT_RELOAD", script = %name, tab = owner.tab,
            pane = ?owner.side, action = "recompile_and_replay", "indicator script changed on disk");
    }

    pub(super) fn poll_script_files(&mut self) -> Vec<(TabSlot, String, String)> {
        if self.registry.script_files().is_empty()
            || self.last_script_poll.elapsed() < SCRIPT_RELOAD_POLL_INTERVAL
        {
            return Vec::new();
        }
        self.last_script_poll = Instant::now();
        let mut reloads: Vec<(TabSlot, String, String)> = Vec::new();
        for watch in 0..self.registry.script_files().len() {
            let (owner, index, seen_mtime) = self.registry.script_files()[watch];
            let Some((path, mtime)) = self.script_library.file_info(index) else {
                continue;
            };
            if mtime == seen_mtime {
                continue;
            }
            self.registry.observe_script(owner, mtime);
            match std::fs::read_to_string(&path) {
                Ok(text) => {
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| path.display().to_string());
                    reloads.push((owner, name, text));
                }
                Err(error) => tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "INDICATOR_SCRIPT_UNREADABLE",
                    script = %path.display(),
                    error = %error,
                    action = "reload_skipped",
                    "cannot re-read a changed indicator script"
                ),
            }
        }
        reloads
    }
}

pub(super) fn reload_changed_scripts(
    indicators: &mut IndicatorState,
    tabs: &mut ArrangementHost,
    store: &crate::workspace_store::LayoutStore,
) {
    for (owner, name, text) in indicators.poll_script_files() {
        let saved = super::layout_wiring::saved_inputs(tabs, indicators, store, owner);
        if owner.workspace == tabs.owner()
            && let Some(tab) = tabs.by_id_mut(owner.tab)
            && let Some(pane) = tab.pane_at_mut(owner.side.index())
            && pane.id == owner.pane
        {
            IndicatorState::log_reload(owner, &name);
            let source = crate::indicator_worker::IndicatorSource::Script { name, text };
            pane.indicator_worker.send(match saved {
                Some(values) => crate::indicator_worker::IndicatorCommand::ReloadWithInputs {
                    slot: owner.slot,
                    source,
                    values,
                },
                None => crate::indicator_worker::IndicatorCommand::Reload {
                    slot: owner.slot,
                    source,
                },
            });
        }
    }
}
