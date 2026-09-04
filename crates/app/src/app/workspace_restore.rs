//! Putting a saved cockpit back on the screen.
//!
//! [`super::QuantickApp::restore_workspace`] is the read half of the
//! workspace round trip whose write half is `capture_workspace`: it takes the
//! arrangement a previous session wrote down and rebuilds the tabs, the
//! panes, the dock, the rail and the chrome from it. It is one method and it
//! is long because a cockpit has many parts; it is here rather than in
//! `app.rs` because nothing else in the window needs to see it.

use crate::state::BarSpec;
use crate::tab::CanvasLayout;
use crate::tab::LegendFold;
use crate::ui_state;

use super::{FIRST_TAB_ID, QuantickApp, saved_context_intervals};

impl QuantickApp {
    /// Open the saved workspace over the configured defaults.
    ///
    /// The first tab already exists and is already streaming the market
    /// `main` picked from this same workspace, so it is *arranged* here rather
    /// than opened; the rest are opened outright, each on its own feed. A tab
    /// carries its bar rule explicitly (see [`Self::adopt_tab`]) — inheriting
    /// would replace what the user saved with what the tab beside it happens
    /// to show.
    ///
    /// `save_on_exit` is taken from the file even when the file has no tabs:
    /// a trader who switched autosave off and then reset their layout must not
    /// find it switched back on at the next launch.
    pub(super) fn restore_workspace(&mut self, workspace: ui_state::Workspace) {
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
        if let Some(chrome) = &workspace.chrome {
            self.restore_chrome(chrome);
        }
        if workspace.is_empty() {
            return;
        }
        // Whether tab zero is already the workspace's first market.
        //
        // At startup it is: `main` read that market out of this very file and
        // spawned the window on it, so the tab exists and only its bar rule
        // can differ. Mid-session — a workspace file being opened — it is
        // whatever the trader was looking at, and adopting it would leave the
        // strip holding one market with another's name. Then every tab is
        // opened outright and the ones that were there are closed after, so
        // the strip is *replaced* rather than grown.
        let adopt_first = self.tabs.first().is_some_and(|tab| tab.id == FIRST_TAB_ID)
            && self.tabs.len() == 1
            && self.tabs[0].feed_id == workspace.tabs[0].feed
            && self.tabs[0].symbol == workspace.tabs[0].symbol;
        let stale: Vec<u64> = if adopt_first {
            Vec::new()
        } else {
            self.tabs.iter().map(|tab| tab.id).collect()
        };
        for (index, saved) in workspace.tabs.iter().enumerate() {
            // `restore` has already dropped anything unparseable, so a spec
            // reaching here is one a control could have produced.
            let flow = BarSpec::parse(&saved.flow_bars).ok();
            if index == 0 && adopt_first {
                // Tab zero is the one `main` spawned. Its market matches this
                // entry (that is where `main` read it from), so only its bar
                // rule can still differ — `main` prefers a feed's declared
                // `default_bars` when the workspace names none.
                if let Some(spec) = flow {
                    self.tabs[0].flow_pane.set_spec(spec);
                }
            } else {
                self.open_tab(saved.feed.clone(), saved.symbol.clone(), flow);
            }
            let context_intervals =
                saved_context_intervals(&saved.context_bars, saved.time_bars.as_deref());
            let focus = saved.focus.map(|focus| focus.to_side(saved.focus_slot));
            // `open_tab` activates what it opened, so the tab just arranged is
            // always the last one — index zero on the first pass.
            let target = if index == 0 && adopt_first {
                0
            } else {
                self.tabs.len() - 1
            };
            self.tabs[target].restore_canvas(
                CanvasLayout::from(saved.layout),
                saved.split_fraction,
                saved.context_collapsed,
                focus,
                &context_intervals,
                LegendFold {
                    flow: saved.flow_legend_collapsed,
                    time: saved.time_legend_collapsed,
                },
            );
            self.tabs[target].set_opening_layouts(saved.flow_layout, &saved.context_layouts);
        }
        // The markets that were on screen before this workspace was opened.
        // Closed last, so the strip is never empty in between and `close_tab`
        // — which refuses to close the only tab — always has the restored
        // ones to keep. Each closes through its own path, so a simulated
        // position ends in a journaled flatten rather than vanishing.
        for id in stale {
            if let Some(index) = self.tabs.iter().position(|tab| tab.id == id) {
                self.close_tab(index);
            }
        }
        self.active_tab = workspace.active_tab.min(self.tabs.len() - 1);
        let config = self.config.clone();
        self.active_tab_mut().refresh_chip_label(&config);
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "UI_STATE_RESTORED",
            path = %self.workspace.ui_state_path().display(),
            tabs = self.tabs.len(),
            active = self.active_tab,
            save_on_exit = self.workspace.session().save_on_exit(),
            "workspace restored"
        );
    }
}
