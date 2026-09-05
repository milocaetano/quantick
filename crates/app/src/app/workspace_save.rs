//! Writing the cockpit down: what a saved workspace is made of.
//!
//! The write half of the round trip whose read half is
//! [`super::workspace_restore`]. `capture_workspace` reads the arrangement
//! off the live window, the named-workspace methods put it on disk under a
//! name and take it back off again, the `write_*` methods edit one section of
//! one cockpit store in place, and the export/import pair moves a whole
//! cockpit between machines. They are together because they share the
//! stores, the pickers and the one acknowledgement lane — `note_workspace`
//! — and because nothing outside the window calls any of them.

use std::time::Instant;

use eframe::egui;

use crate::drawings;
use crate::indicator_worker::{IndicatorCommand, SlotId};
use crate::indicators::preset_file;
use crate::state::BarSpec;
use crate::symbols_file;
use crate::tab::{CanvasLayout, LegendFold};
use crate::timezone::TzOffset;
use crate::ui_state;
use crate::workspace_store::WorkspacePick;

use quantick_feed::history_reach;

use super::{QuantickApp, saved_context_intervals};

impl QuantickApp {
    /// The window as it stands, in the form the workspace file records.
    ///
    /// Read off the live state rather than accumulated as it changes: the
    /// arrangement is a dozen fields spread over the tabs and the chrome, and
    /// a second copy maintained by every control that moves one of them would
    /// be a dozen chances to forget. Saving is rare and event-driven, so
    /// reading them all at once costs nothing anyone can see.
    pub(super) fn capture_workspace(&self) -> ui_state::Workspace {
        let (tabs, chrome) = self.capture_arrangement();
        ui_state::Workspace::new(
            self.workspace.session().save_on_exit(),
            self.chrome.window_size,
            self.active_tab,
            tabs,
            Some(chrome),
        )
        // Every write rewrites the whole file, so the bookmarks have to ride
        // along or saving the startup screen would silently delete them.
        .with_saved(self.workspace.session().bookmarks().to_vec())
        // And the recent workspace files, for the same reason: a save that
        // dropped them would empty the Open-recent menu every time the
        // trader saved their layout.
        .with_recent(self.workspace.session().recent().to_vec())
        // And so does the replay folder, for exactly the same reason: a save
        // that dropped it would send the browser back to nowhere on the next
        // launch, which is the failure this field was added to end. The
        // trader's *pick*, never the folder in use — a run under
        // `QUANTICK_REPLAY_DIR` must not write a QA scratch path into their
        // workspace, and accepting the default home is not a choice either.
        .with_replay_folder(self.replay_view.stored_pick().map(str::to_owned))
        .with_replay_day_before(self.replay_view.stored_day_before())
        // And the starred tools, for the third time and the same reason. They
        // are already on disk the moment the star is clicked; riding along
        // here keeps a full-file write from erasing what the star wrote.
        .with_favorites(self.starred_tool_ids())
    }

    /// The rail's pinned section as tool ids, in star order — the form the
    /// workspace file keeps it in.
    pub(super) fn starred_tool_ids(&self) -> Vec<String> {
        self.toolrail
            .favorites()
            .iter()
            .map(|tool| tool.id().to_owned())
            .collect()
    }

    /// Put the chrome back — the mirror of the [`Self::capture_arrangement`]
    /// half that produced it.
    ///
    /// One function because there are two callers and no way for the compiler
    /// to notice when only one of them learns a new field: the startup
    /// workspace and a named bookmark describe the same thing, and
    /// [`ui_state::NamedArrangement`] says so in as many words. Restoring them
    /// through two copies of the same eight lines is how a field comes to
    /// persist but never come back from a bookmark — a bug with no compile
    /// error behind it.
    ///
    /// The starred tools are the one thing it does not speak for: they live at
    /// the file's own level rather than inside an arrangement
    /// ([`ui_state::Workspace::favorite_tools`]), precisely so that opening a
    /// bookmark cannot rearrange the rail the trader curated.
    pub(super) fn restore_chrome(&mut self, chrome: &ui_state::SavedChrome) {
        self.tz = TzOffset::new(chrome.timezone_minutes);
        self.dock
            .restore(chrome.dock_visible, chrome.dock_tab.map(Into::into));
        self.toolrail.set_dock(chrome.rail_dock.into());
        self.toolrail.set_visible(chrome.rail_visible);
        self.health.show_perf = chrome.perf_readings;
        self.history.progressive_history = chrome.progressive_history;
        // A token this release does not know keeps the reach it had — the
        // default on startup, whatever the trader picked when a bookmark is
        // opened mid-session. Never a silent fallback to something else: the
        // reach decides how much a press fetches.
        if let Some(reach) = chrome
            .history_reach
            .as_deref()
            .and_then(history_reach::HistoryReach::from_token)
        {
            self.history.history_reach = reach;
        }
        // Through the setter, so a hand-edited workspace cannot restore a span
        // the campaign could never reach.
        if let Some(minutes) = chrome.history_reach_span_minutes {
            self.set_history_reach_span_minutes(minutes);
        }
        self.history.venue_lead_in = chrome.venue_lead_in;
        self.surfaces
            .drawing_chrome
            .restore_inspector_position(chrome.inspector_position);
    }

