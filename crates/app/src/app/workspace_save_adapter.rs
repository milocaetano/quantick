//! Executes workspace persistence effects over explicit arrangement/session/file ports.
use super::arrangement_adapter::ArrangementRead;
use crate::ui_state;
use eframe::egui;
use quantick_workspace::workspace_commit::{
    DocumentEdit, FrameFacts, FrameSave, WorkspaceCommitSession, WritePurpose,
};
use std::{path::Path, time::Instant};

pub(crate) struct WorkspaceRead<'a> {
    pub(super) arrangement: ArrangementRead<'a>,
    pub(super) session: &'a WorkspaceCommitSession,
    pub(super) replay_view: &'a crate::replay_view::ReplayView,
}
pub(crate) struct WorkspaceSaveAdapter<'a> {
    pub(super) arrangement: ArrangementRead<'a>,
    pub(super) session: &'a mut WorkspaceCommitSession,
    pub(super) path: &'a Path,
    pub(super) replay_view: &'a crate::replay_view::ReplayView,
    pub(super) toast: &'a mut crate::surfaces::ToastSurface,
}

impl WorkspaceRead<'_> {
    /// The window as it stands, in the form the workspace file records.
    ///
    /// Read off the live state rather than accumulated as it changes: the
    /// arrangement is a dozen fields spread over the tabs and the chrome, and
    /// a second copy maintained by every control that moves one of them would
    /// be a dozen chances to forget. Saving is rare and event-driven, so
    /// reading them all at once costs nothing anyone can see.
    pub(super) fn capture_workspace(&self) -> ui_state::Workspace {
        let (tabs, chrome) = self.arrangement.capture_arrangement();
        ui_state::Workspace::new(
            self.session.save_on_exit(),
            self.session.window_size(),
            self.arrangement.tabs.active_index(),
            tabs,
            Some(chrome),
        )
        // Every write rewrites the whole file, so the bookmarks have to ride
        // along or saving the startup screen would silently delete them.
        .with_saved(self.session.bookmarks().to_vec())
        // And the recent workspace files, for the same reason: a save that
        // dropped them would empty the Open-recent menu every time the
        // trader saved their layout.
        .with_recent(self.session.recent().to_vec())
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
        self.arrangement
            .toolrail
            .favorites()
            .iter()
            .map(|tool| tool.id().to_owned())
            .collect()
    }
}

