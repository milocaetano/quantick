//! What the trader does to the chart itself: the canvas under the
//! cursor, and reading further back than the window holds.
//!
//! Read [`super`] first: it says why the rows are written, how they are
//! joined, and what the drift guard does with them.

use super::{ExclusionClass, Mapping, Source, UiBehaviour};

/// Reading further back than the window holds, with no capability for it.
const PENDING_HISTORY: Mapping = Mapping::Excluded {
    class: ExclusionClass::PendingCapability,
    reason: "no capability pages history; `chart.window.read` reads what is already loaded. \
             Tracked in issue 401",
};

/// The canvas.
pub(super) const CANVAS: &[UiBehaviour] = &[
    UiBehaviour {
        id: "chart.bars.set_spec",
        title: "Change what one bar is — kind and size",
        reach: "toolbar bar controls",
        keys: &[(
            Source::Authored,
            "the bar-kind and size controls are toolbar widgets, not entries in its action enum",
        )],
        mapping: capability!("layout.pane.set_bar_spec", "layout.pane.set_interval"),
    },
    UiBehaviour {
        id: "chart.pan",
        title: "Drag the chart back through the tape",
        reach: "primary drag on the canvas; the price and time axes",
        keys: &[(
            Source::Authored,
            "a pointer drag the canvas handles directly; no registry names it",
        )],
        mapping: excluded!(
            PendingCapability,
            "`chart.window.read` reports the visible window; nothing sets it, so an operator \
             reads where the trader is looking and cannot look elsewhere. Tracked in issue 401"
        ),
    },
    UiBehaviour {
        id: "chart.zoom",
        title: "Zoom the chart in or out",
        reach: "wheel on the canvas; drag on either axis",
        keys: &[(
            Source::Authored,
            "a wheel and an axis drag the canvas handles directly; no registry names it",
        )],
        mapping: excluded!(
            PendingCapability,
            "the read half exists as `chart.window.read` and the write half does not. Tracked \
             in issue 401"
        ),
    },
    UiBehaviour {
        id: "layout.context.collapse",
        title: "Put the context charts away, or bring them back",
        reach: "View menu, Ctrl+0",
        keys: &[(Source::Hotkey, "COLLAPSE_CONTEXT_SHORTCUT")],
        mapping: capability!("layout.pane.collapse", "layout.pane.expand"),
    },
    UiBehaviour {
        id: "layout.pane.focus",
        title: "Make another chart the focused one",
        reach: "click anywhere on a chart",
        keys: &[(
            Source::Authored,
            "a click anywhere on a pane; the focus follows it without a named control",
        )],
        mapping: capability!("layout.focus.set"),
    },
    UiBehaviour {
        id: "layout.pane.move",
        title: "Move a context chart up or down the column",
        reach: "View → Move chart, and the drag the menu entry exists to replace",
        keys: &[(Source::MenuEntry, "Move chart")],
        mapping: capability!("layout.pane.move"),
    },
    UiBehaviour {
        id: "layout.pane.resize",
        title: "Resize columns or adjacent context charts",
        reach: "drag the horizontal or vertical divider between two charts",
        keys: &[(Source::Authored, "a drag on the divider between two panes")],
        mapping: capability!("layout.pane.resize", "layout.pane.resize_pair"),
    },
    UiBehaviour {
        id: "layout.preset.apply",
        title: "Switch the canvas to another arrangement",
        reach: "toolbar layout picker, View → Layout, Ctrl+1 … Ctrl+9",
        keys: &[
            (Source::ToolbarAction, "SetLayout"),
            (Source::LayoutPreset, "flow"),
            (Source::LayoutPreset, "time"),
            (Source::LayoutPreset, "time+flow"),
            (Source::LayoutPreset, "time+time+flow"),
            (Source::Hotkey, "LAYOUT_PRESET_KEYS"),
            (Source::MenuEntry, "Layout"),
        ],
        mapping: capability!("layout.preset.apply"),
    },
];

/// History.
pub(super) const HISTORY: &[UiBehaviour] = &[
    UiBehaviour {
        id: "history.candles.load_older",
        title: "Fetch another span of older venue candles",
        reach: "toolbar history caret",
        keys: &[(Source::ToolbarAction, "LoadOlderCandles")],
        mapping: PENDING_HISTORY,
    },
    UiBehaviour {
        id: "history.progressive.toggle",
        title: "Build venue history backwards a week at a time, or in one request",
        reach: "View menu",
        keys: &[(Source::MenuEntry, "Progressive venue history")],
        mapping: PENDING_HISTORY,
    },
    UiBehaviour {
        id: "history.reach.set",
        title: "Choose how far back the chart reaches, and the page size",
        reach: "toolbar history caret menu, reachable by the `history` scripted-menu hook",
        keys: &[(
            Source::Authored,
            "the reach chips and page size inside the toolbar caret menu, drawn per frame",
        )],
        mapping: PENDING_HISTORY,
    },
    UiBehaviour {
        id: "history.trades.load_older",
        title: "Fetch another page of older trades",
        reach: "toolbar history caret",
        keys: &[(Source::ToolbarAction, "LoadOlder")],
        mapping: PENDING_HISTORY,
    },
    UiBehaviour {
        id: "history.venue_lead_in.toggle",
        title: "Put venue minute candles in front of bars cut by trades",
        reach: "View menu",
        keys: &[(Source::MenuEntry, "Venue candles on charts cut by trades")],
        mapping: PENDING_HISTORY,
    },
];
