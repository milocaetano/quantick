//! The workspace book as named calls: which workspace a tab uses, and the
//! saved recipes (`docs/ux/window-menu-and-workspace-tabs.md`).
//!
//! A refusal carries `control.invalid_request` with the reason and a stable
//! `details.code`; a write that did not reach the disk carries
//! `control.capability_unavailable` with [`WRITE_FAILED_CODE`] and changes
//! nothing, as does a tab still opening, with [`TAB_NOT_READY_CODE`]. A tab id
//! no open tab has is refused with `workspace.unknown_tab`. A repeat answers
//! `changed: false` and journals nothing, bar a delete, which is refused with
//! `workspace.unknown`.

use quantick_control::id::PermissionId;
use quantick_control::registry::{CapabilityDescriptor, EffectPersistence, IdempotencyPolicy};
use quantick_control::wire::WireU64;
use quantick_control_host::authority::{
    COCKPIT_DESK_PERMISSION_ID, COCKPIT_PERMISSION_ID, OBSERVE_PERMISSION_ID, ReadSpec,
    read_descriptor,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::layout::transient_descriptor;
use crate::readback::{EVERY_OPTIONAL_TEST, Readback, journal};
use crate::workspace::MODULE_ID;

pub const TAB_WORKSPACE_SET_CAPABILITY_ID: &str = "tab.workspace.set";
pub const WORKSPACE_SAVE_TO_CAPABILITY_ID: &str = "workspace.save_to";
pub const WORKSPACE_SAVE_AS_CAPABILITY_ID: &str = "workspace.save_as";
pub const WORKSPACE_NEW_CAPABILITY_ID: &str = "workspace.new";
pub const WORKSPACE_REVERT_CAPABILITY_ID: &str = "workspace.revert";
pub const WORKSPACE_RENAME_CAPABILITY_ID: &str = "workspace.rename";
pub const WORKSPACE_DELETE_CAPABILITY_ID: &str = "workspace.delete";
pub const WORKSPACE_LIST_CAPABILITY_ID: &str = "workspace.list";
pub const WORKSPACE_AUTOSAVE_SET_CAPABILITY_ID: &str = "workspace.autosave.set";
pub const TAB_CLOSE_CAPABILITY_ID: &str = "tab.close";

/// A tab moved to another workspace.
pub const TAB_WORKSPACE_SET_EVENT_KIND: &str = "workspace.tab_set";
/// A recipe reached the disk, by Save to or Save as.
pub const WORKSPACE_SAVED_EVENT_KIND: &str = "workspace.saved";
/// A new, empty workspace reached the disk and a tab moved onto it.
pub const WORKSPACE_CREATED_EVENT_KIND: &str = "workspace.created";
/// A tab's edits were dropped.
pub const WORKSPACE_REVERTED_EVENT_KIND: &str = "workspace.reverted";
/// A new name reached the disk.
pub const WORKSPACE_RENAMED_EVENT_KIND: &str = "workspace.renamed";
/// A deletion reached the disk.
pub const WORKSPACE_DELETED_EVENT_KIND: &str = "workspace.deleted";
/// Journaled when autosave turns on or off.
pub const WORKSPACE_AUTOSAVE_SET_EVENT_KIND: &str = "workspace.autosave_set";
/// Journaled when `tab.close` closes a tab.
pub const TAB_CLOSED_EVENT_KIND: &str = "workspace.tab_closed";
/// `tab.close` without `save` on a tab holding edits, autosave off: nothing
/// closes, and `details` names the workspace, the followers and the changes.
pub const UNSAVED_EDITS_CODE: &str = "workspace.unsaved_edits";
/// `workspace.revert` is gone; the refusal names `workspace.save_to` and
/// `workspace.autosave.set`.
pub const REVERT_REMOVED_CODE: &str = "workspace.revert_removed";
/// The last tab of the window cannot close.
pub const LAST_TAB_CODE: &str = "workspace.last_tab";

/// The `details.code` of a write that did not reach the disk.
pub const WRITE_FAILED_CODE: &str = "workspace.write_failed";
/// The `details.code` of a call naming a tab whose charts are still being
/// built: `control.capability_unavailable`, retryable, nothing changed.
pub const TAB_NOT_READY_CODE: &str = "workspace.tab_not_ready";

const RESULT_MAX_BYTES: usize = 4096;

/// Put a tab on a workspace.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TabWorkspaceSetInput {
    /// The tab. Left out, the one on screen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_id: Option<WireU64>,
    pub name: String,
}