impl WorkspaceSaveAdapter<'_> {
    /// Write the workspace and say so on the status bar.
    ///
    /// The notice is the point of the explicit action: a trader who arranges a
    /// cockpit and clicks Save wants to know it is kept, and "it looks the
    /// same" is not an answer. A failed write says *that* instead — being told
    /// "saved" and finding out at the next launch is the one outcome worth
    /// engineering against.
    pub(super) fn save_workspace(&mut self, reason: &'static str) {
        let workspace = self.read().capture_workspace();
        let saved = ui_state::save(self.path, &workspace);
        self.session.note_write(WritePurpose::Full, saved);
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "UI_STATE_SAVED",
            path = %self.path.display(),
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
        let position = self
            .arrangement
            .drawing_chrome
            .remembered_inspector_position();
        let Some(file) = self
            .session
            .prepare_inspector(ui_state::load(self.path), position)
        else {
            return false;
        };
        let saved = ui_state::save(self.path, &file);
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "UI_STATE_POPUP_POSITION_SAVED",
            path = %self.path.display(),
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
        let written =
            self.edit_workspace_file("UI_STATE_BOOKMARKS_WRITTEN", DocumentEdit::Bookmarks);
        self.session.note_write(WritePurpose::Bookmarks, written);
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
    fn edit_workspace_file(&mut self, event_code: &'static str, edit: DocumentEdit) -> bool {
        let Some(file) = self
            .session
            .prepare_edit(ui_state::load_for_edit(self.path), edit)
        else {
            self.note_workspace(
                "The workspace file could not be read, so it was left alone — see the log"
                    .to_owned(),
            );
            return false;
        };
        let written = ui_state::save(self.path, &file);
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code,
            path = %self.path.display(),
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
        let written = self.edit_workspace_file(
            "REPLAY_FOLDER_REMEMBERED",
            DocumentEdit::ReplayFolder(stored),
        );
        self.session
            .note_write(WritePurpose::ReplayPreference, written);
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
        let written = self.edit_workspace_file(
            "REPLAY_DAY_BEFORE_REMEMBERED",
            DocumentEdit::ReplayDayBefore(enabled),
        );
        self.session
            .note_write(WritePurpose::ReplayPreference, written);
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
        if self.session.favorites_are_staged() {
            tracing::info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "TOOL_FAVORITES_NOT_WRITTEN",
                action = "staged_by_hook",
                "a run under QUANTICK_TOOL_FAVORITES does not write the rail down"
            );
            return;
        }
        let tools = self.read().starred_tool_ids();
        let count = tools.len();
        let written =
            self.edit_workspace_file("TOOL_FAVORITES_REMEMBERED", DocumentEdit::Favorites(tools));
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
        let (tabs, chrome) = self.arrangement.capture_arrangement();
        let entry = ui_state::NamedArrangement {
            name: name.clone(),
            window: self.session.window_size(),
            active_tab: self.arrangement.tabs.active_index(),
            tabs,
            chrome: Some(chrome),
        };
        let replaced = self.session.keep_bookmark(entry);
        let written = self.write_bookmarks();
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "UI_STATE_NAMED_SAVED",
            name = %name,
            replaced,
            saved = self.session.bookmarks().len(),
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
    /// Forget the bookmark called `name`. The window on screen is untouched —
    /// deleting a bookmark throws away a way back, not the place you are.
    pub(super) fn delete_named_workspace(&mut self, name: &str) {
        if !self.session.delete_bookmark(name) {
            return;
        }
        let written = self.write_bookmarks();
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "UI_STATE_NAMED_DELETED",
            name = %name,
            remaining = self.session.bookmarks().len(),
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
        let bookmarks_kept = !self.session.bookmarks().is_empty();
        // The starred tools survive it too, and for a plainer reason: they
        // were never part of the arrangement being reset. Resetting a layout
        // is not asking to rebuild the rail by hand — and the same goes for
        // every other standing choice this file holds. The replay folder and
        // the Open-recent list are facts about this installation; the entry
        // resets a *layout* and must not quietly take them with it.
        let stars = self.read().starred_tool_ids();
        let kept = self
            .session
            .reset_keeps(!stars.is_empty(), self.replay_view.stored_pick().is_some());
        let stars_kept = !stars.is_empty();
        let forgotten = if kept {
            // Edited rather than rebuilt from the defaults: writing a fresh
            // `Workspace` would carry only what this function remembered to
            // thread through it, and the fields it forgot would be reset by
            // omission. Clearing the arrangement names what goes; everything
            // unnamed stays by construction.
            self.edit_workspace_file(
                "UI_STATE_FORGOTTEN",
                DocumentEdit::ClearStartup { favorites: stars },
            )
        } else {
            ui_state::forget(self.path)
        };
        // The file still exists while it holds standing choices, so Reset
        // stays available — it is now a no-op for the startup screen and the
        // entry says as much. A reset that *failed* leaves the old
        // arrangement on disk and so leaves the entry live: the trader has to
        // be able to try again, and telling them "nothing saved yet" while the
        // next launch still reopens the layout they discarded would be a lie.
        self.session.finish_reset(kept, forgotten);
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "UI_STATE_FORGOTTEN",
            path = %self.path.display(),
            forgotten,
            bookmarks_kept = self.session.bookmarks().len(),
            favorites_kept = self.arrangement.toolrail.favorites().len(),
            action = if forgotten { "open_on_config_defaults" } else { "workspace_kept" },
            "workspace reset"
        );
        // What survived is named, never left to be discovered. A trader who
        // resets a layout and is told nothing else assumes nothing else was
        // kept — and would go looking for stars that are still there.
        let survivors = match (bookmarks_kept, stars_kept) {
            (true, true) => format!(
                " {} saved {} and the starred tools kept.",
                self.session.bookmarks().len(),
                if self.session.bookmarks().len() == 1 {
                    "workspace"
                } else {
                    "workspaces"
                }
            ),
            (true, false) => format!(
                " {} saved {} kept.",
                self.session.bookmarks().len(),
                if self.session.bookmarks().len() == 1 {
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
        match self.session.frame(FrameFacts { size, closing }) {
            FrameSave::Wait => {}
            FrameSave::Full => self.save_workspace("exit"),
            FrameSave::Inspector => {
                self.write_inspector_position();
            }
        }
    }

    fn read(&self) -> WorkspaceRead<'_> {
        WorkspaceRead {
            arrangement: self.arrangement,
            session: self.session,
            replay_view: self.replay_view,
        }
    }
    fn note_workspace(&mut self, message: String) {
        self.toast.note(message, Instant::now());
    }
}
