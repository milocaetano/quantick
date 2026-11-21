use super::{ExclusionClass, Mapping, PENDING_SURFACE, Source, UiBehaviour};
const PENDING_DRAWING_TOOL: Mapping = Mapping::Excluded {
    class: ExclusionClass::PendingCapability,
    reason: "no `annotate.*` capability places this shape; only text, arrow, rectangle, \
             fixed-range profile, the two Fibonacci tools, trend line, horizontal line and \
             parallel channel have one. Tracked in issue 401",
};

pub(super) const BINDING: (usize, &[UiBehaviour]) = (10, ROWS);
const ROWS: &[UiBehaviour] = &[
    UiBehaviour {
        id: "toolrail.dock.set",
        title: "Park the drawing rail on the left, top or bottom edge",
        reach: "window menu, Panels, and dragging the rail grip",
        keys: &[
            (Source::ToolboxDock, "Left"),
            (Source::ToolboxDock, "Top"),
            (Source::ToolboxDock, "Bottom"),
            (Source::MenuEntry, "Left"),
            (Source::MenuEntry, "Top"),
            (Source::MenuEntry, "Bottom"),
        ],
        mapping: PENDING_SURFACE,
    },
    UiBehaviour {
        id: "toolrail.visible.toggle",
        title: "Show or hide the drawing rail",
        reach: "window menu",
        keys: &[(Source::MenuEntry, "Hidden")],
        mapping: PENDING_SURFACE,
    },
    UiBehaviour {
        id: "toolrail.magnet.toggle",
        title: "Snap drawing anchors to the candle's open, high, low or close",
        reach: "tool rail magnet button; the rail's More menu on its narrowest stage",
        keys: &[(
            Source::Authored,
            "the rail's magnet button and its More menu entry are widgets, not registry entries",
        )],
        mapping: capability!("annotate.magnet.set"),
    },
    UiBehaviour {
        id: "tool.crosshair",
        title: "Arm the crosshair",
        reach: "tool rail",
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
        reach: "tool rail, Escape",
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
        mapping: capability!("annotate.horizontal_levels.create"),
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
        mapping: capability!("annotate.parallel_channel.create"),
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
        mapping: capability!("annotate.zone.create", "annotate.rectangle.create"),
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
        mapping: capability!("annotate.trend_line.create"),
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
