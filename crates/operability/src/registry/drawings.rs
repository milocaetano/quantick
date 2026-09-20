//! The tool rail, and the objects a trader has already put on the chart.
//!
//! Read [`super`] first: it says why the rows are written, how they are
//! joined, and what the drift guard does with them.

use super::PENDING_SURFACE;
use super::{ExclusionClass, Mapping, Source, UiBehaviour};

/// A drawing tool the rail arms, for which no capability places the object.
///
/// The six registered `annotate.*.create` capabilities place text, arrows,
/// rectangles, fixed-range profiles and both Fibonacci tools; the other
/// sixteen registered tools have no counterpart.
const PENDING_DRAWING_TOOL: Mapping = Mapping::Excluded {
    class: ExclusionClass::PendingCapability,
    reason: "no `annotate.*` capability places this shape; only text, arrow, rectangle, \
             fixed-range profile and the two Fibonacci tools have one. Tracked in issue 401",
};

/// The tool rail.
pub(super) const TOOL_RAIL: &[UiBehaviour] = &[
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
        mapping: capability!("annotate.fib_projection.create"),
    },
    UiBehaviour {
        id: "tool.fib-retracement",
        title: "Arm the Fibonacci retracement",
        reach: "tool rail, family flyout, canvas right-click",
        keys: &[(Source::DrawingTool, "fib-retracement")],
        mapping: capability!("annotate.fib_retracement.create"),
    },
    UiBehaviour {
        id: "tool.fixed-range-profile",
        title: "Arm the fixed-range volume profile",
        reach: "tool rail, family flyout, canvas right-click",
        keys: &[(Source::DrawingTool, "fixed-range-profile")],
        mapping: capability!("annotate.fixed_range_profile.create"),
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
];

/// Objects already on the chart.
pub(super) const OBJECTS: &[UiBehaviour] = &[
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
        id: "drawing.duplicate",
        title: "Copy a drawing: duplicate it in place, or copy it and paste it on any chart",
        reach: "the object context bar's Duplicate button, Ctrl+D; on the focused pane, Ctrl+C \
                then Ctrl+V or any native copy and paste (Windows: Ctrl+Insert, Shift+Insert)",
        keys: &[(
            Source::Authored,
            "the Duplicate button, Ctrl+D and the native copy and paste events are read per frame \
             by `app/drawing_input.rs` and the context bar, not entries in a hotkey registry",
        )],
        mapping: excluded!(
            PendingCapability,
            "no capability copies an object; `annotate.*` places four shapes afresh, so an \
             operator re-creates a copy rather than duplicating one. Tracked in issue 401"
        ),
    },
    UiBehaviour {
        id: "drawing.quick_range_profile",
        title: "Measure a range with a right-drag and turn it into a volume profile",
        reach: "a secondary-button drag on the price band with the Pointer tool, then the \
                range's action bar; a chart click or Escape dismisses the temporary range",
        keys: &[(
            Source::Authored,
            "a secondary-button drag read per frame by `pane/quick_range.rs`, and the action \
             bar `surfaces/drawing_chrome/quick_range.rs` lays out over it; neither is an \
             entry in a registry the drift guard walks",
        )],
        mapping: capability!("annotate.fixed_range_profile.create"),
    },
    UiBehaviour {
        id: "drawing.quick_range_fib_retracement",
        title: "Measure a range with a right-drag and turn it into a Fibonacci retracement",
        reach: "a secondary-button drag on the price band with the Pointer tool, then the \
                range's Fib Retracement action",
        keys: &[(
            Source::Authored,
            "the quick-range action bar delegates to the registered Fibonacci retracement tool",
        )],
        mapping: capability!("annotate.fib_retracement.create"),
    },
    UiBehaviour {
        id: "drawing.quick_range_fib_projection",
        title: "Measure a range with a right-drag and project it from the final point",
        reach: "a secondary-button drag on the price band with the Pointer tool, then the \
                range's Fib Projection action",
        keys: &[(
            Source::Authored,
            "the quick-range action bar delegates to the registered Fibonacci projection tool",
        )],
        mapping: capability!("annotate.fib_projection.create"),
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
];
