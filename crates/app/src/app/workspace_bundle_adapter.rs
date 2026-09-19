//! Workspace bundle file actions: real menu rendering and ordered runtime effects.
//! The headless bundle transaction owns capture/install policy; this owner
//! renders its existing controls and executes effects over explicit app ports.
use crate::indicators::preset_file;
use crate::ui_state::WorkspaceExt;
use crate::{drawings, symbols_file, ui_state};

pub(super) fn report_picker_lost(toast: &mut crate::surfaces::ToastSurface) {
    tracing::warn!(target: "quantick::app", schema_version = 1_u8,
        event_code = "WORKSPACE_PICKER_LOST", action = "picker_reset",
        "the file dialog closed without an answer");
    toast.note(
        "The file chooser could not open — see the log. Try again.",
        std::time::Instant::now(),
    );
}

pub(crate) struct WorkspaceBundleAdapter<'a> {
    /// The tab strip and chrome ports an import restores, borrowed once.
    pub(super) arrangement: super::arrangement_adapter::ArrangementAdapter<'a>,
    pub(super) replay_view: &'a crate::replay_view::ReplayView,
    pub(super) layout_rename: &'a mut Option<super::chrome::LayoutRename>,
    pub(super) layout_delete_confirm: &'a mut Option<crate::layouts::LayoutId>,
    pub(super) added_symbols: &'a mut crate::symbols_file::AddedSymbols,
    pub(super) drawing_presets: &'a mut crate::drawings::presets::PresetStore,
    pub(super) footprint_config: &'a mut crate::footprint_config::FootprintConfig,
    pub(super) footprint_settings: &'a mut crate::surfaces::FootprintSettingsSurface,
}