/// The tab, and whether unsaved edits were dropped by the switch.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
pub struct TabWorkspaceSetResult {
    pub tab_id: WireU64,
    pub workspace: String,
    /// `false` for a repeat, which keeps any edits.
    pub changed: bool,
    pub dropped_edits: bool,
}

/// A call about one tab's copy.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceTabInput {
    /// The tab. Left out, the one on screen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_id: Option<WireU64>,
}

/// Make a new workspace of a tab's copy.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceSaveAsInput {
    /// The tab. Left out, the one on screen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_id: Option<WireU64>,
    pub name: String,
}

/// Make a new, empty workspace and put a tab on it.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceNewInput {
    /// The tab. Left out, the one on screen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_id: Option<WireU64>,
    pub name: String,
}

/// The new workspace as stored, and whether the tab's edits were dropped.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
pub struct WorkspaceNewResult {
    pub tab_id: WireU64,
    pub workspace: String,
    pub dropped_edits: bool,
}

/// What a save did.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
pub struct WorkspaceSaveResult {
    pub workspace: String,
    /// `false` for a repeat, which writes nothing.
    pub changed: bool,
    /// The other tabs whose charts followed the new recipe. A tab with edits
    /// of its own keeps them and is not listed.
    pub tabs_changed: Vec<WireU64>,
}

/// What the removed revert would have answered; it now always refuses.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
pub struct WorkspaceRevertResult {
    pub tab_id: WireU64,
    pub workspace: String,
    pub changed: bool,
}

/// Turn "auto save layout changes" on or off.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceAutosaveInput {
    pub on: bool,
}

/// The setting as it now stands, and the saves turning it on made at once.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
pub struct WorkspaceAutosaveResult {
    pub auto_save: bool,
    /// `false` for a repeat, which writes nothing.
    pub changed: bool,
    /// One per tab that held edits when autosave turned on.
    pub saved: Vec<WorkspaceSaveResult>,
}

/// Close a market tab.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TabCloseInput {
    /// The tab. Left out, the one on screen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_id: Option<WireU64>,
    /// What to do with held edits: `true` saves them to the workspace first,
    /// `false` drops them. Left out on a tab holding edits, nothing closes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub save: Option<bool>,
}

/// The tab that closed, and the save made first when asked.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
pub struct TabCloseResult {
    pub tab_id: WireU64,
    pub saved: Option<WorkspaceSaveResult>,
}

/// Rename a workspace.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceRenameInput {
    pub name: String,
    pub to: String,
}

/// The name in force.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
pub struct WorkspaceRenameResult {
    pub workspace: String,
    /// `false` when the name already read so.
    pub changed: bool,
}

/// Delete a workspace.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceDeleteInput {
    pub name: String,
}

/// What a delete did.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
pub struct WorkspaceDeleteResult {
    /// Always `true`: a name no workspace has is refused with `workspace.unknown`.
    pub changed: bool,
    /// Every tab moved to `Default`.
    pub moved: Vec<WireU64>,
    /// The moved tabs that kept their edits, marked `Default •`.
    pub kept_edits: Vec<WireU64>,
}

/// Takes nothing: every workspace is listed.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceListInput {}

