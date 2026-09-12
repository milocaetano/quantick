//! The table: one row per supported UI behaviour.
//!
//! Read [`super`] first for why the rows are written and the drift guard is
//! mechanical. This file is the data.
//!
//! Adding a behaviour is a row here. The guard tells you when one is missing —
//! it names the registry entry nothing claims — and it will not tell you what
//! the row should say, because that is the decision the table exists to
//! record: either a capability performs the behaviour, or somebody has
//! written down why none does.

use super::{ExclusionClass, Mapping, Source, UiBehaviour};

/// Behaviours nothing refuses and nothing performs yet, by group.
///
/// Every one of these cites the same issue on purpose: the matrix is the
/// enumeration, and thirty stub issues would put that list in two places. The
/// text differs so a reader sees what each group is actually missing.
pub(crate) const PENDING_ISSUE: &str = "https://github.com/milocaetano/quantick/issues/401";

/// One capability, as a slice, because [`Mapping::Capabilities`] takes the
/// plural case as the general one.
macro_rules! capability {
    ($($id:literal),+ $(,)?) => {
        Mapping::Capabilities(&[$($id),+])
    };
}

/// An exclusion, spelled once per class so a row reads as prose.
macro_rules! excluded {
    ($class:ident, $reason:literal) => {
        Mapping::Excluded {
            class: ExclusionClass::$class,
            reason: $reason,
        }
    };
}

/// A drawing tool the rail arms, for which no capability places the object.
///
/// `annotate.label.create`, `annotate.arrow.create` and `annotate.zone.create`
/// place the text, arrow and rectangle respectively; the other nineteen
/// registered tools have no counterpart, so an operator can draw three of the
/// twenty-two shapes a trader can.
const PENDING_DRAWING_TOOL: Mapping = Mapping::Excluded {
    class: ExclusionClass::PendingCapability,
    reason: "no `annotate.*` capability places this shape; only the text, arrow and rectangle \
             tools have one. Tracked in issue 401",
};

/// A chart layer switch with no capability behind it.
const PENDING_LAYER: Mapping = Mapping::Excluded {
    class: ExclusionClass::PendingCapability,
    reason: "no capability switches a chart layer; the scene reports the toggle's state but \
             nothing can press it. Tracked in issue 401",
};

/// A window or panel that opens, with no capability that opens it.
const PENDING_SURFACE: Mapping = Mapping::Excluded {
    class: ExclusionClass::PendingCapability,
    reason: "no capability opens or closes this surface; an operator can read what is on screen \
             and not change it. Tracked in issue 401",
};

/// Something the trader keeps between sessions, with no capability for it.
const PENDING_WORKSPACE: Mapping = Mapping::Excluded {
    class: ExclusionClass::PendingCapability,
    reason: "no capability reaches the saved cockpit; `layout.*` moves panes within a session \
             and stops there. Tracked in issue 401",
};

/// Reading further back than the window holds, with no capability for it.
const PENDING_HISTORY: Mapping = Mapping::Excluded {
    class: ExclusionClass::PendingCapability,
    reason: "no capability pages history; `chart.window.read` reads what is already loaded. \
             Tracked in issue 401",
};

/// The three refusals the observer threat model states, quoted where a row
/// depends on one.
const AUTHORITY_ARBITRARY_PATH: Mapping = Mapping::Excluded {
    class: ExclusionClass::Authority,
    reason: "`docs/control-plane/observer-threat-model.md` section 6 refuses reading an \
             arbitrary file and exporting data to an arbitrary path. The trader's own file \
             dialog is the ceiling's other side, not a gap in it",
};

