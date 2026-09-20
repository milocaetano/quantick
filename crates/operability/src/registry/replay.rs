//! Replaying a recorded tape.
//!
//! Read [`super`] first: it says why the rows are written, how they are
//! joined, and what the drift guard does with them.

use super::{ExclusionClass, Mapping, PENDING_SURFACE, Source, UiBehaviour};

/// Every row this family owns.
pub(super) const ROWS: &[UiBehaviour] = &[
    UiBehaviour {
        id: "replay.browser.open",
        title: "Open the market replay browser",
        reach: "File menu, Ctrl+R",
        keys: &[
            (Source::Hotkey, "REPLAY_SHORTCUT"),
            (Source::MenuEntry, "Market Replay…"),
        ],
        mapping: PENDING_SURFACE,
    },
    UiBehaviour {
        id: "replay.close",
        title: "Leave replay and go back to the live tape",
        reach: "File menu, while a tab is replaying",
        keys: &[(Source::MenuEntry, "Close Replay")],
        mapping: excluded!(
            PendingCapability,
            "no capability drives the replay transport, so an operator can neither start a \
             session nor end one. Tracked in issue 401"
        ),
    },
    UiBehaviour {
        id: "replay.format_help.open",
        title: "Read what a replay file has to look like",
        reach: "Help menu",
        keys: &[(Source::MenuEntry, "Replay file format…")],
        mapping: PENDING_SURFACE,
    },
];
