//! The paper simulator's settings owner: where its trades are saved and what
//! is persisted when the trader changes a ticket setting.
//!
//! Nothing here decides anything about a simulated trade -- that is
//! [`crate::paper`] and [`crate::paper_account`]. This owner holds the one
//! rule the window used to spell out four times: a ticket setting is
//! app-wide, so every tab is told and the sidecar (`paper-state.toml`) is
//! written, in that order. The window builds a [`PaperSettingsAdapter`] over
//! the two ports it needs and hands it a message; the adapter never sees the
//! rest of the app.

use std::path::PathBuf;

use quantick_paper::report::session_edit::{
    JournalEdit, JournalEditOutcome, JournalScope, SessionEditError, apply_journal_edit, edit_toast,
};
use quantick_session::paper::SharedSettings;

use super::arrangement_host::ArrangementHost;
use crate::paper_trading::PaperTrading;
use crate::workspace_store::WorkspaceStore;

pub(crate) use quantick_session::paper::SharedChange as PaperSettingsChange;

/// The paper settings' two ports: every tab's simulator, and the store that
/// knows where trades go and whether a folder dialog is open.
pub(crate) struct PaperSettingsAdapter<'a> {
    pub(super) tabs: &'a mut ArrangementHost,
    pub(super) workspace: &'a mut WorkspaceStore,
}

impl PaperSettingsAdapter<'_> {
    fn active(&self) -> &PaperTrading {
        &self.tabs[self.tabs.active_index()].paper
    }

    /// The trades folder and every file any tab is still journaling to.
    pub(crate) fn journal_scope(&self) -> JournalScope {
        journal_scope(self.tabs)
    }

    /// One session edit, as the ledger's gesture makes it.
    pub(crate) fn apply_journal_edit(
        &mut self,
        edit: &JournalEdit,
    ) -> Result<JournalEditOutcome, SessionEditError> {
        apply_journal_edit_across(self.tabs, edit)
    }

    /// Ask the operating system for a trades folder, off the UI thread —
    /// the panel's "choose where trades are saved". One dialog at a time.
    pub(crate) fn open_trades_dir_picker(&mut self) {
        if self.workspace.trades_dir_picker_open() {
            return;
        }
        let (sender, receiver) = std::sync::mpsc::channel();
        // Start where trades actually go right now — under an env override
        // that is the override's folder, not the stored base.
        let start = self.active().account().trades_dir().to_path_buf();
        std::thread::Builder::new()
            .name("quantick-trades-dir-picker".into())
            .spawn(move || {
                let mut dialog = rfd::FileDialog::new().set_title("Choose where trades are saved");
                if start.is_dir() {
                    dialog = dialog.set_directory(&start);
                }
                let _ = sender.send(dialog.pick_folder());
            })
            .expect("spawn trades-dir picker thread");
        self.workspace.open_trades_dir_picker(receiver);
    }

    /// Land the picked folder: every tab journals there from now on, and
    /// the choice is remembered across restarts (`paper-state.toml`) —
    /// files already written stay where they are.
    pub(crate) fn poll_trades_dir_picker(&mut self) {
        let Some(receiver) = self.workspace.trades_dir_picker() else {
            return;
        };
        let Ok(choice) = receiver.try_recv() else {
            return;
        };
        self.workspace.close_trades_dir_picker();
        if let Some(dir) = choice {
            self.set_trades_dir(dir);
        }
    }

    fn set_trades_dir(&mut self, dir: PathBuf) {
        let path = crate::paper_state::default_path();
        let mut state = crate::paper_state::load(&path);
        state.trades_dir = Some(dir.display().to_string());
        crate::paper_state::save(&path, &state);
        self.workspace.set_trades_dir(dir);
        for tab in self.tabs.iter_mut() {
            tab.paper
                .account_mut()
                .set_trades_dir(self.workspace.trades_dir().to_path_buf());
        }
    }

    /// Persist the active tab's copy of one shared setting and fan it out —
    /// one gesture, one meaning, every tab (the trades-dir rule). The same
    /// call whether a click or a capability changed it, so a named call
    /// leaves the durable trace a click does.
    pub(crate) fn persist(&mut self, change: PaperSettingsChange) {
        let shared = SharedSettings::of(&**self.active(), change);
        for tab in self.tabs.iter_mut() {
            shared.apply(&mut *tab.paper);
        }
        let path = crate::paper_state::default_path();
        let mut state = crate::paper_state::load(&path);
        shared.record(&mut state);
        crate::paper_state::save(&path, &state);
    }
}

/// The trades folder and every file any tab is still journaling to.
fn journal_scope(tabs: &ArrangementHost) -> JournalScope {
    let live = tabs
        .iter()
        .flat_map(|tab| tab.paper.account().session_journal_paths());
    JournalScope {
        dir: tabs[tabs.active_index()]
            .paper
            .account()
            .trades_dir()
            .to_path_buf(),
        live: live.cloned().collect(),
    }
}

/// One session edit, from a ledger gesture or a capability alike: every
/// tab's live file is refused, a done edit has every ledger and report
/// re-read the folder — a refused one changed nothing to re-read — and the
/// toast lands on the active tab either way.
pub(super) fn apply_journal_edit_across(
    tabs: &mut ArrangementHost,
    edit: &JournalEdit,
) -> Result<JournalEditOutcome, SessionEditError> {
    let outcome = apply_journal_edit(&journal_scope(tabs), edit);
    if outcome.is_ok() {
        for tab in tabs.iter_mut() {
            let (report, env) = tab.paper.account_mut().report_parts();
            report.journal_edited(&env);
        }
    }
    let index = tabs.active_index();
    tabs.runtime_mut(index)
        .paper
        .show_toast(edit_toast(&outcome));
    outcome
}
