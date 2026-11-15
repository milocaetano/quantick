//! Manage workspaces and the two create dialogs as named calls:
//! `workspace.dialog.open` puts Save as…, New workspace… or Manage
//! workspaces on screen for a tab, and `workspace.delete.undo` takes a
//! delete back inside its ten seconds. Recolour, rename and delete in
//! Manage are `workspace.colour.set`, `workspace.rename` and
//! `workspace.delete`; the create dialogs' Save is `workspace.save_as` or
//! `workspace.new` with their `colour`.

use quantick_control::registry::{CapabilityDescriptor, IdempotencyPolicy};
use quantick_control::wire::WireU64;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::layout::transient_descriptor;
use crate::readback::{EVERY_OPTIONAL_TEST, Readback, journal};
use crate::workspace::{MODULE_ID, WorkspaceColourDto};
use crate::workspace_book::writes_the_book;

pub const WORKSPACE_DIALOG_OPEN_CAPABILITY_ID: &str = "workspace.dialog.open";
pub const WORKSPACE_DELETE_UNDO_CAPABILITY_ID: &str = "workspace.delete.undo";
/// A workspace dialog came on screen.
pub const WORKSPACE_DIALOG_OPENED_EVENT_KIND: &str = "workspace.dialog_opened";
/// A deleted workspace came back, with its colour and its tabs.
pub const WORKSPACE_RESTORED_EVENT_KIND: &str = "workspace.restored";
/// The `details.code` of an Undo after its ten seconds.
pub const UNDO_EXPIRED_CODE: &str = "workspace.undo_expired";

/// Which dialog to open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceDialog {
    /// Save as new workspace: the tab's copy under a new name.
    SaveAs,
    /// New workspace: an empty recipe.
    New,
    /// Manage workspaces.
    Manage,
}

impl WorkspaceDialog {
    /// The word a call and the result name it by.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::SaveAs => "save_as",
            Self::New => "new",
            Self::Manage => "manage",
        }
    }
}

/// Open a workspace dialog on a tab.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceDialogOpenInput {
    pub mode: WorkspaceDialog,
    /// The tab. Left out, the one on screen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_id: Option<WireU64>,
}

/// The dialog now on screen.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
pub struct WorkspaceDialogOpenResult {
    /// `save_as`, `new` or `manage`.
    pub dialog: String,
    pub title: String,
    pub tab_id: WireU64,
    /// The create dialogs' suggested name, selected in the field.
    pub name: Option<String>,
    /// The create dialogs' preselected colour: the next free one.
    pub colour: Option<WorkspaceColourDto>,
}

/// Take back a recent delete.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceDeleteUndoInput {
    pub name: String,
}

/// The workspace back, its colour and the tabs moved back.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
pub struct WorkspaceDeleteUndoResult {
    pub workspace: String,
    /// `false` for a repeat after the Undo landed.
    pub changed: bool,
    pub colour: WorkspaceColourDto,
    /// Its own colour was taken meanwhile, so it took the next free one.
    pub recoloured: bool,
    /// The tabs moved back: still open and still on Default.
    pub tabs: Vec<WireU64>,
}

pub fn workspace_dialog_open_descriptor() -> CapabilityDescriptor {
    let mut descriptor = transient_descriptor::<WorkspaceDialogOpenInput, WorkspaceDialogOpenResult>(
        WORKSPACE_DIALOG_OPEN_CAPABILITY_ID,
        MODULE_ID,
        "Open a workspace dialog",
        "Puts a workspace dialog on screen for a tab (left out, the one on screen), as the workspace menu's entries do, and journals workspace.dialog_opened: save_as (Save as new workspace) or new (New workspace), each with its suggested name selected and the palette open on the next free colour, which the answer names; or manage (Manage workspaces: one row per workspace with its colour chip, name, tab count, rename and delete). Nothing is written to the workspaces: Save is workspace.save_as or workspace.new with colour, and Manage's actions are workspace.colour.set, workspace.rename, workspace.delete and workspace.delete.undo. An unknown tab is refused with workspace.unknown_tab.",
        "Opening a dialog twice leaves one open, the create dialogs reset to their suggested name and colour.",
    );
    descriptor.expected_cost.max_response_bytes = Some(2048);
    descriptor
}

