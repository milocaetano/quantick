//! Bundle picker, export, import and reload orchestration.
//!
//! WorkspaceSaveAdapter owns arrangement persistence and standing edits;
//! these remaining bundle operations keep their existing flush/install/reload
//! order. Maximize remains with its existing harness owner. The one-call toast
//! helper is shared by unrelated feature notices without borrowing save state.

use crate::ui_state::WorkspaceExt;
use std::time::Instant;

use eframe::egui;

use crate::drawings;
use crate::indicators::preset_file;
use crate::symbols_file;
use crate::ui_state;
use crate::workspace_store::WorkspacePick;

use super::QuantickApp;
impl QuantickApp {
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
        self.workspace_save_adapter().save_workspace("export");
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
                self.workspace
                    .session_mut()
                    .visit(path.to_string_lossy().into_owned());
                self.refresh_recent_workspaces();
                self.workspace_save_adapter()
                    .save_workspace("export_recent");
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
                self.workspace
                    .session_mut()
                    .visit(path.to_string_lossy().into_owned());
                self.refresh_recent_workspaces();
                // The recent list lives in the workspace file the import just
                // replaced, so it has to be written back after the reload —
                // otherwise opening a file would forget that it was opened.
                self.workspace_save_adapter()
                    .save_workspace("import_recent");
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
        self.arrangement_adapter().restore_workspace(workspace);
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
        let existing = crate::workspace_bundle::existing_recent(self.workspace.session().recent());
        self.workspace.set_recent_on_disk(existing);
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

    /// Post a Workspace-menu answer through the window's one acknowledgement
    /// channel ([`Toast`]).
    ///
    /// No Undo: the file it replaced is gone, and `Reset startup layout` is
    /// the honest way back rather than a button that pretends otherwise.
    pub(super) fn note_workspace(&mut self, message: String) {
        self.surfaces.toast.note(message, Instant::now());
    }
}
