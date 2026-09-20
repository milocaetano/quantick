//! Indicators and the scripts behind them.
//!
//! Read [`super`] first: it says why the rows are written, how they are
//! joined, and what the drift guard does with them.

use super::PENDING_SURFACE;
use super::{ExclusionClass, Mapping, Source, UiBehaviour};

/// Every row this family owns.
pub(super) const ROWS: &[UiBehaviour] = &[
    UiBehaviour {
        id: "appearance.dialog.toggle",
        title: "Open the appearance dialog — candles, canvas, grid",
        reach: "toolbar brush button, Tools menu",
        keys: &[
            (Source::ToolbarAction, "ToggleAppearance"),
            (Source::MenuEntry, "Appearance…"),
        ],
        mapping: PENDING_SURFACE,
    },
    UiBehaviour {
        id: "indicator.hidden.toggle",
        title: "Hide an indicator's drawing without removing it",
        reach: "the eye on the legend row, and the indicators menu",
        keys: &[(Source::ToolbarAction, "ToggleIndicatorHidden")],
        mapping: excluded!(
            PendingCapability,
            "`indicator.script.attach` and `indicator.script.detach` add and remove; nothing \
             hides. Tracked in issue 401"
        ),
    },
    UiBehaviour {
        id: "indicator.legend.collapse",
        title: "Fold the focused chart's indicator legend to its count",
        reach: "the legend's own chevron, View menu, Ctrl+L",
        keys: &[(Source::Hotkey, "LEGEND_SHORTCUT")],
        mapping: PENDING_SURFACE,
    },
    UiBehaviour {
        id: "indicator.mouse_vertical_line.toggle",
        title: "Mirror price hover into one non-price indicator pane",
        reach: "right-click menu on an indicator pane",
        keys: &[(
            Source::Authored,
            "the indicator pane's Mouse vertical line checkbox",
        )],
        mapping: capability!("indicator.mouse_vertical_line.set"),
    },
    UiBehaviour {
        id: "indicator.native.add",
        title: "Add a native indicator from the catalog",
        reach: "toolbar indicators menu",
        keys: &[(Source::ToolbarAction, "AddNative")],
        mapping: excluded!(
            PendingCapability,
            "`indicator.script.attach` attaches a Pine script; the native catalog has no \
             capability, so an operator can add the indicators a trader writes and not the \
             ones shipped. Tracked in issue 401"
        ),
    },
    UiBehaviour {
        id: "indicator.native.remove",
        title: "Remove a native indicator",
        reach: "the legend row's close, and the indicators menu",
        keys: &[(
            Source::Authored,
            "the legend row close, which the toolbar enum sees only as `RemoveIndicator`",
        )],
        mapping: excluded!(
            PendingCapability,
            "`indicator.script.detach` names a script; a native has no identifier it accepts. \
             Tracked in issue 401"
        ),
    },
    UiBehaviour {
        id: "indicator.script.add",
        title: "Load a Pine script from the library",
        reach: "toolbar indicators menu",
        keys: &[(Source::ToolbarAction, "AddScriptIndicator")],
        mapping: capability!("indicator.script.attach"),
    },
    UiBehaviour {
        id: "indicator.script.remove",
        title: "Remove a script indicator",
        reach: "the legend row's close, and the indicators menu",
        keys: &[(Source::ToolbarAction, "RemoveIndicator")],
        mapping: capability!("indicator.script.detach"),
    },
    UiBehaviour {
        id: "indicator.settings.open",
        title: "Open an indicator's settings",
        reach: "the legend row's gear, and the indicators menu",
        keys: &[(Source::ToolbarAction, "OpenIndicatorSettings")],
        mapping: PENDING_SURFACE,
    },
];