impl WorkspaceBundleAdapter<'_> {
    pub(super) fn show_file_actions(&mut self, ui: &mut eframe::egui::Ui) {
        // Files, named apart from the two groups above again:
        // those live inside quantick, these are documents the
        // trader owns, can copy, back up and carry to another
        // machine. That is the difference the wording carries.
        let tab = &self.arrangement.tabs[self.arrangement.tabs.active_index()];
        self.arrangement.workspace.picker_mut().show_open_actions(
            ui,
            &tab.symbol,
            tab.flow_pane.state.spec(),
        );
        // Read off the field, not the filesystem: this body
        // runs every frame the menu is open.
        let mut reopen: Option<std::path::PathBuf> = None;
        ui.add_enabled_ui(
            !self.arrangement.workspace.recent_on_disk().is_empty(),
            |ui| {
                ui.menu_button("Open recent", |ui| {
                    for path in self.arrangement.workspace.recent_on_disk() {
                        if ui
                            .button(crate::workspace_bundle::recent_label(path))
                            // The same warning the bookmark list
                            // carries: this replaces the cockpit,
                            // and a trader mid-tape has to read
                            // that before the click, not after.
                            .on_hover_text(format!(
                                "Replaces the cockpit on screen\n{}",
                                path.display()
                            ))
                            .clicked()
                        {
                            reopen = Some(path.clone());
                            ui.close_menu();
                        }
                    }
                })
                .response
                .on_disabled_hover_text("No workspace files opened yet");
            },
        );
        if let Some(path) = reopen {
            self.import_workspace_from(&path);
        }
        if ui
            .button("Show where it's saved")
            .on_hover_text(
                "Open the folder quantick keeps your cockpit in, so you can see \
             it and back it up",
            )
            .clicked()
        {
            self.reveal_cockpit_home();
            ui.close_menu();
        }
    }

    fn save_adapter(&mut self) -> super::workspace_save_adapter::WorkspaceSaveAdapter<'_> {
        let replay_view = self.replay_view;
        self.arrangement.reborrow().into_save(replay_view)
    }
    fn layout_adapter(&mut self) -> super::layout_wiring::LayoutAdapter<'_> {
        super::layout_wiring::LayoutAdapter {
            active: self.arrangement.tabs.active_index(),
            tabs: self.arrangement.tabs,
            indicators: self.arrangement.indicators,
            store: self.arrangement.workspace.layouts_mut(),
            drawing_chrome: self.arrangement.drawing_chrome,
            toast: self.arrangement.toast,
            rename: self.layout_rename,
            delete_confirm: self.layout_delete_confirm,
        }
    }
    fn maintain_chart_layers(&mut self) {
        let active = self.arrangement.tabs.active_index();
        crate::chart_layers::maintain(
            self.arrangement.workspace,
            self.arrangement.tabs.id_at(active),
            &self.arrangement.tabs[active].flow_pane,
            self.arrangement.style,
        );
    }
    fn restore_chart_layers(&mut self) {
        let active = self.arrangement.tabs.active_index();
        crate::chart_layers::restore(
            self.arrangement.workspace,
            self.arrangement.tabs,
            active,
            self.arrangement.style,
        );
    }
    fn note_workspace(&mut self, message: String) {
        self.arrangement
            .toast
            .note(message, std::time::Instant::now());
    }
    /// Route the file dialog's answer: a chosen path runs the intent the
    /// dialog was opened for, a lost dialog is reported once.
    pub(crate) fn poll_picker(&mut self) {
        use crate::workspace_picker::PickerOutcome;
        match self.arrangement.workspace.picker_mut().poll() {
            PickerOutcome::Idle | PickerOutcome::Cancelled => {}
            PickerOutcome::Lost => report_picker_lost(self.arrangement.toast),
            PickerOutcome::Chosen { intent, path } => self.dispatch(intent, &path),
        }
    }
    pub(super) fn dispatch(
        &mut self,
        intent: crate::workspace_picker::WorkspacePick,
        path: &std::path::Path,
    ) {
        match intent {
            crate::workspace_picker::WorkspacePick::Export => self.export_workspace_to(path),
            crate::workspace_picker::WorkspacePick::Import => self.import_workspace_from(path),
        }
    }
    /// Write every store that is still only in memory, so a bundle captured
    /// next describes the screen rather than the last flush.
    fn flush_cockpit_stores(&mut self) {
        // The layouts file is written debounced, off the frame path. An
        // export is the one moment worth paying it immediately, or the
        // bundle would carry the layouts as they stood a second ago.
        self.layout_adapter().flush_layouts();
        self.maintain_chart_layers();
    }

    /// Export the whole cockpit to one file, and say what happened.
    pub(super) fn export_workspace_to(&mut self, path: &std::path::Path) {
        // Here rather than before the dialog: these two writes are how the
        // bundle comes to describe the screen instead of the last flush, and
        // doing them up front meant a trader who pressed Cancel had still
        // silently redefined what the app opens on. It also keeps the harness
        // hook on exactly the menu's path.
        self.save_adapter().save_workspace("export");
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
                self.arrangement
                    .workspace
                    .session_mut()
                    .visit(path.to_string_lossy().into_owned());
                self.refresh_recent_workspaces();
                self.save_adapter().save_workspace("export_recent");
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
        self.layout_adapter().flush_layouts();
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
                self.arrangement
                    .workspace
                    .session_mut()
                    .visit(path.to_string_lossy().into_owned());
                self.refresh_recent_workspaces();
                // The recent list lives in the workspace file the import just
                // replaced, so it has to be written back after the reload —
                // otherwise opening a file would forget that it was opened.
                self.save_adapter().save_workspace("import_recent");
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
        *self.added_symbols = symbols_file::load(self.arrangement.workspace.symbols_path());
        *self.drawing_presets = drawings::presets::PresetStore::load_from(
            drawings::presets::PresetStore::default_path(),
        );
        *self.footprint_config =
            crate::footprint_config::load(self.arrangement.workspace.footprint_settings_path());
        self.footprint_settings.reload_presets();
        self.arrangement.indicators.indicator_presets =
            preset_file::PresetStore::load(self.arrangement.workspace.indicator_presets_path());

        // The tab strip first, and *before* the indicators: the restore adds
        // each indicator to whatever pane is focused right now, so the tabs
        // have to be the imported ones — and the focus on the pane the file
        // describes — before a single indicator is added. Getting this order
        // wrong puts a trader's imported indicators on the tab they happened
        // to be looking at, or on its time pane.
        let workspace = ui_state::load(self.arrangement.workspace.ui_state_path())
            .restore(&(*self.arrangement.config).clone());
        self.arrangement.restore_workspace(workspace);
        self.restore_chart_layers();

        // The layouts come last, once the tabs are the imported ones: every
        // pane is stripped and re-seeded from the imported file.
        self.layout_adapter().reload_layouts(imported);
    }

    /// Work out which remembered workspace files are still there.
    ///
    /// Called when the list changes, never from the menu body — see
    /// [`Self::recent_on_disk`]. The stored list keeps every entry: a file on
    /// a drive that is merely unplugged today comes back when it is plugged
    /// in, and only the menu is filtered.
    pub(super) fn refresh_recent_workspaces(&mut self) {
        let existing =
            crate::workspace_bundle::existing_recent(self.arrangement.workspace.session().recent());
        self.arrangement.workspace.set_recent_on_disk(existing);
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
}

impl super::arrangement_adapter::ArrangementAdapter<'_> {
    /// The same ports for a shorter borrow, so an owner that holds the
    /// adapter can hand one out without listing its fields again.
    pub(super) fn reborrow(&mut self) -> super::arrangement_adapter::ArrangementAdapter<'_> {
        super::arrangement_adapter::ArrangementAdapter {
            tabs: self.tabs,
            config: self.config,
            style: self.style,
            pane_ids: self.pane_ids,
            workspace: self.workspace,
            indicators: self.indicators,
            #[cfg(any(feature = "scenario-harness", test))]
            harness: self.harness,
            toolrail: self.toolrail,
            tz: self.tz,
            dock: self.dock,
            show_perf: self.show_perf,
            record_deals: self.record_deals,
            history: self.history,
            drawing_chrome: self.drawing_chrome,
            toast: self.toast,
        }
    }
}