/// One workspace and the tabs using it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
pub struct WorkspaceEntry {
    pub name: String,
    /// One line: the panes, each context pane's interval and the indicator
    /// count, e.g. `Time 1m · Flow · 3 ind`.
    pub summary: String,
    pub tabs: Vec<WireU64>,
}

/// Every workspace, `Default` first, then by name.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
pub struct WorkspaceListResult {
    pub workspaces: Vec<WorkspaceEntry>,
}

fn bounded(mut descriptor: CapabilityDescriptor) -> CapabilityDescriptor {
    descriptor.expected_cost.max_response_bytes = Some(RESULT_MAX_BYTES);
    descriptor
}

/// Behind the desk's consent: this call changes what the trader comes back to.
fn writes_the_book(mut descriptor: CapabilityDescriptor) -> CapabilityDescriptor {
    descriptor.persistence = EffectPersistence::Durable;
    descriptor.required_permissions = [COCKPIT_PERMISSION_ID, COCKPIT_DESK_PERMISSION_ID]
        .into_iter()
        .map(|id| PermissionId::new(id).expect("static permission ID is valid"))
        .collect();
    bounded(descriptor)
}

pub fn tab_workspace_set_descriptor() -> CapabilityDescriptor {
    bounded(transient_descriptor::<
        TabWorkspaceSetInput,
        TabWorkspaceSetResult,
    >(
        TAB_WORKSPACE_SET_CAPABILITY_ID,
        quantick_control_host::authority::TAB_MODULE_ID,
        "Put a tab on a workspace",
        "Switches one tab to a saved workspace: its charts, context intervals, indicators, layers and footprint style. Drawings, anchored studies, bar size and the paper position stay with the market. It never asks: unsaved edits on the tab are dropped and the answer says so. Discover names with workspace.list.",
        "The call names the workspace it wants, so a repeat leaves the tab there, keeps any edits made since and answers changed false.",
    ))
}

pub fn workspace_save_to_descriptor() -> CapabilityDescriptor {
    writes_the_book(
        transient_descriptor::<WorkspaceTabInput, WorkspaceSaveResult>(
            WORKSPACE_SAVE_TO_CAPABILITY_ID,
            MODULE_ID,
            "Save a tab's edits to its workspace",
            "Writes the tab's edits into the workspace it uses and answers the other tabs that changed: the ones on that workspace without edits of their own. A tab with edits keeps them.",
            "A repeat finds no edits, writes nothing and answers changed false.",
        ),
    )
}

pub fn workspace_save_as_descriptor() -> CapabilityDescriptor {
    writes_the_book(transient_descriptor::<
        WorkspaceSaveAsInput,
        WorkspaceSaveResult,
    >(
        WORKSPACE_SAVE_AS_CAPABILITY_ID,
        MODULE_ID,
        "Save a tab as a new workspace",
        "Makes a workspace of what the tab shows and puts the tab on it. No other tab changes. An empty or taken name is refused with the reason.",
        "A repeat from the tab that made the name writes nothing and answers changed false.",
    ))
}

pub fn workspace_new_descriptor() -> CapabilityDescriptor {
    writes_the_book(
        transient_descriptor::<WorkspaceNewInput, WorkspaceNewResult>(
            WORKSPACE_NEW_CAPABILITY_ID,
            MODULE_ID,
            "Make a new, empty workspace for a tab",
            "Makes a workspace of an empty recipe: what the tab shows, its canvas, pane sizes, context intervals, layers and footprint style, with every indicator removed. The tab moves onto it and its unsaved edits are dropped; the answer says so. No other tab changes. Unlike workspace.save_as, it copies no indicators. An empty or taken name is refused with the reason; a tab still opening is refused with workspace.tab_not_ready, retryable.",
            "A repeat finds the name taken and is refused with workspace.name_taken; nothing changes.",
        ),
    )
}