pub fn workspace_delete_undo_descriptor() -> CapabilityDescriptor {
    writes_the_book(transient_descriptor::<
        WorkspaceDeleteUndoInput,
        WorkspaceDeleteUndoResult,
    >(
        WORKSPACE_DELETE_UNDO_CAPABILITY_ID,
        MODULE_ID,
        "Undo a workspace delete",
        "Takes back a workspace.delete within 10 seconds of it (its answer's undo_until_unix_ms), as Manage workspaces' Undo and Ctrl+Z do: the workspace comes back with its recipe and colour, and the tabs the delete moved that are still open and still on Default move back. When its colour was taken meanwhile it takes the next free one, named in colour with recoloured true. Refused with workspace.undo_expired after the 10 seconds, workspace.unknown with no recent delete of that name, and workspace.name_taken when a workspace took the name meanwhile.",
        "A repeat after the Undo landed answers changed false and writes nothing.",
    ))
}

/// How a client reconciles an interrupted call.
pub const READBACKS: &[Readback] = &[
    journal(
        WORKSPACE_DIALOG_OPEN_CAPABILITY_ID,
        IdempotencyPolicy::Optional,
        WORKSPACE_DIALOG_OPENED_EVENT_KIND,
        "payload.dialog",
        "an event after the pre-call cursor names the requested dialog",
        &[EVERY_OPTIONAL_TEST],
    ),
    journal(
        WORKSPACE_DELETE_UNDO_CAPABILITY_ID,
        IdempotencyPolicy::Optional,
        WORKSPACE_RESTORED_EVENT_KIND,
        "payload.workspace",
        "an event after the pre-call cursor names the restored workspace",
        &[EVERY_OPTIONAL_TEST],
    ),
];

/// Published schemas of the dialog and Undo calls.
pub fn documents() -> Vec<(&'static str, serde_json::Value)> {
    use quantick_control::schema::generated_schema as schema;
    vec![
        (
            "workspace-dialog-open-input-v1.schema.json",
            schema::<WorkspaceDialogOpenInput>(),
        ),
        (
            "workspace-dialog-open-result-v1.schema.json",
            schema::<WorkspaceDialogOpenResult>(),
        ),
        (
            "workspace-delete-undo-input-v1.schema.json",
            schema::<WorkspaceDeleteUndoInput>(),
        ),
        (
            "workspace-delete-undo-result-v1.schema.json",
            schema::<WorkspaceDeleteUndoResult>(),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_dialog_call_names_one_of_three_modes() {
        let read = |value| serde_json::from_value::<WorkspaceDialogOpenInput>(value);
        for (word, mode) in [
            ("save_as", WorkspaceDialog::SaveAs),
            ("new", WorkspaceDialog::New),
            ("manage", WorkspaceDialog::Manage),
        ] {
            let input = read(json!({ "mode": word })).unwrap();
            assert_eq!((input.mode, input.tab_id), (mode, None));
            assert_eq!(mode.word(), word);
        }
        let on = read(json!({"mode": "manage", "tab_id": "7"})).unwrap();
        assert_eq!(on.tab_id.map(WireU64::get), Some(7));
        assert!(read(json!({"mode": "rename"})).is_err());
        assert!(read(json!({})).is_err());
    }

    #[test]
    fn the_undo_call_takes_a_name() {
        let input: WorkspaceDeleteUndoInput =
            serde_json::from_value(json!({"name": "Scalp"})).unwrap();
        assert_eq!(input.name, "Scalp");
        assert!(serde_json::from_value::<WorkspaceDeleteUndoInput>(json!({})).is_err());
    }
}
