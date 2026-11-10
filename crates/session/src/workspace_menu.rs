//! The workspace menus' state, apart from their pixels: the status bar cell's
//! menu, the Manage workspaces dialog whose delete asks twice, the dialog that
//! asks for a name and the close that asks per tab holding edits.

use crate::close_queue::{CloseAnswer, CloseQueue, CloseScope};

/// What a workspace row or the dialog asked the window to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceCommand {
    /// Put the active tab on this workspace.
    Set(String),
    /// Write the active tab's edits into its workspace.
    SaveTo,
    /// Turn "auto save layout changes" on or off.
    SetAutosave(bool),
    BeginSaveAs,
    /// Ask the name of a new, empty workspace. Nothing changes before Enter.
    BeginNew,
    /// Ask a new name for this workspace.
    BeginRename(String),
    /// Open the Manage workspaces dialog.
    BeginManage,
    Delete(String),
    /// The dialog's Enter.
    SubmitDialog,
    /// Close these tabs, or exit, asking first per tab holding edits.
    Close(CloseScope, Vec<u64>),
    /// The close prompt's answer.
    AnswerClose(CloseAnswer),
}

/// What the name dialog is asking for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NameDialogMode {
    SaveAsNew,
    /// An empty recipe, made and applied only on Enter.
    New,
    Rename {
        from: String,
    },
}

/// The dialog that asks for a workspace name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameDialog {
    /// The tab whose menu asked: the dialog shows in that tab's window and
    /// Enter acts on that tab, whichever window has the focus.
    pub tab: u64,
    pub mode: NameDialogMode,
    pub name: String,
    /// Why the last Enter was refused, shown under the field.
    pub refusal: Option<String>,
    /// The field takes the keyboard on the dialog's first frame, and again
    /// after a refusal, so the trader retypes without reaching for the mouse.
    pub wants_focus: bool,
}

/// What a click on a row's Delete meant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteClick {
    /// The first click: the row now states the outcome and asks.
    Armed,
    /// The second click on the same workspace.
    Confirmed,
}

/// The Manage workspaces dialog: every workspace, with its Rename and Delete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManageDialog {
    /// The tab whose menu opened it: the dialog shows in that tab's window.
    pub tab: u64,
    /// The workspace whose Delete asks for its confirmation.
    pub armed: Option<String>,
}

impl ManageDialog {
    /// A click on `name`'s Delete. Arming another workspace disarms the first,
    /// so no confirmation ever lands on a name it did not read.
    pub fn click_delete(&mut self, name: &str) -> DeleteClick {
        if self.armed.as_deref() == Some(name) {
            self.armed = None;
            return DeleteClick::Confirmed;
        }
        self.armed = Some(name.to_owned());
        DeleteClick::Armed
    }
}

/// The menus' state across frames.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkspaceMenuState {
    /// The tab whose cell opened the menu, while it is open. The menu shows
    /// and acts in the window presenting that tab, so a torn-off window's
    /// cell never reaches another window's tab.
    pub chip_menu: Option<u64>,
    pub manage: Option<ManageDialog>,
    pub dialog: Option<NameDialog>,
    pub close: CloseQueue,
}

impl WorkspaceMenuState {
    /// The cell of `tab` was clicked, or its shortcut pressed: open the menu
    /// on that tab, or close it when it was already open there.
    pub fn toggle_chip_menu(&mut self, tab: u64) {
        self.chip_menu = (self.chip_menu != Some(tab)).then_some(tab);
    }

    pub fn close_chip_menu(&mut self) {
        self.chip_menu = None;
    }

    /// Open the Manage workspaces dialog in the window presenting `tab`.
    pub fn open_manage(&mut self, tab: u64) {
        self.manage = Some(ManageDialog { tab, armed: None });
    }

    pub fn close_manage(&mut self) {
        self.manage = None;
    }

    /// The window presenting `presented` (`None` on Home) holds `window`'s
    /// tabs. A menu open on one of them that is no longer the one presented
    /// closes, so it never pops back open when its tab returns. A menu on
    /// another window's tab is that window's business.
    pub fn follow_window(&mut self, presented: Option<u64>, window: &[u64]) {
        if let Some(open) = self.chip_menu
            && window.contains(&open)
            && presented != Some(open)
        {
            self.close_chip_menu();
        }
    }

    /// One frame of the cell in the window presenting `tab` among `window`'s
    /// tabs; `shown` is false on Home, where the cell is absent. A click or
    /// the shortcut (`toggled`) counts only where the cell stands. Answers
    /// whether the menu is open on `tab`.
    pub fn follow_cell(&mut self, tab: u64, shown: bool, window: &[u64], toggled: bool) -> bool {
        self.follow_window(shown.then_some(tab), window);
        if shown && toggled {
            self.toggle_chip_menu(tab);
        }
        self.chip_menu_on(tab)
    }

    /// Whether the menu is open on `tab`.
    #[must_use]
    pub fn chip_menu_on(&self, tab: u64) -> bool {
        self.chip_menu == Some(tab)
    }

    fn open_dialog(&mut self, tab: u64, mode: NameDialogMode, name: String) {
        self.dialog = Some(NameDialog {
            tab,
            mode,
            name,
            refusal: None,
            wants_focus: true,
        });
    }

