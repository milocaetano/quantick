//! The workspace menus' state, apart from their pixels: the chip's menu, the
//! delete that asks twice and the dialog that asks for a name.

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
    Rename { from: String },
}

/// The dialog that asks for a workspace name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameDialog {
    pub mode: NameDialogMode,
    pub name: String,
    /// Why the last Enter was refused, shown under the field.
    pub refusal: Option<String>,
    /// The field takes the keyboard on the dialog's first frame.
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
    pub chip_menu: bool,
    pub armed_delete: Option<String>,
    pub dialog: Option<NameDialog>,
}

impl WorkspaceMenuState {
    pub fn toggle_chip_menu(&mut self) {
        self.chip_menu = !self.chip_menu;
        self.armed_delete = None;
    }

    pub fn close_chip_menu(&mut self) {
        self.chip_menu = false;
        self.armed_delete = None;
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

    /// Opened on the current name, which is what a rename edits.
    pub fn open_rename(&mut self, from: &str) {
        self.open_dialog(
            NameDialogMode::Rename {
                from: from.to_owned(),
            },
            from.to_owned(),
        );
    }

    /// Enter was refused: the dialog stays up and says why.
    pub fn refuse(&mut self, reason: String) {
        if let Some(dialog) = &mut self.dialog {
            dialog.refusal = Some(reason);
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
        state.toggle_chip_menu();
        state.click_delete("Scalp");
        state.close_chip_menu();
        assert!(!state.chip_menu);
        assert_eq!(state.armed_delete, None);
    }

    #[test]
    fn a_refusal_keeps_the_dialog_and_the_name_typed() {
        let mut state = WorkspaceMenuState::default();
        state.open_save_as();
        state.dialog.as_mut().unwrap().name = "Scalp".to_owned();
        state.refuse("a workspace is already called Scalp".to_owned());
        let dialog = state.dialog.as_ref().unwrap();
        assert_eq!(dialog.name, "Scalp");
        assert!(dialog.refusal.is_some());
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
