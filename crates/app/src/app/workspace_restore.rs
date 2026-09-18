//! Synchronous imperative restore effects driven by the headless arrangement owner.
use super::arrangement_adapter::ArrangementAdapter;
use super::saved_context_intervals;
use crate::tab::{CanvasLayout, LegendFold};
use crate::ui_state::{self, SavedFocusExt};
use quantick_workspace::arrangement::{RestoreEffect, RestoreMode};

impl ArrangementAdapter<'_> {
    pub(super) fn restore_workspace(&mut self, workspace: ui_state::Workspace) {
        self.run_restore(
            RestoreMode::StartupOrImport,
            &workspace.tabs,
            workspace.active_tab,
            workspace.chrome.as_ref(),
            Some(&workspace),
        );
        if workspace.is_empty() {
            return;
        }
        tracing::info!(target: "quantick::app", schema_version = 1_u8,
            event_code = "UI_STATE_RESTORED", path = %self.workspace.ui_state_path().display(),
            tabs = self.tabs.len(), active = self.tabs.active_index(),
            save_on_exit = self.workspace.session().save_on_exit(), "workspace restored");
    }
    pub(super) fn run_restore(
        &mut self,
        mode: RestoreMode,
        documents: &[ui_state::SavedTab],
        active: usize,
        chrome: Option<&ui_state::SavedChrome>,
        startup: Option<&ui_state::Workspace>,
    ) {
        self.tabs
            .begin_restore(
                mode,
                documents.len(),
                documents
                    .first()
                    .map(|saved| (saved.feed.as_str(), saved.symbol.as_str())),
                active,
            )
            .expect("caller checked nonempty named arrangement");
        loop {
            let step = self.tabs.restore_step();
            let effect = step.effect();
            match effect {
                RestoreEffect::AdoptSettings => {
                    self.adopt_workspace_settings(startup.expect("startup settings effect"))
                }
                RestoreEffect::RestoreChrome => {
                    if let Some(chrome) = chrome {
                        self.restore_chrome(chrome);
                    }
                }
                RestoreEffect::OpenSaved { index, adopt } => {
                    let saved = &documents[index];
                    let flow = quantick_engine::bar_registry::BUILTIN_BARS
                        .parse(&saved.flow_bars)
                        .ok();
                    if let Some(id) = adopt {
                        if let Some(spec) = flow {
                            self.tabs
                                .by_id_mut(id.0)
                                .expect("adopted runtime exists")
                                .flow_pane
                                .set_spec(spec);
                        }
                    } else {
                        let opening = self
                            .tabs
                            .plan_restore(&step)
                            .expect("open effect plans insertion");
                        self.open_with_plan(
                            saved.feed.clone(),
                            saved.symbol.clone(),
                            flow,
                            Some(opening),
                        );
                    }
                }
                RestoreEffect::ArrangeSaved { index, target } => {
                    let saved = &documents[index];
                    let intervals =
                        saved_context_intervals(&saved.context_bars, saved.time_bars.as_deref());
                    let tab = self
                        .tabs
                        .by_id_mut(target.0)
                        .expect("core selected an existing runtime");
                    tab.restore_canvas(
                        CanvasLayout::from(saved.layout),
                        saved.split_fraction,
                        saved.context_collapsed,
                        saved.focus.map(|focus| focus.to_side(saved.focus_slot)),
                        &intervals,
                        LegendFold {
                            flow: saved.flow_legend_collapsed,
                            time: saved.time_legend_collapsed,
                        },
                    );
                    tab.set_opening_layouts(saved.flow_layout, &saved.context_layouts);
                }
                RestoreEffect::CloseStale { .. } => {
                    if let Some(plan) = self.tabs.plan_restore(&step) {
                        self.close_planned(plan)
                            .expect("exclusive restore keeps close current");
                    }
                }
                RestoreEffect::SelectSaved { .. } => {
                    let plan = self.tabs.plan_restore(&step).expect("selection effect");
                    self.tabs.commit_selection(plan);
                }
                RestoreEffect::RefreshLabel => {
                    let config = self.config.clone();
                    self.active_tab_mut().refresh_chip_label(&config);
                }
                RestoreEffect::Finish => {}
            }
            self.tabs.advance_restore(step);
            if effect == RestoreEffect::Finish {
                break;
            }
        }
    }
    fn adopt_workspace_settings(&mut self, workspace: &ui_state::Workspace) {
        self.workspace.session_mut().adopt(
            workspace.save_on_exit,
            workspace.saved.clone(),
            workspace.recent_workspaces.clone(),
        );
        self.refresh_recent_workspaces();
        // One stat at boot, so the Reset entry can gate on a field instead of
        // the filesystem for the rest of the session. A file with no tabs
        // still counts: it carries the autosave setting, and Reset is how the
        // trader gets rid of it.
        let on_disk = self.workspace.ui_state_path().exists();
        self.workspace.session_mut().set_saved(on_disk);
        // Outside the chrome block deliberately: the stars belong to the file,
        // not to the arrangement, so a workspace with nothing else in it still
        // hands the rail back its pinned section.
        //
        // An empty list is silence, not an instruction. The format cannot tell
        // "the trader starred nothing" from "this file predates the field" or
        // "this bundle was written by an install that never saved a cockpit",
        // and this same function restores an *imported* workspace mid-session —
        // where emptying the rail on silence would throw away a curated rail on
        // the strength of a key that was never written. Unstarring the last
        // tool is not lost by this: that click writes the empty list itself,
        // and the rail it would be restored onto is already empty.
        if !workspace.favorite_tools.is_empty() {
            let unknown = self.toolrail.set_favorites(&workspace.favorite_tools);
            if !unknown.is_empty() {
                // Said out loud because the next star click writes the pruned
                // list back over the file: this is the only moment the id
                // still exists anywhere.
                tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "TOOL_FAVORITE_DROPPED",
                    tools = %unknown.join(","),
                    action = "no_such_drawing_tool",
                    "a starred tool this build does not offer was dropped from the rail"
                );
            }
        }
    }
}
