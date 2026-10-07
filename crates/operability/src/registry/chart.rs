//! What the trader does to the chart itself: the canvas under the
//! cursor, and reading further back than the window holds.
//!
//! Read [`super`] first: it says why the rows are written, how they are
//! joined, and what the drift guard does with them.

use super::{ExclusionClass, Mapping, Source, UiBehaviour};

/// The venue-candle reach and the two View switches, with no capability yet.
/// Trade history loads through `feed.history.load` and `feed.history.cancel`.
const PENDING_HISTORY: Mapping = Mapping::Excluded {
    class: ExclusionClass::PendingCapability,
    reason: "no capability fetches older venue candles or flips these View switches; trade \
             history loads through feed.history.load. Tracked in issue 401",
};

/// The canvas.
pub(super) const CANVAS: &[UiBehaviour] = &[
    UiBehaviour {
        id: "chart.bars.set_spec",
        title: "Change what one bar is — kind and size",
        reach: "toolbar bar controls, typing a number over the chart",
        keys: &[(
            Source::Authored,
            "the bar-kind and size controls are toolbar widgets, not entries in its action enum",
        )],
        mapping: capability!("layout.pane.set_bar_spec", "layout.pane.set_interval"),
    },
    UiBehaviour {
        id: "chart.pan",
        title: "Drag the chart horizontally through history",
        reach: "horizontal primary drag on the canvas",
        keys: &[(
            Source::Authored,
            "a pointer drag the canvas handles directly; no registry names it",
        )],
        mapping: excluded!(
            PendingCapability,
            "`chart.window.read` reports the visible time window; no capability pans that \
             window. Price framing has its own capability below. Tracked in issue 401"
        ),
    },
    UiBehaviour {
        id: "chart.price_axis.set",
        title: "Set a pane's price range or resume automatic fitting",
        reach: "drag or wheel on the price axis; double-click it to reset",
        keys: &[(
            Source::Authored,
            "price-axis gestures update the pane's PriceView directly",
        )],
        mapping: capability!("chart.price_axis.set"),
    },
    UiBehaviour {
        id: "chart.tape.pan",
        title: "Drag the tape beside the candles through time and price",
        reach: "primary or middle drag over the native tape; double-click it to return to live",
        keys: &[(
            Source::Authored,
            "a pointer drag over the tape the canvas handles directly; no registry names it",
        )],
        mapping: capability!("chart.tape_view.set", "chart.price_axis.set"),
    },
    UiBehaviour {
        id: "chart.tape.zoom",
        title: "Zoom the tape's time window in or out",
        reach: "wheel over the native tape; drag or wheel on its time strip; its window menu",
        keys: &[(
            Source::Authored,
            "a wheel over the tape and its time strip the canvas handles directly",
        )],
        mapping: capability!("chart.tape_view.set"),
    },
    UiBehaviour {
        id: "chart.zoom",
        title: "Zoom the chart's time window in or out",
        reach: "wheel on the canvas; drag on the time axis",
        keys: &[(
            Source::Authored,
            "a wheel and an axis drag the canvas handles directly; no registry names it",
        )],
        mapping: excluded!(
            PendingCapability,
            "`chart.window.read` reports the time window; no capability changes its zoom. \
             Price-axis zoom has its own capability above. Tracked in issue 401"
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
        id: "layout.flow.collapse",
        title: "Put the flow chart away, or bring it back",
        reach: "drag the vertical canvas divider toward the right edge or drag its rail left",
        keys: &[(Source::Authored, "the right-side canvas rail and divider")],
        mapping: capability!("layout.flow.collapse", "layout.flow.expand"),
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
        id: "history.trades.cancel",
        title: "Stop loading history and keep what arrived",
        reach: "the loading History button, its menu's Cancel loading, or Esc",
        keys: &[(Source::ToolbarAction, "CancelHistory")],
        mapping: capability!("feed.history.cancel"),
    },
    UiBehaviour {
        id: "history.trades.load",
        title: "Load the chart back by hours of trading or to a previous session's open",
        reach: "toolbar History button (repeats the last target) and its menu's targets",
        keys: &[(Source::ToolbarAction, "LoadHistory")],
        mapping: capability!("feed.history.load"),
    },
    UiBehaviour {
        id: "history.venue_lead_in.toggle",
        title: "Put venue minute candles in front of bars cut by trades",
        reach: "View menu",
        keys: &[(Source::MenuEntry, "Venue candles on charts cut by trades")],
        mapping: PENDING_HISTORY,
    },
];