pub fn workspace_revert_descriptor() -> CapabilityDescriptor {
    bounded(transient_descriptor::<
        WorkspaceTabInput,
        WorkspaceRevertResult,
    >(
        WORKSPACE_REVERT_CAPABILITY_ID,
        MODULE_ID,
        "Removed: always refuses",
        "Revert is gone. Every call refuses with workspace.revert_removed, naming workspace.save_to to keep edits and workspace.autosave.set to write them as they are made. A switch with tab.workspace.set drops held edits.",
        "Always refuses; nothing changes.",
    ))
}

pub fn workspace_autosave_set_descriptor() -> CapabilityDescriptor {
    writes_the_book(transient_descriptor::<
        WorkspaceAutosaveInput,
        WorkspaceAutosaveResult,
    >(
        WORKSPACE_AUTOSAVE_SET_CAPABILITY_ID,
        MODULE_ID,
        "Turn auto save layout changes on or off",
        "On, every layout edit on a tab is written into its workspace a second after it stops, and the clean tabs on that workspace follow; a tab reads as modified only while its copy is not in its workspace: a write that failed (retried each quiet second) or a second tab on that workspace holding edits (one copy is written: the active tab's, else the lowest id's). Off, a tab holds its edits, marked, until workspace.save_to, and closing it asks. Turning it on saves every tab holding edits at once, one per workspace, and answers each save; when one fails the setting stays on, the setting and the saves that landed are journaled, and the call refuses with workspace.write_failed whose details name auto_save and the workspaces saved. The desk keeps the choice; the workspace snapshot reads it as auto_save.",
        "The call names the state it wants, so a repeat answers changed false and writes only a save still owed, as after a failure.",
    ))
}

pub fn tab_close_descriptor() -> CapabilityDescriptor {
    writes_the_book(transient_descriptor::<TabCloseInput, TabCloseResult>(
        TAB_CLOSE_CAPABILITY_ID,
        quantick_control_host::authority::TAB_MODULE_ID,
        "Close a market tab",
        "Closes one tab. With autosave on its edits were written already and it closes at once. With autosave off and edits held, a call without save refuses with workspace.unsaved_edits, naming the workspace, the tabs a save also changes and what changed, and closes nothing; save true writes the edits first, save false drops them. The window's last tab refuses with workspace.last_tab before anything is saved or written.",
        "A closed tab is gone, so a repeat names an unknown tab and is refused with workspace.unknown_tab; nothing else closes.",
    ))
}

pub fn workspace_rename_descriptor() -> CapabilityDescriptor {
    writes_the_book(transient_descriptor::<
        WorkspaceRenameInput,
        WorkspaceRenameResult,
    >(
        WORKSPACE_RENAME_CAPABILITY_ID,
        MODULE_ID,
        "Rename a workspace",
        "Renames a workspace; the tabs on it follow. Default cannot be renamed, and an empty or taken name is refused with the reason.",
        "Naming the name it already has writes nothing and answers changed false.",
    ))
}

pub fn workspace_delete_descriptor() -> CapabilityDescriptor {
    writes_the_book(transient_descriptor::<
        WorkspaceDeleteInput,
        WorkspaceDeleteResult,
    >(
        WORKSPACE_DELETE_CAPABILITY_ID,
        MODULE_ID,
        "Delete a workspace",
        "Deletes a workspace and moves the tabs on it to Default. A tab with unsaved edits keeps them, marked Default •. Default cannot be deleted.",
        "A repeat finds no such workspace and is refused with workspace.unknown; nothing is written.",
    ))
}

pub fn workspace_list_descriptor() -> CapabilityDescriptor {
    read_descriptor::<WorkspaceListInput, WorkspaceListResult>(&ReadSpec {
        id: WORKSPACE_LIST_CAPABILITY_ID,
        module: MODULE_ID,
        title: "List the workspaces",
        description: "Lists every saved workspace with a one-line summary (panes, intervals, indicator count) and the open tabs using it, Default first. Which workspace a tab uses and whether it holds edits is also on each tab in workspace.summary.",
        permissions: &[OBSERVE_PERMISSION_ID, "observe.workspace"],
        pagination: None,
    })
}

