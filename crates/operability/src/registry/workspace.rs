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

/// The desk's rows, read before the doors a workspace-book call performs:
/// those are that call's `ui` entry in `quantick_control_schema::workspace_book`.
pub const DESK: &[UiBehaviour] = &[
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
];

/// Manage workspaces and the colour popover, read after the book's calls.
pub const MANAGE: &[UiBehaviour] = &[
    UiBehaviour {
        id: "workspace.manage",
        title: "Open Manage workspaces: every workspace with its colour, name, tab count, \
                rename and delete",
        reach: "window menu, Workspace, Manage workspaces\u{2026}, or the capsule's workspace menu",
        keys: &[(Source::MenuEntry, "Manage workspaces\u{2026}")],
        mapping: Mapping::Capabilities(&["workspace.dialog.open", "workspace.list"]),
    },
    UiBehaviour {
        id: "workspace.manage.colour",
        title: "Recolour any workspace from its Manage row, swapping with the holder when asked",
        reach: "Manage workspaces, a row's colour chip or C on the chosen row",
        keys: &[(
            Source::Authored,
            "the chip and the palette under the row are marks the dialog paints: listed in the \
             scene as toolbar.manage_workspaces.<workspace>.chip and \
             toolbar.manage_workspaces.<workspace>.colour.<key>",
        )],
        mapping: Mapping::Capabilities(&["workspace.colour.set", "workspace.colour.palette"]),
    },
    UiBehaviour {
        id: "workspace.manage.rename",
        title: "Rename a workspace in its Manage row: Enter saves, Esc cancels",
        reach: "Manage workspaces, a row's pencil, F2 on the chosen row, or a double click on the name",
        keys: &[(
            Source::Authored,
            "the pencil and the field are marks the dialog paints: \
             toolbar.manage_workspaces.<workspace>.rename and .name",
        )],
        mapping: Mapping::Capabilities(&["workspace.rename"]),
    },
    UiBehaviour {
        id: "workspace.manage.delete",
        title: "Delete a workspace, confirmed in its row naming the tabs that go back to Default",
        reach: "Manage workspaces, a row's trash or Del on the chosen row, then Delete or Enter",
        keys: &[(
            Source::Authored,
            "the trash, Delete and Keep are marks the dialog paints: \
             toolbar.manage_workspaces.<workspace>.delete, .confirm and .keep",
        )],
        mapping: Mapping::Capabilities(&["workspace.delete"]),
    },
    UiBehaviour {
        id: "workspace.manage.undo",
        title: "Undo a workspace delete for 10 seconds: the workspace, its colour and its tabs",
        reach: "Manage workspaces, the deleted row's Undo or Ctrl+Z",
        keys: &[(
            Source::Authored,
            "the Undo row is marks the dialog paints: toolbar.manage_workspaces.<workspace>.undo",
        )],
        mapping: Mapping::Capabilities(&["workspace.delete.undo"]),
    },
    UiBehaviour {
        id: "workspace.colour.palette",
        title: "Read a workspace's palette: the presets, who holds each, the last custom colour",
        reach: "the capsule's workspace menu, the header swatch's popover",
        keys: &[(
            Source::Authored,
            "the popover is drawn from this model; its swatches are scene entries toolbar.workspace_menu.colour.<key>",
        )],
        mapping: Mapping::Capabilities(&["workspace.colour.palette"]),
    },
    UiBehaviour {
        id: "workspace.colour.popover",
        title: "Open the colour popover, pick a swatch, type a Custom colour and answer the swap question",
        reach: "the capsule's workspace menu, the header swatch",
        keys: &[(
            Source::Authored,
            "a press on the header swatch, a swatch, Custom\u{2026}, Apply, Swap or Cancel; arrows, Enter and Esc in the popover",
        )],
        mapping: Mapping::Capabilities(&["workspace.colour.popover"]),
    },
];
