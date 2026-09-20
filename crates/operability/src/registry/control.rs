//! The window, its tabs, and the chrome around the chart: what an operator
//! reaches that is not the chart, a drawing or a trade.
//!
//! Read [`super`] first: it says why the rows are written, how they are
//! joined, and what the drift guard does with them.

use super::{ExclusionClass, Mapping, Source, UiBehaviour};

/// The window and its market tabs.
pub(super) const WINDOW_AND_TABS: &[UiBehaviour] = &[
    UiBehaviour {
        id: "app.exit",
        title: "Close the application",
        reach: "File menu, and the window's own close button",
        keys: &[(Source::MenuEntry, "Exit")],
        mapping: excluded!(
            Authority,
            "`docs/control-plane/observer-threat-model.md` section 6 refuses starting or \
             stopping Quantick, a terminal, a broker or any other process. A capability that \
             closes the window would end every other capability's session"
        ),
    },
    UiBehaviour {
        id: "tab.market.activate",
        title: "Switch to another market tab",
        reach: "tab strip in the menu row; Ctrl+Tab and Ctrl+Shift+Tab",
        keys: &[
            (Source::TabAction, "Activate"),
            (Source::Hotkey, "NEXT_TAB_SHORTCUT"),
            (Source::Hotkey, "PREVIOUS_TAB_SHORTCUT"),
        ],
        mapping: excluded!(
            PendingCapability,
            "`layout.tab.switch` switches a layout tab, not a market tab; nothing reaches the \
             strip that carries the open markets. Tracked in issue 401"
        ),
    },
    UiBehaviour {
        id: "tab.market.close",
        title: "Close the active market tab",
        reach: "tab strip chip, File menu, Ctrl+W",
        keys: &[
            (Source::TabAction, "Close"),
            (Source::Hotkey, "CLOSE_TAB_SHORTCUT"),
            (Source::MenuEntry, "Close Tab"),
        ],
        mapping: excluded!(
            PendingCapability,
            "closing a tab discards its drawings and its paper position, and the control \
             contract admits no destructive effect yet — the same ceiling `layout.tab.delete` \
             sits under. Tracked in issue 401"
        ),
    },
    UiBehaviour {
        id: "tab.market.open",
        title: "Open a new market tab",
        reach: "tab strip +, File menu, Ctrl+T",
        keys: &[
            (Source::TabAction, "New"),
            (Source::Hotkey, "NEW_TAB_SHORTCUT"),
            (Source::MenuEntry, "New Tab…"),
        ],
        mapping: excluded!(
            PendingCapability,
            "no capability opens a market; the source picker is the only door. Tracked in \
             issue 401"
        ),
    },
];

/// Layout tabs.
pub(super) const LAYOUT_TABS: &[UiBehaviour] = &[
    UiBehaviour {
        id: "layout.tab.create",
        title: "Add a layout tab",
        reach: "layout strip +, View → Layouts",
        keys: &[
            (Source::StripAction, "Create"),
            (Source::MenuEntry, "New layout"),
        ],
        mapping: capability!("layout.tab.create"),
    },
    UiBehaviour {
        id: "layout.tab.delete",
        title: "Delete a layout tab",
        reach: "layout strip context menu, View → Layouts",
        keys: &[
            (Source::StripAction, "Delete"),
            (Source::MenuEntry, "Delete layout"),
        ],
        mapping: excluded!(
            UiOnlyByDecision,
            "`crate::control::layout` states it: `layout.tab.delete` is deliberately not \
             registered, because deleting a layout destroys its indicator set and every \
             drawing under it and no effect policy in the control contract allows a \
             destructive capability yet. The trader deletes from the strip or the View menu"
        ),
    },
    UiBehaviour {
        id: "layout.tab.rename",
        title: "Rename a layout tab",
        reach: "layout strip double-click, View → Layouts",
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
        reach: "layout strip, View → Layouts, Alt+1 … Alt+9",
        keys: &[(Source::StripAction, "Switch")],
        mapping: capability!("layout.tab.switch"),
    },
];

/// Feed recovery.
pub(super) const FEED_RECOVERY: &[UiBehaviour] = &[
    UiBehaviour {
        id: "feed.reconnect",
        title: "Respawn the transport and keep the timeline",
        reach: "the feed notice popup",
        keys: &[(Source::NoticeAction, "Reconnect")],
        mapping: capability!("feed.reconnect"),
    },
    UiBehaviour {
        id: "feed.reload",
        title: "Throw the timeline away and rebuild the chart",
        reach: "the feed notice popup",
        keys: &[(Source::NoticeAction, "Reload")],
        mapping: capability!("feed.reload"),
    },
    UiBehaviour {
        id: "feed.deal_recording.set",
        title: "Record the venue's deal counter, stop, load a recorded day, or show the pane as \
                trades bars",
        reach: "the REC control beside the symbol and its popover; the Tools menu's \
                `Record deals by default` checkbox",
        // One registry key: the matrix counts a row carrying an `Authored` key
        // as one no registry stands behind. The Tools menu checkbox is drawn by
        // `app/deal_recording_wiring.rs`, not an entry in the menu registry;
        // the call's `record_by_default` is the same choice.
        keys: &[(Source::ToolbarAction, "DealRecording")],
        // `ShowAsTrades` is the popover's shortcut to the `trades` rule, the
        // same recut `layout.pane.set_bar_spec` drives; `OpenFolder` reveals
        // the recording directory in the OS file browser, a hand-only
        // convenience whose path `feed.status` reports as `file`.
        mapping: capability!("feed.deal_recording.set", "layout.pane.set_bar_spec"),
    },
];

/// The rest of the chrome.
pub(super) const CHROME: &[UiBehaviour] = &[
    UiBehaviour {
        id: "control.access.open",
        title: "Open the local agent access panel",
        reach: "Tools menu, and the menu row access chip",
        keys: &[(Source::MenuEntry, "Agent access: on")],
        mapping: excluded!(
            Authority,
            "the panel is where the trader grants and revokes an operator's access. A \
             capability that opened it would let a connection widen its own authority, which \
             `docs/control-plane/observer-threat-model.md` exists to prevent"
        ),
    },
    UiBehaviour {
        id: "view.perf_readings.toggle",
        title: "Show fps, frame time and trade count on the status bar",
        reach: "View menu",
        keys: &[(Source::MenuEntry, "Perf readings")],
        mapping: excluded!(
            PendingCapability,
            "`health.diagnostics.read` reports the same numbers to an operator directly, so \
             the switch governs the trader's own status bar and nothing else. A capability \
             would be for parity of the screen, not of the reading. Tracked in issue 401"
        ),
    },
    UiBehaviour {
        id: "view.timezone.set",
        title: "Change the timezone the time axis is labelled in",
        reach: "View menu, Timezone",
        keys: &[(Source::MenuEntry, "Timezone")],
        mapping: excluded!(
            PendingCapability,
            "no capability sets the display timezone; every control-plane reading is already \
             absolute, so this is the trader's own axis. Tracked in issue 401"
        ),
    },
];