/// Every supported UI behaviour, in identifier order within its group.
///
/// The order here is the order the matrix renders and therefore the order a
/// reviewer reads. Grouped by what the trader is doing rather than by which
/// registry registered it, because "what can I not do without a mouse" is the
/// question this answers.
pub(crate) const UI_BEHAVIOURS: &[UiBehaviour] = &[
    // ---- The window and its market tabs -------------------------------
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
    // ---- Replay --------------------------------------------------------
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
    // ---- Layout tabs ---------------------------------------------------
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
    // ---- The canvas ----------------------------------------------------
    UiBehaviour {
        id: "chart.bars.set_spec",
        title: "Change what one bar is — kind and size",
        reach: "toolbar bar controls",
        keys: &[(
            Source::Authored,
            "the bar-kind and size controls are toolbar widgets, not entries in its action enum",
        )],
        mapping: capability!("layout.pane.set_interval"),
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
        title: "Resize a chart by its splitter",
        reach: "drag the divider between two charts",
        keys: &[(Source::Authored, "a drag on the divider between two panes")],
        mapping: capability!("layout.pane.resize"),
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
    // ---- Panels and layers ---------------------------------------------
    UiBehaviour {
        id: "dock.tab.open",
        title: "Open a panel — L2, bubbles, session, trading or trades",
        reach: "View menu, the dock's own strip, a layer button's right-click",
        keys: &[
            (Source::ToolbarAction, "OpenDockTab"),
            (Source::DockTab, "l2"),
            (Source::DockTab, "bubbles"),
            (Source::DockTab, "session"),
            (Source::DockTab, "trading"),
            (Source::DockTab, "trades"),
        ],
        mapping: PENDING_SURFACE,
    },
    UiBehaviour {
        id: "dock.toggle",
        title: "Show or hide the panels dock",
        reach: "toolbar sidebar button, View menu, Ctrl+B",
        keys: &[
            (Source::ToolbarAction, "ToggleDock"),
            (Source::Hotkey, "DOCK_SHORTCUT"),
        ],
        mapping: PENDING_SURFACE,
    },
    UiBehaviour {
        id: "layer.bubbles.toggle",
        title: "Switch the aggression bubbles on or off",
        reach: "toolbar LAYERS group, pane right-click layer menu",
        keys: &[
            (Source::ToolbarAction, "SetBubbles"),
            (Source::LayerToggle, "Bubbles"),
        ],
        mapping: PENDING_LAYER,
    },
    UiBehaviour {
        id: "layer.footprint.settings.open",
        title: "Open the footprint's settings window",
        reach: "right-click the toolbar's footprint button",
        keys: &[(Source::ToolbarAction, "OpenFootprintSettings")],
        mapping: PENDING_SURFACE,
    },
    UiBehaviour {
        id: "layer.footprint.toggle",
        title: "Switch the candle footprint on or off",
        reach: "toolbar LAYERS group, pane right-click layer menu",
        keys: &[
            (Source::ToolbarAction, "SetFootprint"),
            (Source::LayerToggle, "Footprint"),
        ],
        mapping: PENDING_LAYER,
    },
    UiBehaviour {
        id: "layer.heatmap.toggle",
        title: "Switch the L2 depth map on or off",
        reach: "toolbar LAYERS group, pane right-click layer menu",
        keys: &[
            (Source::ToolbarAction, "SetHeatmap"),
            (Source::LayerToggle, "Heatmap"),
        ],
        mapping: PENDING_LAYER,
    },
    UiBehaviour {
        id: "layer.live_strip.toggle",
        title: "Switch the live depth strip on or off",
        reach: "toolbar LAYERS group, pane right-click layer menu",
        keys: &[
            (Source::ToolbarAction, "SetLiveStrip"),
            (Source::LayerToggle, "LiveStrip"),
        ],
        mapping: PENDING_LAYER,
    },
    // ---- History --------------------------------------------------------
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
    // ---- Indicators ------------------------------------------------------
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
    // ---- Paper trading ---------------------------------------------------
    UiBehaviour {
        id: "trade.aim.bracket",
        title: "Send an entry with its stop and target attached",
        reach: "the ticket's bracket fields, and the aim on the chart",
        keys: &[(
            Source::Authored,
            "the ticket bracket fields and the aim on the chart",
        )],
        mapping: capability!("trade.order.bracket"),
    },
    UiBehaviour {
        id: "trade.instrument.money.set",
        title: "Declare what one point of an instrument is worth",
        reach: "the Trading panel's instrument section",
        keys: &[(Source::Authored, "the Trading panel instrument section")],
        mapping: capability!("trade.instrument.set_money"),
    },
    UiBehaviour {
        id: "trade.market.buy",
        title: "Buy at market, simulated",
        reach: "toolbar trade group, Trading panel, Shift+B",
        keys: &[
            (Source::ToolbarAction, "PaperBuy"),
            (Source::Hotkey, "PAPER_BUY_SHORTCUT"),
        ],
        mapping: capability!("trade.order.place"),
    },
    UiBehaviour {
        id: "trade.market.sell",
        title: "Sell at market, simulated",
        reach: "toolbar trade group, Trading panel, Shift+S",
        keys: &[
            (Source::ToolbarAction, "PaperSell"),
            (Source::Hotkey, "PAPER_SELL_SHORTCUT"),
        ],
        mapping: capability!("trade.order.place"),
    },
    UiBehaviour {
        id: "trade.order.place_at_price",
        title: "Rest an order at the price under the pointer",
        reach: "the canvas right-click menu's trade section",
        keys: &[(
            Source::Authored,
            "the canvas right-click menu trade section, resolved per click",
        )],
        mapping: capability!("trade.order.place"),
    },
    UiBehaviour {
        id: "trade.orders.cancel_all",
        title: "Cancel every working order without trading",
        reach: "Trading panel, Shift+X",
        keys: &[(Source::Hotkey, "PAPER_CANCEL_SHORTCUT")],
        mapping: capability!("trade.order.cancel"),
    },
    UiBehaviour {
        id: "trade.position.close",
        title: "Exit the open position at the next print",
        reach: "toolbar trade group, Trading panel",
        keys: &[(Source::ToolbarAction, "PaperClose")],
        mapping: capability!("trade.order.place"),
    },
    UiBehaviour {
        id: "trade.position.flatten",
        title: "Close the position and cancel every working order",
        reach: "Trading panel, Shift+F",
        keys: &[(Source::Hotkey, "PAPER_FLATTEN_SHORTCUT")],
        mapping: excluded!(
            PendingCapability,
            "`trade.order.place` and `trade.order.cancel` do the two halves, and a caller that \
             runs them in sequence is not flat between them. The one-shot has no capability. \
             Tracked in issue 401"
        ),
    },
    UiBehaviour {
        id: "trade.position.reverse",
        title: "Reverse the simulated position",
        reach: "Trading panel, Shift+R",
        keys: &[(Source::Hotkey, "PAPER_REVERSE_SHORTCUT")],
        mapping: excluded!(
            PendingCapability,
            "no capability reverses; a caller would have to size the flip itself from a read \
             that may already be stale. Tracked in issue 401"
        ),
    },
    UiBehaviour {
        id: "trade.ticket.risk.set",
        title: "Say what one trade may lose",
        reach: "the Trading panel's risk section",
        keys: &[(Source::Authored, "the Trading panel risk field")],
        mapping: capability!("trade.risk.set"),
    },
    UiBehaviour {
        id: "trade.ticket.ruler.set",
        title: "Walk the projected stop and target out from the aim",
        reach: "the ruler wheel on the chart's aim",
        keys: &[(Source::Authored, "the ruler wheel on the chart aim")],
        mapping: capability!("trade.ruler.set"),
    },
    UiBehaviour {
        id: "trade.ticket.strategy.select",
        title: "Choose the ticket's exit ladder",
        reach: "the Trading panel's strategy selector",
        keys: &[(Source::Authored, "the Trading panel strategy selector")],
        mapping: capability!("trade.strategy.select"),
    },
    // ---- Feed recovery ----------------------------------------------------
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
    // ---- The tool rail ----------------------------------------------------
    UiBehaviour {
        id: "toolrail.dock.set",
        title: "Park the drawing rail on the left, top or bottom edge",
        reach: "View menu, Drawing toolbar, and dragging the rail grip",
        keys: &[
            (Source::ToolboxDock, "Left"),
            (Source::ToolboxDock, "Top"),
            (Source::ToolboxDock, "Bottom"),
            (Source::MenuEntry, "Drawing toolbar"),
        ],
        mapping: PENDING_SURFACE,
    },
    UiBehaviour {
        id: "toolrail.visible.toggle",
        title: "Show or hide the drawing rail",
        reach: "View menu",
        keys: &[(
            Source::Authored,
            "a View menu entry whose label the source computes, so no literal to claim",
        )],
        mapping: PENDING_SURFACE,
    },
    UiBehaviour {
        id: "tool.crosshair",
        title: "Arm the crosshair",
        reach: "tool rail, key 2",
        keys: &[(Source::RailTool, "Crosshair")],
        mapping: excluded!(
            PendingCapability,
            "arming a tool changes what the next click does, and no capability arms one. \
             Tracked in issue 401"
        ),
    },
    UiBehaviour {
        id: "tool.pointer",
        title: "Arm the pointer — pan, zoom, select and move",
        reach: "tool rail, key 1, Escape",
        keys: &[(Source::RailTool, "Pointer")],
        mapping: excluded!(
            PendingCapability,
            "arming a tool changes what the next click does, and no capability arms one. \
             Tracked in issue 401"
        ),
    },
    UiBehaviour {
        id: "tool.anchored-vwap",
        title: "Arm the anchored VWAP",
        reach: "tool rail, family flyout, canvas right-click",
        keys: &[(Source::DrawingTool, "anchored-vwap")],
        mapping: PENDING_DRAWING_TOOL,
    },
    UiBehaviour {
        id: "tool.arrow",
        title: "Arm the arrow",
        reach: "tool rail, family flyout, canvas right-click",
        keys: &[(Source::DrawingTool, "arrow")],
        mapping: capability!("annotate.arrow.create"),
    },
    UiBehaviour {
        id: "tool.arrow-mark-down",
        title: "Arm the down arrow mark",
        reach: "tool rail, family flyout, canvas right-click",
        keys: &[(Source::DrawingTool, "arrow-mark-down")],
        mapping: PENDING_DRAWING_TOOL,
    },
    UiBehaviour {
        id: "tool.arrow-mark-up",
        title: "Arm the up arrow mark",
        reach: "tool rail, family flyout, canvas right-click",
        keys: &[(Source::DrawingTool, "arrow-mark-up")],
        mapping: PENDING_DRAWING_TOOL,
    },
    UiBehaviour {
        id: "tool.brush",
        title: "Arm the freehand brush",
        reach: "tool rail, family flyout, canvas right-click",
        keys: &[(Source::DrawingTool, "brush")],
        mapping: PENDING_DRAWING_TOOL,
    },
    UiBehaviour {
        id: "tool.date-range",
        title: "Arm the date range",
        reach: "tool rail, family flyout, canvas right-click",
        keys: &[(Source::DrawingTool, "date-range")],
        mapping: PENDING_DRAWING_TOOL,
    },
    UiBehaviour {
        id: "tool.ellipse",
        title: "Arm the ellipse",
        reach: "tool rail, family flyout, canvas right-click",
        keys: &[(Source::DrawingTool, "ellipse")],
        mapping: PENDING_DRAWING_TOOL,
    },
    UiBehaviour {
        id: "tool.extended-line",
        title: "Arm the extended line",
        reach: "tool rail, family flyout, canvas right-click",
        keys: &[(Source::DrawingTool, "extended-line")],
        mapping: PENDING_DRAWING_TOOL,
    },
    UiBehaviour {
        id: "tool.fib-extension",
        title: "Arm the Fibonacci extension",
        reach: "tool rail, family flyout, canvas right-click",
        keys: &[(Source::DrawingTool, "fib-extension")],
        mapping: PENDING_DRAWING_TOOL,
    },
    UiBehaviour {
        id: "tool.fib-retracement",
        title: "Arm the Fibonacci retracement",
        reach: "tool rail, family flyout, canvas right-click",
        keys: &[(Source::DrawingTool, "fib-retracement")],
        mapping: PENDING_DRAWING_TOOL,
    },
    UiBehaviour {
        id: "tool.fixed-range-profile",
        title: "Arm the fixed-range volume profile",
        reach: "tool rail, family flyout, canvas right-click",
        keys: &[(Source::DrawingTool, "fixed-range-profile")],
        mapping: PENDING_DRAWING_TOOL,
    },
    UiBehaviour {
        id: "tool.horizontal-line",
        title: "Arm the horizontal line",
        reach: "tool rail, family flyout, canvas right-click",
        keys: &[(Source::DrawingTool, "horizontal-line")],
        mapping: PENDING_DRAWING_TOOL,
    },
    UiBehaviour {
        id: "tool.horizontal-ray",
        title: "Arm the horizontal ray",
        reach: "tool rail, family flyout, canvas right-click",
        keys: &[(Source::DrawingTool, "horizontal-ray")],
        mapping: PENDING_DRAWING_TOOL,
    },
    UiBehaviour {
        id: "tool.measure",
        title: "Arm the measure",
        reach: "tool rail, family flyout, canvas right-click",
        keys: &[(Source::DrawingTool, "measure")],
        mapping: PENDING_DRAWING_TOOL,
    },
    UiBehaviour {
        id: "tool.parallel-channel",
        title: "Arm the parallel channel",
        reach: "tool rail, family flyout, canvas right-click",
        keys: &[(Source::DrawingTool, "parallel-channel")],
        mapping: PENDING_DRAWING_TOOL,
    },
    UiBehaviour {
        id: "tool.price-range",
        title: "Arm the price range",
        reach: "tool rail, family flyout, canvas right-click",
        keys: &[(Source::DrawingTool, "price-range")],
        mapping: PENDING_DRAWING_TOOL,
    },
    UiBehaviour {
        id: "tool.ray",
        title: "Arm the ray",
        reach: "tool rail, family flyout, canvas right-click",
        keys: &[(Source::DrawingTool, "ray")],
        mapping: PENDING_DRAWING_TOOL,
    },
    UiBehaviour {
        id: "tool.rectangle",
        title: "Arm the rectangle",
        reach: "tool rail, family flyout, canvas right-click",
        keys: &[(Source::DrawingTool, "rectangle")],
        mapping: capability!("annotate.zone.create"),
    },
    UiBehaviour {
        id: "tool.text",
        title: "Arm the text label",
        reach: "tool rail, family flyout, canvas right-click",
        keys: &[(Source::DrawingTool, "text")],
        mapping: capability!("annotate.label.create"),
    },
    UiBehaviour {
        id: "tool.trend-line",
        title: "Arm the trend line",
        reach: "tool rail, family flyout, canvas right-click",
        keys: &[(Source::DrawingTool, "trend-line")],
        mapping: PENDING_DRAWING_TOOL,
    },
    UiBehaviour {
        id: "tool.triangle",
        title: "Arm the triangle",
        reach: "tool rail, family flyout, canvas right-click",
        keys: &[(Source::DrawingTool, "triangle")],
        mapping: PENDING_DRAWING_TOOL,
    },
    UiBehaviour {
        id: "tool.vertical-line",
        title: "Arm the vertical line",
        reach: "tool rail, family flyout, canvas right-click",
        keys: &[(Source::DrawingTool, "vertical-line")],
        mapping: PENDING_DRAWING_TOOL,
    },
    // ---- Objects already on the chart -------------------------------------
    UiBehaviour {
        id: "attention.mark.create",
        title: "Take a mark of what is under the pointer",
        reach: "Ctrl+M",
        keys: &[(Source::Hotkey, "MARK_SHORTCUT")],
        mapping: capability!("attention.mark.create"),
    },
    UiBehaviour {
        id: "drawing.remove",
        title: "Delete a drawing",
        reach: "the object context bar, the canvas right-click menu, Delete",
        keys: &[(
            Source::Authored,
            "the object context bar and the canvas right-click menu, resolved per click",
        )],
        mapping: capability!("annotate.remove"),
    },
    UiBehaviour {
        id: "drawing.rename",
        title: "Rename a drawing",
        reach: "the canvas right-click menu, drawing section",
        keys: &[(
            Source::Authored,
            "the rename box inside the canvas right-click menu",
        )],
        mapping: excluded!(
            PendingCapability,
            "`annotate.*` places and removes; nothing edits an object that already exists. \
             Tracked in issue 401"
        ),
    },
    UiBehaviour {
        id: "drawing.select_and_move",
        title: "Select a drawing and drag it, or one of its handles",
        reach: "primary click and drag on the canvas",
        keys: &[(Source::Authored, "a primary click and drag on the canvas")],
        mapping: excluded!(
            PendingCapability,
            "an object can be placed and removed by capability and not moved, so an operator \
             corrects a level by deleting and replacing it. Tracked in issue 401"
        ),
    },
    // ---- The saved cockpit -------------------------------------------------
    UiBehaviour {
        id: "workspace.bookmark.delete",
        title: "Forget a named arrangement",
        reach: "Workspace menu, Delete",
        keys: &[(Source::MenuEntry, "Delete")],
        mapping: PENDING_WORKSPACE,
    },
    UiBehaviour {
        id: "workspace.bookmark.open",
        title: "Reopen a named arrangement",
        reach: "Workspace menu, Open",
        keys: &[(Source::MenuEntry, "Open")],
        mapping: PENDING_WORKSPACE,
    },
    UiBehaviour {
        id: "workspace.bookmark.save",
        title: "Keep these tabs and panels under a name",
        reach: "Workspace menu, Save as…",
        keys: &[(Source::MenuEntry, "Save as…")],
        mapping: PENDING_WORKSPACE,
    },
    UiBehaviour {
        id: "workspace.export",
        title: "Write the whole cockpit to a file",
        reach: "Workspace menu, Export to file…",
        keys: &[(Source::MenuEntry, "Export to file…")],
        mapping: AUTHORITY_ARBITRARY_PATH,
    },
    UiBehaviour {
        id: "workspace.import",
        title: "Open a cockpit from a file",
        reach: "Workspace menu, Open from file…",
        keys: &[(Source::MenuEntry, "Open from file…")],
        mapping: AUTHORITY_ARBITRARY_PATH,
    },
    UiBehaviour {
        id: "workspace.recent.open",
        title: "Reopen a cockpit file opened before",
        reach: "Workspace menu, Open recent",
        keys: &[(Source::MenuEntry, "Open recent")],
        mapping: AUTHORITY_ARBITRARY_PATH,
    },
    UiBehaviour {
        id: "workspace.reveal",
        title: "Open the folder the cockpit is kept in",
        reach: "Workspace menu, Show where it is saved",
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
        reach: "Workspace menu, Ctrl+Shift+S",
        keys: &[
            (Source::Hotkey, "SAVE_WORKSPACE_SHORTCUT"),
            (Source::MenuEntry, "Save workspace"),
        ],
        mapping: PENDING_WORKSPACE,
    },
    UiBehaviour {
        id: "workspace.save_on_exit",
        title: "Keep the arrangement automatically when the window closes",
        reach: "Workspace menu",
        keys: &[(Source::MenuEntry, "Save on exit")],
        mapping: PENDING_WORKSPACE,
    },
    UiBehaviour {
        id: "workspace.startup.reset",
        title: "Forget the saved workspace and open on the configured default",
        reach: "Workspace menu, Reset startup layout",
        keys: &[(Source::MenuEntry, "Reset startup layout")],
        mapping: PENDING_WORKSPACE,
    },
    // ---- The rest of the chrome --------------------------------------------
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

/// Registry entries that are deliberately **not** behaviours.
///
/// The `super::drift` guard demands a row for every registered entry, and
/// these are the entries where a row would be a lie: a submenu that holds
/// other entries and performs nothing itself, a hook that opens a menu so a
/// capture can photograph it, a "nothing was clicked" enum variant.
///
/// Each carries its reason, for the same reason `hooks::NOT_HOOKS` does: an
/// allowlist is how a parity guard is quietly defeated, and a reader who
/// disagrees with an entry needs something to disagree with.
pub(crate) const NOT_A_BEHAVIOUR: &[(Source, &str, &str)] = &[
    (
        Source::MenuEntry,
        "File",
        "a top-level menu; it holds entries and performs nothing",
    ),
    (
        Source::MenuEntry,
        "View",
        "a top-level menu; it holds entries and performs nothing",
    ),
    (
        Source::MenuEntry,
        "Workspace",
        "a top-level menu; it holds entries and performs nothing",
    ),
    (
        Source::MenuEntry,
        "Tools",
        "a top-level menu; it holds entries and performs nothing",
    ),
    (
        Source::MenuEntry,
        "Help",
        "a top-level menu; it holds entries and performs nothing",
    ),
    (
        Source::MenuEntry,
        "Layouts",
        "a submenu holding the layout tabs and the three edits the strip's own menu holds; \
         each of those is a row of its own",
    ),
    (
        Source::RailTool,
        "Drawing",
        "the rail's slot for the drawing registry rather than a tool of its own; every drawing tool behind it is a `tool.*` row",
    ),
    (
        Source::NoticeAction,
        "None",
        "the feed notice's \"nothing was clicked\" variant: an absence, not a behaviour",
    ),
    (
        Source::ScriptedMenu,
        "workspace",
        "a capture hook that opens the Workspace menu so a screenshot can see it. It performs \
         nothing the menu's own entries do not",
    ),
    (
        Source::ScriptedMenu,
        "history",
        "a capture hook that opens the toolbar's history caret. The behaviour behind it is \
         `history.reach.set`",
    ),
];
