use super::{ExclusionClass, Mapping, Source, UiBehaviour};

pub(super) const BINDING: (usize, &[UiBehaviour]) = (2, ROWS);
const ROWS: &[UiBehaviour] = &[
    UiBehaviour {
        id: "layout.tab.create",
        title: "Add a layout tab",
        reach: "layout strip +, window menu, Workspace, Layouts",
        keys: &[
            (Source::StripAction, "Create"),
            (Source::MenuEntry, "New layout"),
        ],
        mapping: capability!("layout.tab.create"),
    },
    UiBehaviour {
        id: "layout.tab.delete",
        title: "Delete a layout tab",
        reach: "layout strip context menu, window menu, Workspace, Layouts",
        keys: &[
            (Source::StripAction, "Delete"),
            (Source::MenuEntry, "Delete layout"),
        ],
        mapping: excluded!(
            UiOnlyByDecision,
            "`crate::control::layout` states it: `layout.tab.delete` is deliberately not \
             registered, because deleting a layout destroys its indicator set and every \
             drawing under it and no effect policy in the control contract allows a \
             destructive capability yet. The trader deletes from the strip or the window menu"
        ),
    },
    UiBehaviour {
        id: "layout.tab.rename",
        title: "Rename a layout tab",
        reach: "layout strip double-click, window menu, Workspace, Layouts",
        keys: &[
            (Source::StripAction, "BeginRename"),
            (Source::StripAction, "CommitRename"),
            (Source::MenuEntry, "Rename layout…"),
        ],
        mapping: capability!("layout.tab.rename"),
    },
    UiBehaviour {
        id: "layout.tab.rename.cancel",
        title: "Abandon a rename in progress",
        reach: "Escape, or clicking away from the strip's rename box",
        keys: &[(Source::StripAction, "CancelRename")],
        mapping: excluded!(
            UiOnlyByDecision,
            "`layout.tab.rename` is atomic: a caller sends the new name or sends nothing, so \
             there is no half-finished rename for it to abandon. The begin/cancel pair is the \
             in-place editor's own state and has no remote counterpart by construction"
        ),
    },
    UiBehaviour {
        id: "layout.tab.switch",
        title: "Show another layout on the focused chart",
        reach: "layout strip, window menu, Workspace, Layouts, Alt+1 … Alt+9",
        keys: &[(Source::StripAction, "Switch")],
        mapping: capability!("layout.tab.switch"),
    },
];
