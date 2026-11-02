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

/// Named ahead of the rows that paint them: the calls ship first, and each key
/// becomes the entry it claims when the strip and the menu draw it.
const NOT_PAINTED_YET: &str = "the workspace rows are painted in the next seam of this feature";

/// Every row this family owns.
pub(super) const ROWS: &[UiBehaviour] = &[
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
        reach: "the active tab's workspace button",
        keys: &[(Source::Authored, NOT_PAINTED_YET)],
        mapping: Mapping::Capabilities(&["tab.workspace.set"]),
    },
    UiBehaviour {
        id: "workspace.save_to",
        title: "Save a tab's edits to its workspace",
        reach: "window menu, Save to",
        keys: &[(Source::Authored, NOT_PAINTED_YET)],
        mapping: Mapping::Capabilities(&["workspace.save_to"]),
    },
    UiBehaviour {
        id: "workspace.save_as",
        title: "Save a tab as a new workspace",
        reach: "window menu, Workspace, Save as new",
        keys: &[(Source::Authored, NOT_PAINTED_YET)],
        mapping: Mapping::Capabilities(&["workspace.save_as"]),
    },
    UiBehaviour {
        id: "workspace.revert",
        title: "Drop a tab's unsaved edits",
        reach: "window menu, Workspace, Revert",
        keys: &[(Source::Authored, NOT_PAINTED_YET)],
        mapping: Mapping::Capabilities(&["workspace.revert"]),
    },
    UiBehaviour {
        id: "workspace.rename",
        title: "Rename a workspace",
        reach: "window menu, Workspace, Rename",
        keys: &[(Source::Authored, NOT_PAINTED_YET)],
        mapping: Mapping::Capabilities(&["workspace.rename"]),
    },
    UiBehaviour {
        id: "workspace.delete",
        title: "Delete a workspace, moving its tabs to Default",
        reach: "window menu, Workspace, Delete",
        keys: &[(Source::Authored, NOT_PAINTED_YET)],
        mapping: Mapping::Capabilities(&["workspace.delete"]),
    },
    UiBehaviour {
        id: "workspace.list",
        title: "List the workspaces and the tabs using each",
        reach: "the active tab's workspace menu",
        keys: &[(Source::Authored, NOT_PAINTED_YET)],
        mapping: Mapping::Capabilities(&["workspace.list"]),
    },
];