    /// The tabs and the chrome as they stand — the part a startup workspace
    /// and a named one describe identically, so both capture through here.
    pub(super) fn capture_arrangement(&self) -> (Vec<ui_state::SavedTab>, ui_state::SavedChrome) {
        let tabs = self
            .tabs
            .iter()
            .map(|tab| ui_state::SavedTab {
                feed: tab.feed_id.clone(),
                symbol: tab.symbol.clone(),
                layout: tab.layout.into(),
                split_fraction: Some(tab.split_fraction),
                context_collapsed: tab.context_collapsed,
                focus: Some(ui_state::SavedFocus::from_side(tab.focused_side()).0),
                focus_slot: ui_state::SavedFocus::from_side(tab.focused_side()).1,
                flow_bars: tab.flow_pane.state.spec().to_config_string(),
                // Only a pane that exists has an interval worth recording; a
                // tab that never showed the split restores on the default,
                // which is what it had.
                time_bars: tab
                    .time_pane()
                    .map(|pane| pane.state.spec().to_config_string()),
                context_bars: tab
                    .time_panes
                    .iter()
                    .map(|pane| pane.state.spec().to_config_string())
                    .collect(),
                flow_layout: tab.flow_pane.layout.map(|layout| layout.0),
                context_layouts: tab
                    .time_panes
                    .iter()
                    .map(|pane| {
                        pane.layout
                            .map_or(crate::ui_state::LAYOUT_UNRECORDED, |layout| layout.0)
                    })
                    .collect(),
                flow_legend_collapsed: tab.flow_pane.legend_collapsed,
                // A tab with no time pane has no second legend, and `false`
                // is what it will restore into when one is opened: a pane
                // that never existed cannot have been folded.
                time_legend_collapsed: tab.time_pane().is_some_and(|pane| pane.legend_collapsed),
            })
            .collect();
        let chrome = ui_state::SavedChrome {
            timezone_minutes: self.tz.minutes(),
            dock_visible: self.dock.visible(),
            dock_tab: self.dock.tab().map(Into::into),
            rail_visible: self.toolrail.visible(),
            rail_dock: self.toolrail.dock().into(),
            perf_readings: self.health.show_perf,
            // Never written any more: the stars are a standing choice and live
            // at the top of the file. An arrangement that carried a copy would
            // be an arrangement that could overwrite them on open.
            legacy_favorite_tools: Vec::new(),
            progressive_history: self.history.progressive_history,
            // The default writes no key: a workspace that says nothing about
            // the reach restores the press the button has always had, which is
            // exactly what the default is.
            // Written whenever it differs from what the config seeds, so a
            // workspace only carries an opinion its owner actually formed.
            history_reach_span_minutes: (self.history.history_reach_span_minutes
                != self.config.history.reach_span_minutes)
                .then_some(self.history.history_reach_span_minutes),
            history_reach: (self.history.history_reach != history_reach::HistoryReach::default())
                .then(|| self.history.history_reach.token().to_owned()),
            venue_lead_in: self.history.venue_lead_in,
            inspector_position: self.surfaces.drawing_chrome.remembered_inspector_position(),
        };
        (tabs, chrome)
    }

    /// Ask the operating system where to put a workspace file, off the UI
    /// thread. One dialog at a time, the trades-folder picker's own pattern.
    ///
    /// The cockpit is written to its stores *first*, so what the file gets is
    /// the screen as it stands rather than whatever was last flushed —
    /// indicator state is written debounced, and an export that raced it
    /// would quietly save a cockpit the trader never had.
    pub(super) fn open_workspace_export_picker(&mut self) {
        if self.workspace.picker_open() {
            return;
        }
        let start = crate::workspace_bundle::default_dir();
        let suggested = crate::workspace_bundle::file_name_for(&self.suggested_workspace_name());
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("quantick-workspace-export-picker".into())
            .spawn(move || {
                let mut dialog = rfd::FileDialog::new()
                    .set_title("Export workspace")
                    .add_filter("quantick workspace", &["toml"])
                    .set_file_name(suggested);
                if let Some(start) = start {
                    let _ = std::fs::create_dir_all(&start);
                    dialog = dialog.set_directory(&start);
                }
                let _ = sender.send(dialog.save_file());
            })
            .expect("spawn workspace export picker thread");
        self.workspace.open_picker(WorkspacePick::Export, receiver);
    }

    /// The same, for choosing a workspace file to open.
    pub(super) fn open_workspace_import_picker(&mut self) {
        if self.workspace.picker_open() {
            return;
        }
        let start = crate::workspace_bundle::default_dir();
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("quantick-workspace-import-picker".into())
            .spawn(move || {
                let mut dialog = rfd::FileDialog::new()
                    .set_title("Open workspace")
                    .add_filter("quantick workspace", &["toml"]);
                if let Some(start) = start
                    && start.is_dir()
                {
                    dialog = dialog.set_directory(&start);
                }
                let _ = sender.send(dialog.pick_file());
            })
            .expect("spawn workspace import picker thread");
        self.workspace.open_picker(WorkspacePick::Import, receiver);
    }

