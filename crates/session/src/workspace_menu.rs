//! The workspace menus' state, apart from their pixels: the status bar cell's
//! menu, the delete that asks twice and the dialog that asks for a name.

/// What a workspace row or the dialog asked the window to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceCommand {
    /// Put the active tab on this workspace.
    Set(String),
    /// Write the active tab's edits into its workspace.
    SaveTo,
    /// Drop the active tab's edits.
    Revert,
    BeginSaveAs,
    /// Ask the name of a new, empty workspace. Nothing changes before Enter.
    BeginNew,
    /// Ask a new name for this workspace.
    BeginRename(String),
    Delete(String),
    /// The dialog's Enter.
    SubmitDialog,
}

/// What the name dialog is asking for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NameDialogMode {
    SaveAsNew,
    /// An empty recipe, made and applied only on Enter.
    New,
    Rename { from: String },
}

/// The dialog that asks for a workspace name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameDialog {
    pub mode: NameDialogMode,
    pub name: String,
    /// Why the last Enter was refused, shown under the field.
    pub refusal: Option<String>,
    /// The field takes the keyboard on the dialog's first frame, and again
    /// after a refusal, so the trader retypes without reaching for the mouse.
    pub wants_focus: bool,
}

/// What a click on the delete row meant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteClick {
    /// The first click: the row now states the outcome.
    Armed,
    /// The second click on the same workspace.
    Confirmed,
}

/// The menus' state across frames.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkspaceMenuState {
    /// The tab whose cell opened the menu, while it is open. The menu shows
    /// and acts in the window presenting that tab, so a torn-off window's
    /// cell never reaches another window's tab.
    pub chip_menu: Option<u64>,
    pub armed_delete: Option<String>,
    pub dialog: Option<NameDialog>,
}

impl WorkspaceMenuState {
    /// The cell of `tab` was clicked, or its shortcut pressed: open the menu
    /// on that tab, or close it when it was already open there.
    pub fn toggle_chip_menu(&mut self, tab: u64) {
        self.chip_menu = (self.chip_menu != Some(tab)).then_some(tab);
        self.armed_delete = None;
    }

    pub fn close_chip_menu(&mut self) {
        self.chip_menu = None;
        self.armed_delete = None;
    }

    /// Whether the menu is open on `tab`.
    #[must_use]
    pub fn chip_menu_on(&self, tab: u64) -> bool {
        self.chip_menu == Some(tab)
    }

    /// A click on the delete row of `name`. Arming another workspace disarms
    /// the first, so no second click ever lands on a name it did not read.
    pub fn click_delete(&mut self, name: &str) -> DeleteClick {
        if self.armed_delete.as_deref() == Some(name) {
            self.armed_delete = None;
            return DeleteClick::Confirmed;
        }
        self.armed_delete = Some(name.to_owned());
        DeleteClick::Armed
    }

    fn open_dialog(&mut self, mode: NameDialogMode, name: String) {
        self.dialog = Some(NameDialog {
            mode,
            name,
            refusal: None,
            wants_focus: true,
        });
    }

    pub fn open_save_as(&mut self) {
        self.open_dialog(NameDialogMode::SaveAsNew, String::new());
    }

    /// Asks the new workspace's name first; Esc closes it with nothing made.
    pub fn open_new(&mut self) {
        self.open_dialog(NameDialogMode::New, String::new());
    }

    /// Opened on the current name, which is what a rename edits.
    pub fn open_rename(&mut self, from: &str) {
        self.open_dialog(
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

    #[test]
    fn delete_asks_twice_for_the_same_name() {
        let mut state = WorkspaceMenuState::default();
        assert_eq!(state.click_delete("Scalp"), DeleteClick::Armed);
        assert_eq!(state.click_delete("Scalp"), DeleteClick::Confirmed);
        assert_eq!(state.armed_delete, None);
    }

    #[test]
    fn a_second_click_on_another_name_only_moves_the_arm() {
        let mut state = WorkspaceMenuState::default();
        state.click_delete("Scalp");
        assert_eq!(state.click_delete("Tape"), DeleteClick::Armed);
        assert_eq!(state.armed_delete.as_deref(), Some("Tape"));
    }

    #[test]
    fn closing_the_chip_menu_disarms_the_delete() {
        let mut state = WorkspaceMenuState::default();
        state.toggle_chip_menu(7);
        state.click_delete("Scalp");
        state.close_chip_menu();
        assert_eq!(state.chip_menu, None);
        assert_eq!(state.armed_delete, None);
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
    fn new_workspace_asks_an_empty_name_and_esc_leaves_no_dialog() {
        let mut state = WorkspaceMenuState::default();
        state.open_new();
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
        state.open_save_as();
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
        state.open_rename("Scalp");
        let dialog = state.dialog.as_ref().unwrap();
        assert_eq!(dialog.name, "Scalp");
        assert_eq!(
            dialog.mode,
            NameDialogMode::Rename {
                from: "Scalp".to_owned()
            }
        );
    }
}
