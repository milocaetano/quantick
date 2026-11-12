//! The cockpit the trader keeps between sessions.
//!
//! Read [`super`] first: it says why the rows are written, how they are
//! joined, and what the drift guard does with them.

use super::{ExclusionClass, Mapping, Source, UiBehaviour};

/// The three refusals the observer threat model states, quoted where a row
/// depends on one.
const AUTHORITY_ARBITRARY_PATH: Mapping = Mapping::Excluded {
    class: ExclusionClass::Authority,
    reason: "`docs/control-plane/observer-threat-model.md` section 6 refuses reading an \
             arbitrary file and exporting data to an arbitrary path. The trader's own file \
             dialog is the ceiling's other side, not a gap in it",
};

/// Every row this family owns.
pub(super) const ROWS: &[UiBehaviour] = &[
    UiBehaviour {
        id: "alerts.list",
        title: "List local alerts and manage their definitions, monitoring and history",
        reach: "window menu, Alerts\u{2026}",
        keys: &[(Source::MenuEntry, "Alerts\u{2026}")],
        mapping: Mapping::Capabilities(&[
            "alerts.open",
            "alerts.list",
            "alerts.read",
            "alerts.create",
            "alerts.edit",
            "alerts.delete",
            "alerts.pause",
            "alerts.resume",
            "alerts.rearm",
            "alerts.history",
        ]),
    },
    UiBehaviour {
        id: "desk.export",
        title: "Write the whole cockpit to a file",
        reach: "window menu, Desk, Export desk to file…",
        keys: &[(Source::MenuEntry, "Export desk to file…")],
        mapping: Mapping::Capabilities(&["desk.export"]),
    },
    UiBehaviour {
        id: "desk.import",
        title: "Open a cockpit from a file",
        reach: "window menu, Desk, Import desk from file…",
        keys: &[(Source::MenuEntry, "Import desk from file…")],
        mapping: Mapping::Capabilities(&["desk.import"]),
    },
    UiBehaviour {
        id: "workspace.recent.open",
        title: "Reopen a cockpit file opened before",
        reach: "window menu, Desk, Open recent",
        keys: &[(Source::MenuEntry, "Open recent")],
        mapping: AUTHORITY_ARBITRARY_PATH,
    },
    UiBehaviour {
        id: "workspace.reveal",
        title: "Open the folder the cockpit is kept in",
        reach: "window menu, Desk, Show where it is saved",
        keys: &[(Source::MenuEntry, "Show where it's saved")],
        mapping: excluded!(
            Authority,
            "`docs/control-plane/observer-threat-model.md` section 6 refuses starting a \
             terminal or any other process, and this entry hands the path to the system file \
             manager"
        ),
    },
    UiBehaviour {
        id: "desk.save",
        title: "Remember this arrangement as what quantick opens on",
        reach: "window menu, Save desk, Ctrl+Shift+S",
        keys: &[
            (Source::Hotkey, "SAVE_WORKSPACE_SHORTCUT"),
            (Source::MenuEntry, "Save desk"),
        ],
        mapping: Mapping::Capabilities(&["desk.save"]),
    },
    UiBehaviour {
        id: "desk.reopen_on_launch.set",
        title: "Keep the arrangement automatically when the window closes",
        reach: "window menu, Desk",
        keys: &[(Source::MenuEntry, "Reopen last tabs on launch")],
        mapping: Mapping::Capabilities(&["desk.reopen_on_launch.set"]),
    },
    UiBehaviour {
        id: "desk.forget",
        title: "Forget the saved workspace and open on the configured default",
        reach: "window menu, Desk, Reset startup desk",
        keys: &[(Source::MenuEntry, "Reset startup desk")],
        mapping: Mapping::Capabilities(&["desk.forget"]),
    },
    UiBehaviour {
        id: "tab.workspace.set",
        title: "Put a tab on a saved workspace",
        reach: "the capsule's workspace name, Ctrl+Shift+O",
        keys: &[
            (Source::Hotkey, "WORKSPACE_MENU_SHORTCUT"),
            (Source::ToolbarAction, "ToggleWorkspaceMenu"),
        ],
        mapping: Mapping::Capabilities(&["tab.workspace.set"]),
    },
    UiBehaviour {
        id: "workspace.save_to",
        title: "Save a tab's edits to its workspace",
        reach: "the capsule's floppy, its workspace menu, window menu, Save to, Ctrl+S",
        keys: &[
            (Source::ToolbarAction, "SaveWorkspace"),
            (Source::MenuEntry, "Save to workspace"),
            (Source::Hotkey, "SAVE_TO_WORKSPACE_SHORTCUT"),
        ],
        mapping: Mapping::Capabilities(&["workspace.save_to"]),
    },
    UiBehaviour {
        id: "workspace.save_as",
        title: "Save a tab as a new workspace",
        reach: "window menu, Workspace, Save as new",
        keys: &[(Source::MenuEntry, "Save as new\u{2026}")],
        mapping: Mapping::Capabilities(&["workspace.save_as"]),
    },
    UiBehaviour {
        id: "workspace.new",
        title: "Make a new, empty workspace for a tab",
        reach: "the capsule's workspace menu, New workspace",
        keys: &[(Source::MenuEntry, "New workspace\u{2026}")],
        mapping: Mapping::Capabilities(&["workspace.new"]),
    },
    UiBehaviour {
        id: "workspace.autosave.set",
        title: "Turn auto save layout changes on or off",
        reach: "the capsule's workspace menu, Auto save",
        keys: &[(Source::MenuEntry, "Auto save layout changes")],
        mapping: Mapping::Capabilities(&["workspace.autosave.set"]),
    },
    UiBehaviour {
        id: "workspace.rename",
        title: "Rename a workspace",
        reach: "window menu, Workspace, Rename, or a row's Rename in Manage workspaces",
        keys: &[(Source::MenuEntry, "Rename workspace\u{2026}")],
        mapping: Mapping::Capabilities(&["workspace.rename"]),
    },
    UiBehaviour {
        id: "workspace.manage",
        title: "List every workspace with its tabs, to rename or delete one (its tabs move to \
                Default), Delete confirmed in the row",
        reach: "window menu, Workspace, Manage workspaces\u{2026}, or the capsule's workspace menu",
        keys: &[(Source::MenuEntry, "Manage workspaces\u{2026}")],
        mapping: Mapping::Capabilities(&["workspace.list", "workspace.rename", "workspace.delete"]),
    },
    UiBehaviour {
        id: "workspace.colour.set",
        title: "Give a workspace a colour, swapping with the workspace holding it",
        reach: "the capsule's workspace menu, the swatches under the workspaces",
        keys: &[(
            Source::Authored,
            "the swatch row under the capsule's workspace menu is neither a menu-bar entry nor a toolbar action",
        )],
        mapping: Mapping::Capabilities(&["workspace.colour.set"]),
    },
    UiBehaviour {
        id: "workspace.list",
        title: "List the workspaces and the tabs using each",
        reach: "window menu, Workspace, whose list names each workspace",
        keys: &[(Source::MenuEntry, "Workspace")],
        mapping: Mapping::Capabilities(&["workspace.list"]),
    },
];
