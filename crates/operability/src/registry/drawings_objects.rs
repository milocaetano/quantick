use super::{ExclusionClass, Mapping, Source, UiBehaviour};

pub(super) const BINDING: (usize, &[UiBehaviour]) = (11, ROWS);
const ROWS: &[UiBehaviour] = &[
    UiBehaviour {
        id: "drawing.text.properties",
        title: "Edit a note's words, font, size, color, background, border and wrapping",
        reach: "inline editor, text properties tab and size slider",
        keys: &[(Source::Authored, "the text properties widgets")],
        mapping: capability!("annotate.text.set"),
    },
    UiBehaviour {
        id: "drawing.text.repeat",
        title: "Place another independent note in the selected style",
        reach: "the text properties bar T button",
        keys: &[(Source::Authored, "the text properties bar repeat button")],
        mapping: capability!("annotate.text.repeat"),
    },
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
        reach: "the object context bar, the canvas right-click menu (drawing section and objects \
                submenu), the object manager, Delete",
        keys: &[(
            Source::Authored,
            "the object context bar, the canvas right-click menu and the object manager rows, \
             resolved per click",
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
        reach: "a secondary-button drag on the price band or an indicator pane with the \
                Pointer tool, then the range's Fib Retracement action",
        keys: &[(
            Source::Authored,
            "the quick-range action bar delegates to the registered Fibonacci retracement tool",
        )],
        mapping: capability!("annotate.fib_retracement.create"),
    },
    UiBehaviour {
        id: "drawing.quick_range_fib_projection",
        title: "Measure a range with a right-drag and project it from the final point",
        reach: "a secondary-button drag on the price band or an indicator pane with the \
                Pointer tool, then the range's Fib Projection action",
        keys: &[(
            Source::Authored,
            "the quick-range action bar delegates to the registered Fibonacci projection tool",
        )],
        mapping: capability!("annotate.fib_projection.create"),
    },
    UiBehaviour {
        id: "drawing.quick_range_rectangle",
        title: "Measure a range with a right-drag and turn its box into a rectangle",
        reach: "a secondary-button drag on the price band or an indicator pane with the \
                Pointer tool, then the range's Rectangle action",
        keys: &[(
            Source::Authored,
            "the quick-range action bar delegates to the registered rectangle tool",
        )],
        mapping: capability!("annotate.rectangle.create"),
    },
    UiBehaviour {
        id: "drawing.quick_range_trend_line",
        title: "Measure a range with a right-drag and turn its leg into a trend line",
        reach: "a secondary-button drag on the price band or an indicator pane with the \
                Pointer tool, then the range's Trend line action",
        keys: &[(
            Source::Authored,
            "the quick-range action bar delegates to the registered trend line tool",
        )],
        mapping: capability!("annotate.trend_line.create"),
    },
    UiBehaviour {
        id: "drawing.quick_range_horizontal_levels",
        title: "Measure a range with a right-drag and mark its high and low with horizontal lines",
        reach: "a secondary-button drag on the price band or an indicator pane with the \
                Pointer tool, then the range's Horizontal lines action; one line when the range has no height",
        keys: &[(
            Source::Authored,
            "the quick-range action bar places one registered horizontal line per distinct price",
        )],
        mapping: capability!("annotate.horizontal_levels.create"),
    },
    UiBehaviour {
        id: "drawing.quick_range_parallel_channel",
        title: "Measure a range with a right-drag and turn its leg into a parallel channel's base",
        reach: "a secondary-button drag on the price band or an indicator pane with the \
                Pointer tool, then the range's Parallel channel action, then a click above or below the leg that sets \
                the channel's width",
        keys: &[(
            Source::Authored,
            "the quick-range action bar arms the registered parallel channel tool with the leg \
             as its first two anchors; the third is the trader's click",
        )],
        mapping: capability!("annotate.parallel_channel.create"),
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
    UiBehaviour {
        id: "drawing.magnet.toggle",
        title: "Snap one drawing to the candle's open, high, low or close, whatever the rail says",
        reach: "the object context bar's magnet button, beside Lock",
        keys: &[(
            Source::Authored,
            "the context bar's magnet button is a widget, not a registry entry",
        )],
        mapping: capability!("annotate.drawing_magnet.set"),
    },
    UiBehaviour {
        id: "drawing.scope.toggle",
        title: "Show one drawing on every timeframe of its layout, or keep it to its chart; \
                and on every layout of its market",
        reach: "the object context bar's sharing button, beside Hide, and the inspector's \
                Show on all timeframes and All windows checkboxes",
        keys: &[(
            Source::Authored,
            "the context bar's sharing button and the inspector checkboxes are widgets, not \
             registry entries",
        )],
        mapping: capability!("annotate.drawing_scope.set"),
    },
    UiBehaviour {
        id: "drawing.hide",
        title: "Hide or show one drawing without deleting it",
        reach: "the object context bar, Alt+H, the canvas right-click menu (drawing section and \
                objects submenu), the object manager",
        keys: &[(
            Source::Authored,
            "Alt+H is read per frame by `app/drawing_controller/input.rs`; the Hide and Show \
             buttons on the menu and manager rows are widgets, not hotkey registry entries",
        )],
        mapping: excluded!(
            PendingCapability,
            "`annotate.*` places and removes; nothing toggles an object that already exists. \
             Tracked in issue 401"
        ),
    },
    UiBehaviour {
        id: "drawing.clear_all",
        title: "Delete every drawing on a chart, behind a count-bearing confirmation",
        reach: "the canvas right-click menu's clear objects entry; the object manager's Delete all",
        keys: &[(
            Source::Authored,
            "a menu entry and a manager button that ask the same question",
        )],
        mapping: excluded!(
            PendingCapability,
            "the annotate tier never discards work done by hand: `annotate.remove` refuses an \
             object the trader drew, so an operator removes its own marks one by one. Tracked \
             in issue 401"
        ),
    },
];