    pub fn open_save_as(&mut self, tab: u64) {
        self.open_dialog(tab, NameDialogMode::SaveAsNew, String::new());
    }

    /// Asks the new workspace's name first; Esc closes it with nothing made.
    pub fn open_new(&mut self, tab: u64) {
        self.open_dialog(tab, NameDialogMode::New, String::new());
    }

    /// Opened on the current name, which is what a rename edits.
    pub fn open_rename(&mut self, tab: u64, from: &str) {
        self.open_dialog(
            tab,
            NameDialogMode::Rename {
                from: from.to_owned(),
            },
            from.to_owned(),
        );
    }

    /// Enter was refused: the dialog stays up, says why and keeps the keyboard.
    pub fn refuse(&mut self, reason: String) {
        if let Some(dialog) = &mut self.dialog {
            dialog.refusal = Some(reason);
            dialog.wants_focus = true;
        }
    }

    pub fn close_dialog(&mut self) {
        self.dialog = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn managing() -> ManageDialog {
        let mut state = WorkspaceMenuState::default();
        state.open_manage(7);
        state.manage.expect("opened")
    }

    #[test]
    fn delete_asks_twice_for_the_same_name() {
        let mut dialog = managing();
        assert_eq!(dialog.click_delete("Scalp"), DeleteClick::Armed);
        assert_eq!(dialog.click_delete("Scalp"), DeleteClick::Confirmed);
        assert_eq!(dialog.armed, None);
    }

    #[test]
    fn a_second_click_on_another_name_only_moves_the_arm() {
        let mut dialog = managing();
        dialog.click_delete("Scalp");
        assert_eq!(dialog.click_delete("Tape"), DeleteClick::Armed);
        assert_eq!(dialog.armed.as_deref(), Some("Tape"));
    }

    #[test]
    fn reopening_the_manage_dialog_disarms_the_delete() {
        let mut state = WorkspaceMenuState::default();
        state.open_manage(7);
        if let Some(dialog) = &mut state.manage {
            dialog.click_delete("Scalp");
        }
        state.close_manage();
        assert_eq!(state.manage, None);
        state.open_manage(7);
        assert_eq!(state.manage.expect("opened").armed, None);
    }

    #[test]
    fn the_menu_belongs_to_the_tab_whose_cell_opened_it() {
        let mut state = WorkspaceMenuState::default();
        state.toggle_chip_menu(7);
        assert!(state.chip_menu_on(7));
        assert!(!state.chip_menu_on(8), "another window's tab");
        state.toggle_chip_menu(8);
        assert!(state.chip_menu_on(8), "the other cell moves it");
        state.toggle_chip_menu(8);
        assert_eq!(state.chip_menu, None, "the same cell closes it");
    }

    #[test]
    fn a_menu_whose_tab_leaves_the_screen_closes_and_another_windows_stays() {
        let mut state = WorkspaceMenuState::default();
        state.toggle_chip_menu(7);
        state.follow_window(Some(9), &[9]);
        assert!(state.chip_menu_on(7), "tab 7 lives in another window");
        state.follow_window(Some(7), &[7, 8]);
        assert!(state.chip_menu_on(7), "still presented");
        state.follow_window(Some(8), &[7, 8]);
        assert_eq!(state.chip_menu, None, "its window moved to tab 8");
        state.toggle_chip_menu(7);
        state.follow_window(None, &[7, 8]);
        assert_eq!(state.chip_menu, None, "Home shows no cell");
    }

    #[test]
    fn the_cell_opens_only_where_it_stands() {
        let mut state = WorkspaceMenuState::default();
        assert!(!state.follow_cell(7, false, &[7], true), "Home");
        assert!(state.follow_cell(7, true, &[7], true));
        assert!(state.follow_cell(7, true, &[7], false), "stays open");
        assert!(!state.follow_cell(7, true, &[7], true), "a second press");
    }

    #[test]
    fn new_workspace_asks_an_empty_name_and_esc_leaves_no_dialog() {
        let mut state = WorkspaceMenuState::default();
        state.open_new(7);
        let dialog = state.dialog.as_ref().unwrap();
        assert_eq!(dialog.mode, NameDialogMode::New);
        assert!(dialog.name.is_empty());
        assert!(dialog.wants_focus);
        state.close_dialog();
        assert_eq!(state.dialog, None);
    }

    #[test]
    fn a_refusal_keeps_the_dialog_the_name_typed_and_the_focus() {
        let mut state = WorkspaceMenuState::default();
        state.open_save_as(7);
        let dialog = state.dialog.as_mut().unwrap();
        dialog.name = "Scalp".to_owned();
        dialog.wants_focus = false;
        state.refuse("a workspace is already called Scalp".to_owned());
        let dialog = state.dialog.as_ref().unwrap();
        assert_eq!(dialog.name, "Scalp");
        assert!(dialog.refusal.is_some());
        assert!(dialog.wants_focus, "the field takes the keyboard back");
    }

    #[test]
    fn rename_opens_on_the_current_name() {
        let mut state = WorkspaceMenuState::default();
        state.open_rename(7, "Scalp");
        let dialog = state.dialog.as_ref().unwrap();
        assert_eq!((dialog.tab, dialog.name.as_str()), (7, "Scalp"));
        assert_eq!(
            dialog.mode,
            NameDialogMode::Rename {
                from: "Scalp".to_owned()
            }
        );
    }
}
