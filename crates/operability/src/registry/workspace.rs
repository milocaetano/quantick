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

/// Something the trader keeps between sessions, with no capability for it.
const PENDING_WORKSPACE: Mapping = Mapping::Excluded {
    class: ExclusionClass::PendingCapability,
    reason: "no capability reaches the saved cockpit; `layout.*` moves panes within a session \
             and stops there. Tracked in issue 401",
};

/// Every row this family owns.
pub(super) const ROWS: &[UiBehaviour] = &[
    UiBehaviour {
        id: "workspace.export",
        title: "Write the whole cockpit to a file",
        reach: "window menu, Desk, Export to file…",
        keys: &[(Source::MenuEntry, "Export desk to file…")],
        mapping: AUTHORITY_ARBITRARY_PATH,
    },
    UiBehaviour {
        id: "workspace.import",
        title: "Open a cockpit from a file",
        reach: "window menu, Desk, Open from file…",
        keys: &[(Source::MenuEntry, "Import desk from file…")],
        mapping: AUTHORITY_ARBITRARY_PATH,
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
        id: "workspace.save",
        title: "Remember this arrangement as what quantick opens on",
        reach: "window menu, Desk, Ctrl+Shift+S",
        keys: &[
            (Source::Hotkey, "SAVE_WORKSPACE_SHORTCUT"),
            (Source::MenuEntry, "Save desk"),
        ],
        mapping: PENDING_WORKSPACE,
    },
    UiBehaviour {
        id: "workspace.save_on_exit",
        title: "Keep the arrangement automatically when the window closes",
        reach: "window menu, Desk",
        keys: &[(Source::MenuEntry, "Reopen last tabs on launch")],
        mapping: PENDING_WORKSPACE,
    },
    UiBehaviour {
        id: "workspace.startup.reset",
        title: "Forget the saved workspace and open on the configured default",
        reach: "window menu, Desk, Reset startup desk",
        keys: &[(Source::MenuEntry, "Reset startup desk")],
        mapping: PENDING_WORKSPACE,
    },
];