    /// Land whatever the dialog answered.
    pub(super) fn poll_workspace_picker(&mut self) {
        let Some((intent, receiver)) = self.workspace.picker() else {
            return;
        };
        let choice = match receiver.try_recv() {
            Ok(choice) => choice,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            // The dialog thread died without answering — no display server,
            // a COM failure. Treated as "no answer yet" this would leave the
            // field set forever and both menu entries silently dead, since
            // each refuses to open a second dialog.
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.workspace.close_picker();
                tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "WORKSPACE_PICKER_LOST",
                    action = "picker_reset",
                    "the file dialog closed without an answer"
                );
                self.note_workspace(
                    "The file chooser could not open — see the log. Try again.".to_owned(),
                );
                return;
            }
        };
        let intent = *intent;
        self.workspace.close_picker();
        // A cancelled dialog is an answer, not a failure: say nothing.
        let Some(path) = choice else { return };
        match intent {
            WorkspacePick::Export => self.export_workspace_to(&path),
            WorkspacePick::Import => self.import_workspace_from(&path),
        }
    }

    /// A starting name for an export: what the cockpit is showing, so the
    /// trader does not have to invent one to get a usable file name.
    fn suggested_workspace_name(&self) -> String {
        let tab = self.active_tab();
        format!(
            "{} {}",
            tab.symbol,
            tab.flow_pane.state.spec().to_config_string()
        )
    }

    /// Write every store that is still only in memory, so a bundle captured
    /// next describes the screen rather than the last flush.
    fn flush_cockpit_stores(&mut self) {
        // The layouts file is written debounced, off the frame path. An
        // export is the one moment worth paying it immediately, or the
        // bundle would carry the layouts as they stood a second ago.
        self.flush_layouts();
        self.maintain_chart_layers();
    }

    /// Export the whole cockpit to one file, and say what happened.
    pub(super) fn export_workspace_to(&mut self, path: &std::path::Path) {
        // Here rather than before the dialog: these two writes are how the
        // bundle comes to describe the screen instead of the last flush, and
        // doing them up front meant a trader who pressed Cancel had still
        // silently redefined what the app opens on. It also keeps the harness
        // hook on exactly the menu's path.
        self.save_workspace("export");
        self.flush_cockpit_stores();
        let name = crate::workspace_bundle::recent_label(path);
        let outcome = crate::workspace_bundle::capture(
            &name,
            crate::store_home::COCKPIT_STORES,
            &crate::workspace_bundle::live_paths,
        )
        .and_then(|bundle| crate::workspace_bundle::write(path, &bundle).map(|()| bundle.len()));
        match outcome {
            Ok(stores) => {
                crate::workspace_bundle::remember_recent(
                    self.workspace.session_mut().recent_mut(),
                    path,
                );
                self.refresh_recent_workspaces();
                self.save_workspace("export_recent");
                tracing::info!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "WORKSPACE_EXPORTED",
                    path = %path.display(),
                    stores,
                    action = "workspace_written",
                    "workspace exported"
                );
                self.note_workspace(format!(
                    "Workspace exported to {} — {stores} settings groups",
                    path.display()
                ));
            }
            Err(error) => {
                tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "WORKSPACE_EXPORT_FAILED",
                    path = %path.display(),
                    %error,
                    action = "workspace_not_written",
                    "could not export the workspace"
                );
                self.note_workspace(format!("Workspace not exported — {error}"));
            }
        }
    }

    /// Open a workspace file over the live cockpit.
    ///
    /// Refused whole or applied whole: [`crate::workspace_bundle::apply`] checks
    /// every section before writing any, so a bad file leaves the screen
    /// exactly as it was and says why. What reaches the disk is then read
    /// back into the running app, because a cockpit that only changed on disk
    /// would be overwritten by this session's own save on exit.
    pub(super) fn import_workspace_from(&mut self, path: &std::path::Path) {
        // What the debounce still holds is written first, or a bundle with no
        // layouts section would reload a file a second behind the screen.
        self.flush_layouts();
        let outcome = crate::workspace_bundle::read(path).and_then(|bundle| {
            crate::workspace_bundle::apply(
                &bundle,
                crate::store_home::COCKPIT_STORES,
                &crate::workspace_bundle::live_paths,
            )
        });
        match outcome {
            Ok(written) => {
                let stores = written.len();
                self.reload_cockpit_stores(&written);
                crate::workspace_bundle::remember_recent(
                    self.workspace.session_mut().recent_mut(),
                    path,
                );
                self.refresh_recent_workspaces();
                // The recent list lives in the workspace file the import just
                // replaced, so it has to be written back after the reload —
                // otherwise opening a file would forget that it was opened.
                self.save_workspace("import_recent");
                tracing::info!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "WORKSPACE_IMPORTED",
                    path = %path.display(),
                    stores,
                    action = "cockpit_replaced",
                    "workspace imported"
                );
                self.note_workspace(format!(
                    "Workspace \"{}\" opened — {stores} settings groups restored",
                    crate::workspace_bundle::recent_label(path)
                ));
            }
            Err(error) => {
                tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "WORKSPACE_IMPORT_REFUSED",
                    path = %path.display(),
                    %error,
                    action = "cockpit_unchanged",
                    "workspace refused"
                );
                self.note_workspace(format!("Workspace not opened — {error}"));
            }
        }
    }

    /// Read every cockpit store back into the running app.
    ///
    /// Called after an import wrote them. Each store goes through the loader
    /// it already had, so an imported cockpit is restored by exactly the code
    /// that restores one at startup — a second, import-only restore path is
    /// how the two would drift.
    fn reload_cockpit_stores(&mut self, imported: &[&str]) {
        self.added_symbols = symbols_file::load(self.workspace.symbols_path());
        self.drawing_presets = drawings::presets::PresetStore::load_from(
            drawings::presets::PresetStore::default_path(),
        );
        self.footprint_config =
            crate::footprint_config::load(self.workspace.footprint_settings_path());
        self.surfaces.footprint_settings.reload_presets();
        self.indicators.indicator_presets =
            preset_file::PresetStore::load(self.workspace.indicator_presets_path());

        // The tab strip first, and *before* the indicators: the restore adds
        // each indicator to whatever pane is focused right now, so the tabs
        // have to be the imported ones — and the focus on the pane the file
        // describes — before a single indicator is added. Getting this order
        // wrong puts a trader's imported indicators on the tab they happened
        // to be looking at, or on its time pane.
        let workspace =
            ui_state::load(self.workspace.ui_state_path()).restore(&self.config.clone());
        self.restore_workspace(workspace);
        self.restore_chart_layers();

        // The layouts come last, once the tabs are the imported ones: every
        // pane is stripped and re-seeded from the imported file.
        self.reload_layouts(imported);
    }

    /// Work out which remembered workspace files are still there.
    ///
    /// Called when the list changes, never from the menu body — see
    /// [`Self::recent_on_disk`]. The stored list keeps every entry: a file on
    /// a drive that is merely unplugged today comes back when it is plugged
    /// in, and only the menu is filtered.
    pub(super) fn refresh_recent_workspaces(&mut self) {
        let existing = crate::workspace_bundle::existing_recent(self.workspace.session().recent());
        self.workspace.session_mut().set_recent_on_disk(existing);
    }

    /// Take every indicator off every pane.
    ///
    /// Straight off each pane's own collection rather than by walking
    /// `slot_kinds`: that list is bookkeeping for the state *file*, and an
    /// indicator can be on a pane without being in it — the autostart hooks
    /// add without registering, and `forget_last_indicator_state_change` pops
    /// an entry while leaving the indicator on screen. Clearing the list
    /// would have left those behind for the imported set to stack on top of.
    pub(super) fn clear_indicators(&mut self) {
        /// Empty one pane, view and worker alike.
        fn strip(pane: &mut crate::pane::ChartPane) {
            let slots: Vec<SlotId> = pane.indicators.all().iter().map(|view| view.slot).collect();
            for slot in slots {
                pane.indicators.remove(slot);
                pane.indicator_worker.send(IndicatorCommand::Remove(slot));
            }
        }
        for tab in &mut self.tabs {
            // Every pane the tab holds, not the two it used to. `panes_mut`
            // rather than `pane_mut(Time)`: the latter falls back to the flow
            // pane when a tab was never split, which would strip it twice, and
            // it stops at the *first* context chart — so the second stacked
            // chart kept its indicators while `slot_kinds` was cleared out from
            // under them, and the imported set stacked on top.
            for pane in tab.panes_mut() {
                strip(pane);
            }
        }
        self.indicators.slot_kinds.clear();
        self.indicators.operator_slots.clear();
        self.indicators.script_files.clear();
        self.indicators.pending_hidden.clear();
        self.indicators.pending_styles.clear();
        self.mark_indicator_state_dirty();
    }

    /// Show the trader where the cockpit is kept, and open it.
    ///
    /// The answer to "where does this thing save my setup?" — which, before
    /// the durable home, had no single answer at all.
    pub(super) fn reveal_cockpit_home(&mut self) {
        let Some(home) = crate::store_home::home() else {
            self.note_workspace(
                "This system reports no documents folder — quantick keeps the cockpit beside \
                 wherever it was launched from"
                    .to_owned(),
            );
            return;
        };
        self.note_workspace(format!("Cockpit saved in {}", home.display()));
        // The journal panel's opener, not a second copy of the platform
        // table: two of them means the next fix lands on one and misses the
        // other. Best effort — the path is on the status line either way, so
        // a system with no file manager still answers the question.
        crate::paper_trading::reveal_folder(&home);
    }

    /// Write the workspace and say so on the status bar.
    ///
    /// The notice is the point of the explicit action: a trader who arranges a
    /// cockpit and clicks Save wants to know it is kept, and "it looks the
    /// same" is not an answer. A failed write says *that* instead — being told
    /// "saved" and finding out at the next launch is the one outcome worth
    /// engineering against.
    pub(super) fn save_workspace(&mut self, reason: &'static str) {
        let workspace = self.capture_workspace();
        let saved = ui_state::save(self.workspace.ui_state_path(), &workspace);
        self.workspace.session_mut().note_write(saved);
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "UI_STATE_SAVED",
            path = %self.workspace.ui_state_path().display(),
            tabs = workspace.tabs.len(),
            saved,
            reason,
            action = if saved { "workspace_written" } else { "workspace_not_written" },
            "workspace save"
        );
        self.note_workspace(if saved {
            format!(
                "Workspace saved — quantick opens on {} {}",
                workspace.tabs.len(),
                if workspace.tabs.len() == 1 {
                    "chart tab"
                } else {
                    "chart tabs"
                }
            )
        } else {
            "Workspace could not be saved — see the log".to_owned()
        });
    }

    /// Update *only* where the properties popup goes, in the workspace file as
    /// it already stands. Says whether anything was written.
    ///
    /// Deliberately not [`Self::save_workspace`]. That one captures the whole
    /// window — every tab, its market, its bar rule, the layout, the window
    /// size — and adopts it as the startup screen. Parking a popup is not a
    /// statement about any of that: a trader who opens six tabs to research
    /// something, then nudges the popup out of the way, must not find those six
    /// tabs waiting for them tomorrow. The switch that governs whole-window
    /// saves says "when the window closes", and this write happens mid-session,
    /// so it has no business speaking for the tabs.
    ///
    /// A file with no chrome section is left alone rather than created: there
    /// is nothing to update, and inventing a startup workspace out of a drag
    /// would undo a `Reset startup layout` the trader just asked for. The exit
    /// save is what creates the file, and it carries the position with it.
    ///
    /// No toast either. The "Workspace saved" line answers a deliberate *Save*,
    /// and repeating it after every small gesture would turn the window's one
    /// acknowledgement channel into wallpaper — but the log still names the
    /// write, so a position that went missing is answerable.
    fn write_inspector_position(&mut self) -> bool {
        let mut file = ui_state::load(self.workspace.ui_state_path());
        let Some(chrome) = file.chrome.as_mut() else {
            return false;
        };
        let position = self.surfaces.drawing_chrome.remembered_inspector_position();
        if chrome.inspector_position == position {
            return false;
        }
        chrome.inspector_position = position;
        let saved = ui_state::save(self.workspace.ui_state_path(), &file);
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "UI_STATE_POPUP_POSITION_SAVED",
            path = %self.workspace.ui_state_path().display(),
            parked = position.is_some(),
            saved,
            action = if saved { "position_written" } else { "position_not_written" },
            "properties popup position"
        );
        saved
    }

    /// Write the bookmarks without disturbing the startup arrangement.
    ///
    /// Reads the file back and swaps only the named entries, rather than
    /// capturing the live window: saving a bookmark must not redefine what the
    /// app opens on, and `capture_workspace` describes the screen *now*, which
    /// is exactly what the startup arrangement must not become.
    fn write_bookmarks(&mut self) -> bool {
        let bookmarks = self.workspace.session().bookmarks().to_vec();
        let written = self.edit_workspace_file("UI_STATE_BOOKMARKS_WRITTEN", |file| {
            file.saved = bookmarks;
        });
        self.workspace.session_mut().note_write(written);
        written
    }

    /// Change one standing choice in the workspace file, leaving everything
    /// else in it exactly as it was. `true` when the change reached the disk.
    ///
    /// This file holds three choices that are not descriptions of the screen —
    /// the named bookmarks, the replay folder, the starred tools. Each is made
    /// by a single click and each is written on the spot rather than at exit,
    /// because "it forgot again" must not be one crash away. Each used to
    /// hand-roll the same read-swap-write, and three copies were three chances
    /// to differ: two carried `save_on_exit` through and one did not, so a
    /// trader with autosave off who picked a replay folder before the file
    /// existed had autosave quietly switched back on at the next launch.
    ///
    /// `save_on_exit` rides along here because it is the one live setting that
    /// belongs to the *file* rather than to any arrangement inside it.
    ///
    /// A file this build cannot read is never rewritten — see
    /// [`ui_state::load_for_edit`]. `workspace_saved` is deliberately not
    /// touched: whether a startup *arrangement* exists is a different question
    /// from whether this file does, and the caller answers it.
    fn edit_workspace_file(
        &mut self,
        event_code: &'static str,
        edit: impl FnOnce(&mut ui_state::Workspace),
    ) -> bool {
        let Some(mut file) = ui_state::load_for_edit(self.workspace.ui_state_path()) else {
            self.note_workspace(
                "The workspace file could not be read, so it was left alone — see the log"
                    .to_owned(),
            );
            return false;
        };
        file.save_on_exit = self.workspace.session().save_on_exit();
        edit(&mut file);
        let written = ui_state::save(self.workspace.ui_state_path(), &file);
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code,
            path = %self.workspace.ui_state_path().display(),
            written,
            action = if written { "file_updated" } else { "file_not_written" },
            "a standing choice was written to the workspace file"
        );
        written
    }

    /// Write down the replay folder the trader just pointed the browser at,
    /// without disturbing anything else the workspace file holds.
    ///
    /// The same read-swap-write as [`Self::write_bookmarks`], and for the same
    /// reason: this is a standing choice, not a description of the screen, so
    /// it must not wait for a clean exit and must not drag the current
    /// arrangement into the file with it.
    pub(super) fn write_replay_folder(&mut self, folder: Option<&str>) {
        let stored = folder.map(str::to_owned);
        let written = self.edit_workspace_file("REPLAY_FOLDER_REMEMBERED", |file| {
            file.replay_folder = stored;
        });
        self.workspace.session_mut().note_write(written);
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "REPLAY_FOLDER_REMEMBERED",
            folder = folder.unwrap_or_default(),
            written,
            action = if folder.is_some() {
                "store_replay_folder"
            } else {
                "forget_replay_folder"
            },
            "the replay folder is now the one this workspace opens on"
        );
    }

    /// Write down the *day before* choice the trader just made, without
    /// disturbing anything else the workspace file holds.
    ///
    /// The same read-swap-write as [`Self::write_replay_folder`], and for the
    /// same reason: whether yesterday is on the chart is a standing choice,
    /// not a description of the screen, so it must not wait for a clean exit.
    pub(super) fn write_replay_day_before(&mut self, enabled: bool) {
        let written = self.edit_workspace_file("REPLAY_DAY_BEFORE_REMEMBERED", |file| {
            file.replay_day_before = Some(enabled);
        });
        self.workspace.session_mut().note_write(written);
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "REPLAY_DAY_BEFORE_REMEMBERED",
            enabled,
            written,
            action = if enabled {
                "join_day_before"
            } else {
                "chosen_day_only"
            },
            "the day before is now what this workspace opens recordings with"
        );
    }

    /// Write down the tools the trader just starred or unstarred, without
    /// disturbing anything else the workspace holds.
    ///
    /// The same read-swap-write as [`Self::write_replay_folder`], through the
    /// same [`Self::edit_workspace_file`], with two things of its own.
    ///
    /// First, `save_on_exit` does not gate it. That switch governs whether
    /// closing the window redefines the *arrangement* — which tabs open, how
    /// the panes are split. A starred tool is not an arrangement, and a trader
    /// who turned autosave off to stop their layout drifting has not asked to
    /// rebuild their rail every session.
    ///
    /// Second, `workspace_saved` is left alone. It answers "is there a startup
    /// arrangement to reset?", and starring a tool does not create one — a
    /// fresh install whose only saved thing is a star would otherwise light up
    /// a Reset entry that promises to forget a layout nobody ever saved.
    pub(super) fn write_favorites(&mut self) {
        // A run under `QUANTICK_TOOL_FAVORITES` is wearing a rail the harness
        // dressed it in, not one the trader curated, and the same guard the
        // replay folder gets applies: a validation run must not write a QA
        // list into the trader's workspace. The hook stages a screen; it does
        // not make choices on their behalf.
        if self.workspace.session().favorites_are_staged() {
            tracing::info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "TOOL_FAVORITES_NOT_WRITTEN",
                action = "staged_by_hook",
                "a run under QUANTICK_TOOL_FAVORITES does not write the rail down"
            );
            return;
        }
        let tools = self.starred_tool_ids();
        let count = tools.len();
        let written = self.edit_workspace_file("TOOL_FAVORITES_REMEMBERED", |file| {
            file.favorite_tools = tools;
        });
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "TOOL_FAVORITES_REMEMBERED",
            tools = count,
            written,
            action = if written { "favorites_written" } else { "favorites_not_written" },
            "the rail's pinned tools are now what this workspace opens on"
        );
    }

    /// Keep the window as it stands under `name`.
    ///
    /// A bookmark, not a startup setting: what the app opens on is untouched.
    /// The reason to name an arrangement is usually to have somewhere to come
    /// back *to*, and a "save this so I can return to it" that also redefined
    /// the opening screen would be the opposite of a safety net.
    ///
    /// An existing name is replaced rather than duplicated — that is what
    /// "save as" means everywhere else, and it spares the menu a list of five
    /// entries called "scalp".
    pub(super) fn save_named_workspace(&mut self, name: &str) {
        let Some(name) = ui_state::clean_workspace_name(name) else {
            self.note_workspace("A workspace needs a name".to_owned());
            return;
        };
        let (tabs, chrome) = self.capture_arrangement();
        let entry = ui_state::NamedArrangement {
            name: name.clone(),
            window: self.chrome.window_size,
            active_tab: self.active_tab,
            tabs,
            chrome: Some(chrome),
        };
        let replaced = match self
            .workspace
            .session_mut()
            .bookmarks_mut()
            .iter_mut()
            .find(|held| held.name == name)
        {
            Some(held) => {
                *held = entry;
                true
            }
            None => {
                self.workspace.session_mut().bookmarks_mut().push(entry);
                false
            }
        };
        let written = self.write_bookmarks();
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "UI_STATE_NAMED_SAVED",
            name = %name,
            replaced,
            saved = self.workspace.session().bookmarks().len(),
            written,
            action = if written { "bookmark_written" } else { "bookmark_not_written" },
            "named workspace saved"
        );
        self.note_workspace(if written {
            let verb = if replaced { "replaced" } else { "saved" };
            format!("Workspace \"{name}\" {verb} — reopen it from Workspace → Open")
        } else {
            format!("\"{name}\" could not be saved — see the log")
        });
    }

    /// Put the window back the way the bookmark called `name` recorded it.
    ///
    /// The saved markets are opened as new tabs and the tabs that were on
    /// screen are closed afterwards, rather than the reverse: `close_tab`
    /// refuses to close the last tab — a window with no market has nothing to
    /// draw — so growing before shrinking is what lets the whole strip be
    /// replaced. Closing goes through the same path a `Ctrl+W` takes, so a
    /// simulated position ends in the labeled, journaled flatten the
    /// paper-trading contract promises instead of vanishing with its tab.
    ///
    /// The startup workspace is left alone. Opening a bookmark is a thing you
    /// do to *this session*; making it the opening screen is `Save workspace`,
    /// one entry above.
    pub(super) fn open_named_workspace(&mut self, name: &str) {
        let Some(entry) = self
            .workspace
            .session()
            .bookmarks()
            .iter()
            .find(|held| held.name == name)
            .cloned()
        else {
            self.note_workspace(format!("No workspace called \"{name}\""));
            return;
        };
        if entry.tabs.is_empty() {
            // `restore` drops empty bookmarks at load, so this is only
            // reachable from a file edited under a running app.
            self.note_workspace(format!("\"{name}\" has no market left to open"));
            return;
        }
        let replaced = self.tabs.len();
        for saved in &entry.tabs {
            self.open_tab(
                saved.feed.clone(),
                saved.symbol.clone(),
                BarSpec::parse(&saved.flow_bars).ok(),
            );
            let context_intervals =
                saved_context_intervals(&saved.context_bars, saved.time_bars.as_deref());
            let opened = self.tabs.len() - 1;
            self.tabs[opened].restore_canvas(
                CanvasLayout::from(saved.layout),
                saved.split_fraction,
                saved.context_collapsed,
                saved.focus.map(|focus| focus.to_side(saved.focus_slot)),
                &context_intervals,
                LegendFold {
                    flow: saved.flow_legend_collapsed,
                    time: saved.time_legend_collapsed,
                },
            );
            self.tabs[opened].set_opening_layouts(saved.flow_layout, &saved.context_layouts);
        }
        for _ in 0..replaced {
            self.close_tab(0);
        }
        // The pinned section is deliberately untouched: the stars live beside
        // the chrome rather than inside it, and [`Self::restore_chrome`] does
        // not speak for them. A bookmark rearranges the cockpit; the tools the
        // trader keeps at hand are not part of the arrangement, and a bookmark
        // named before they starred anything used to wipe the rail on open.
        if let Some(chrome) = &entry.chrome {
            self.restore_chrome(chrome);
        }
        self.active_tab = entry.active_tab.min(self.tabs.len().saturating_sub(1));
        let config = self.config.clone();
        self.active_tab_mut().refresh_chip_label(&config);
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "UI_STATE_NAMED_OPENED",
            name = %name,
            tabs = self.tabs.len(),
            closed = replaced,
            active = self.active_tab,
            action = "replace_tab_strip",
            "named workspace opened"
        );
        self.note_workspace(format!(
            "Opened \"{name}\" — {} {}",
            self.tabs.len(),
            if self.tabs.len() == 1 {
                "chart tab"
            } else {
                "chart tabs"
            }
        ));
    }

    /// Forget the bookmark called `name`. The window on screen is untouched —
    /// deleting a bookmark throws away a way back, not the place you are.
    pub(super) fn delete_named_workspace(&mut self, name: &str) {
        let before = self.workspace.session().bookmarks().len();
        self.workspace
            .session_mut()
            .bookmarks_mut()
            .retain(|held| held.name != name);
        if self.workspace.session().bookmarks().len() == before {
            return;
        }
        let written = self.write_bookmarks();
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "UI_STATE_NAMED_DELETED",
            name = %name,
            remaining = self.workspace.session().bookmarks().len(),
            written,
            action = if written { "bookmark_forgotten" } else { "file_not_written" },
            "named workspace deleted"
        );
        self.note_workspace(if written {
            format!("Workspace \"{name}\" deleted")
        } else {
            format!("\"{name}\" could not be deleted — see the log")
        });
    }

    /// Forget the saved workspace: the next launch opens on the configured
    /// defaults. The window on screen is deliberately left alone — a trader
    /// resetting their *startup* layout mid-session has not asked to have the
    /// charts they are reading rearranged under them.
    pub(super) fn forget_workspace(&mut self) {
        // Reset clears the *startup* arrangement. The bookmarks survive it,
        // because coming back after a reset is the whole reason to name one:
        // deleting the safety net as part of the act it exists to undo would
        // be the single worst thing this menu could do.
        let bookmarks_kept = !self.workspace.session().bookmarks().is_empty();
        // The starred tools survive it too, and for a plainer reason: they
        // were never part of the arrangement being reset. Resetting a layout
        // is not asking to rebuild the rail by hand — and the same goes for
        // every other standing choice this file holds. The replay folder and
        // the Open-recent list are facts about this installation; the entry
        // resets a *layout* and must not quietly take them with it.
        let stars = self.starred_tool_ids();
        let kept = bookmarks_kept
            || !stars.is_empty()
            || !self.workspace.session().recent().is_empty()
            || self.replay_view.stored_pick().is_some();
        let bookmarks = self.workspace.session().bookmarks().to_vec();
        let stars_kept = !stars.is_empty();
        let forgotten = if kept {
            // Edited rather than rebuilt from the defaults: writing a fresh
            // `Workspace` would carry only what this function remembered to
            // thread through it, and the fields it forgot would be reset by
            // omission. Clearing the arrangement names what goes; everything
            // unnamed stays by construction.
            self.edit_workspace_file("UI_STATE_FORGOTTEN", |file| {
                file.tabs.clear();
                file.chrome = None;
                file.window = None;
                file.active_tab = 0;
                file.saved = bookmarks;
                file.favorite_tools = stars;
            })
        } else {
            ui_state::forget(self.workspace.ui_state_path())
        };
        // The file still exists while it holds standing choices, so Reset
        // stays available — it is now a no-op for the startup screen and the
        // entry says as much. A reset that *failed* leaves the old
        // arrangement on disk and so leaves the entry live: the trader has to
        // be able to try again, and telling them "nothing saved yet" while the
        // next launch still reopens the layout they discarded would be a lie.
        self.workspace
            .session_mut()
            .set_saved(if forgotten { kept } else { true });
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "UI_STATE_FORGOTTEN",
            path = %self.workspace.ui_state_path().display(),
            forgotten,
            bookmarks_kept = self.workspace.session().bookmarks().len(),
            favorites_kept = self.toolrail.favorites().len(),
            action = if forgotten { "open_on_config_defaults" } else { "workspace_kept" },
            "workspace reset"
        );
        // What survived is named, never left to be discovered. A trader who
        // resets a layout and is told nothing else assumes nothing else was
        // kept — and would go looking for stars that are still there.
        let survivors = match (bookmarks_kept, stars_kept) {
            (true, true) => format!(
                " {} saved {} and the starred tools kept.",
                self.workspace.session().bookmarks().len(),
                if self.workspace.session().bookmarks().len() == 1 {
                    "workspace"
                } else {
                    "workspaces"
                }
            ),
            (true, false) => format!(
                " {} saved {} kept.",
                self.workspace.session().bookmarks().len(),
                if self.workspace.session().bookmarks().len() == 1 {
                    "workspace"
                } else {
                    "workspaces"
                }
            ),
            (false, true) => " The starred tools are kept.".to_owned(),
            (false, false) => String::new(),
        };
        self.note_workspace(if forgotten {
            format!(
                "Startup layout reset — the next launch opens on the configured default.{survivors}"
            )
        } else {
            "Workspace could not be reset — see the log".to_owned()
        });
    }

    /// Take the window manager's own maximise, once, on the first frame.
    ///
    /// Through [`egui::ViewportCommand::Maximized`] rather than the viewport
    /// builder's `with_maximized`: eframe 0.29 does not honour that flag beside
    /// an `inner_size`, and a hook that silently opens a 1100×650 window while
    /// reporting success is worse than no hook — a validation run would
    /// photograph the wrong state and call it a pass. The command is the one
    /// the platform runs when a hand hits the title bar, which is the state
    /// this hook exists to reach.
    pub(super) fn apply_maximize_hook(&mut self, ctx: &egui::Context) {
        if !self.harness.take_maximize() {
            return;
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "WINDOW_MAXIMIZE_AUTOSTART",
            action = "maximize",
            "QUANTICK_WINDOW_MAXIMIZED asked for the maximised layout"
        );
    }

    /// Keep the window size the workspace would record, flush a popup the
    /// trader just re-parked, and take the exit save when the window is
    /// closing.
    ///
    /// **Per-frame cost**: two reads off the frame's own input state, a float
    /// compare and a `bool` test. No save is on this path — each of the three
    /// happens on one frame: the frame a drag ends, and the frame the close is
    /// requested, when the window is going away anyway.
    ///
    /// The size is tracked here rather than read at exit because by then the
    /// viewport has already been asked to close: what a workspace should
    /// remember is the window the trader was working in, not whatever the
    /// platform reports on the way out.
    pub(super) fn maintain_workspace(&mut self, ctx: &egui::Context) {
        let (size, closing) = ctx.input(|input| {
            let viewport = input.viewport();
            (
                viewport
                    .inner_rect
                    .map(|rect| [rect.width(), rect.height()]),
                viewport.close_requested(),
            )
        });
        if let Some(size) = size
            && size[0] > 0.0
            && size[1] > 0.0
        {
            self.chrome.window_size = Some(size);
        }
        // Where the trader parks the properties popup is kept the moment the
        // hand comes off it, without a trip through the Workspace menu: the
        // window they dragged out of the way is the window they expect back,
        // and a memory that only survived a clean exit would lose it to the
        // one session that ended badly. Only that one field is written — see
        // [`Self::write_inspector_position`] for why a drag must not adopt the
        // tab strip as a startup screen.
        //
        // Gated on the same switch the exit save is, because the menu already
        // promises it: "Off, only Save workspace changes what quantick opens
        // on." A trader who curates their startup layout by hand still gets the
        // position within the session — it is live state — and an explicit Save
        // still records it.
        //
        // The closing frame takes the exit save instead: it writes the whole
        // window, this position included, so running both would serialise the
        // same file twice on the way out.
        if closing && self.workspace.session().save_on_exit() {
            self.chrome.inspector_position_dirty = false;
            self.save_workspace("exit");
        } else if std::mem::take(&mut self.chrome.inspector_position_dirty)
            && self.workspace.session().save_on_exit()
        {
            self.write_inspector_position();
        }
    }

    /// Post a Workspace-menu answer through the window's one acknowledgement
    /// channel ([`Toast`]).
    ///
    /// No Undo: the file it replaced is gone, and `Reset startup layout` is
    /// the honest way back rather than a button that pretends otherwise.
    pub(super) fn note_workspace(&mut self, message: String) {
        self.surfaces.toast.note(message, Instant::now());
    }
}