/// How a client reconciles an interrupted call.
pub const READBACKS: &[Readback] = &[
    journal(
        TAB_WORKSPACE_SET_CAPABILITY_ID,
        IdempotencyPolicy::Optional,
        TAB_WORKSPACE_SET_EVENT_KIND,
        "payload.workspace",
        "an event after the pre-call cursor names the requested workspace",
        &[EVERY_OPTIONAL_TEST],
    ),
    journal(
        WORKSPACE_SAVE_TO_CAPABILITY_ID,
        IdempotencyPolicy::Optional,
        WORKSPACE_SAVED_EVENT_KIND,
        "payload.workspace",
        "an event after the pre-call cursor names the tab's workspace",
        &[EVERY_OPTIONAL_TEST],
    ),
    journal(
        WORKSPACE_SAVE_AS_CAPABILITY_ID,
        IdempotencyPolicy::Optional,
        WORKSPACE_SAVED_EVENT_KIND,
        "payload.workspace",
        "an event after the pre-call cursor names the new workspace",
        &[EVERY_OPTIONAL_TEST],
    ),
    journal(
        WORKSPACE_NEW_CAPABILITY_ID,
        IdempotencyPolicy::Optional,
        WORKSPACE_CREATED_EVENT_KIND,
        "payload.workspace",
        "an event after the pre-call cursor names the new workspace",
        &[EVERY_OPTIONAL_TEST],
    ),
    journal(
        WORKSPACE_REVERT_CAPABILITY_ID,
        IdempotencyPolicy::Optional,
        WORKSPACE_REVERTED_EVENT_KIND,
        "payload.workspace",
        "no event: every call refuses with workspace.revert_removed",
        &[EVERY_OPTIONAL_TEST],
    ),
    journal(
        WORKSPACE_AUTOSAVE_SET_CAPABILITY_ID,
        IdempotencyPolicy::Optional,
        WORKSPACE_AUTOSAVE_SET_EVENT_KIND,
        "payload.auto_save",
        "an event after the pre-call cursor carries the requested state",
        &[EVERY_OPTIONAL_TEST],
    ),
    journal(
        TAB_CLOSE_CAPABILITY_ID,
        IdempotencyPolicy::Optional,
        TAB_CLOSED_EVENT_KIND,
        "payload.tab_id",
        "an event after the pre-call cursor names the closed tab",
        &[EVERY_OPTIONAL_TEST],
    ),
    journal(
        WORKSPACE_RENAME_CAPABILITY_ID,
        IdempotencyPolicy::Optional,
        WORKSPACE_RENAMED_EVENT_KIND,
        "payload.workspace",
        "an event after the pre-call cursor carries the new name",
        &[EVERY_OPTIONAL_TEST],
    ),
    journal(
        WORKSPACE_DELETE_CAPABILITY_ID,
        IdempotencyPolicy::Optional,
        WORKSPACE_DELETED_EVENT_KIND,
        "payload.workspace",
        "an event after the pre-call cursor names the deleted workspace",
        &[EVERY_OPTIONAL_TEST],
    ),
];

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_tab_is_optional_and_a_stranger_is_refused() {
        let input: WorkspaceTabInput = serde_json::from_value(json!({})).unwrap();
        assert_eq!(input.tab_id, None);
        assert!(serde_json::from_value::<WorkspaceTabInput>(json!({"tab": 1})).is_err());
        assert!(serde_json::from_value::<WorkspaceSaveAsInput>(json!({})).is_err());
        assert!(serde_json::from_value::<WorkspaceNewInput>(json!({})).is_err());
        let new: WorkspaceNewInput = serde_json::from_value(json!({"name": "Blank"})).unwrap();
        assert_eq!((new.tab_id, new.name.as_str()), (None, "Blank"));
    }
}
