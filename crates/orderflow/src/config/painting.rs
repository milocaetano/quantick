//! Shared bubble paint measurements; toolkit colors remain with the painter.
/// Label font size as a fraction of the bubble radius, and the range it is
/// held to: too small to read is pointless, too large stops fitting inside.
pub const LABEL_FONT_SCALE: f32 = 0.68;
/// See [`LABEL_FONT_SCALE`].
pub const LABEL_MIN_FONT_PX: f32 = 8.0;
/// See [`LABEL_FONT_SCALE`].
pub const LABEL_MAX_FONT_PX: f32 = 11.0;

/// Angle a pie starts at: straight up. Screen y grows downward, so a positive
/// sweep from here runs clockwise, the direction a pie chart is read in.
pub const PIE_START_ANGLE: f32 = -std::f32::consts::FRAC_PI_2;

/// Gap between a folded bubble's disc and the ring that marks it as a fold,
/// in points. Wide enough to read as a separate ring at dot size, narrow
/// enough that two neighbouring folds do not run into each other.
pub const FOLD_RING_GAP: f32 = 2.0;
/// Stroke width of that ring.
pub const FOLD_RING_WIDTH: f32 = 1.0;

/// Its alpha. Below the rim's, because a fold ring is a caveat about the mark
/// and not part of the mark: it has to be findable without competing with the
/// pressure the bubble is there to show.
pub const FOLD_RING_ALPHA: f32 = 0.55;

/// Dash and gap, in pixels, of the line dividing the forming bar's candle from
/// its live lane. Fine and airy: it marks where the present begins, and a solid
/// rule there would read as a wall in the data.
pub const LANE_DIVIDER_DASH_PX: f32 = 3.0;
/// See [`LANE_DIVIDER_DASH_PX`].
pub const LANE_DIVIDER_GAP_PX: f32 = 5.0;

/// Dash and gap of the live-time line. Tighter than the divider's, so the two
/// never read as the same mark even where they nearly touch.
pub const LANE_NOW_DASH_PX: f32 = 6.0;
/// See [`LANE_NOW_DASH_PX`].
pub const LANE_NOW_GAP_PX: f32 = 3.0;
/// Stroke width shared by both lane marks.
pub const LANE_MARK_WIDTH_PX: f32 = 1.0;

/// Pixel slack for deciding that a gap boundary coincides with the chart edge.
/// Gap bounds arrive as normalized floats scaled into screen space, so an
/// exact comparison would miss by a rounding bit and draw a stray frame line.
pub const GAP_EDGE_EPSILON: f32 = 0.5;

/// Gap between the leading span's label and the divider it annotates. Small
/// enough that the text reads as belonging to the line rather than floating.
pub const GAP_LABEL_INSET_PX: f32 = 6.0;

/// How far down the canvas the stack above the key may push it, as a share
/// of the canvas height.
///
/// Past this the key would be reading as part of the chart rather than as its
/// key — and a canvas whose top half is chips has no room for it at all, so it
/// stands down instead of printing over them. Chrome yields to the chart;
/// nothing it says is data (the layers keep drawing, and the trader can bring
/// it back from the right-click menu).
pub const MAX_LEGEND_TOP_INSET_FRAC: f32 = 0.5;
/// The key's hairline, included in its published painted footprint.
pub const LEGEND_BORDER_WIDTH_PX: f32 = 0.75;

/// Normalized sizes of the two sample prints in the settings preview: one
/// near full size and one routine print, so the radius range is visible.
pub const PREVIEW_LARGE_PRINT_SIZE: f32 = 0.85;
/// See [`PREVIEW_LARGE_PRINT_SIZE`].
pub const PREVIEW_SMALL_PRINT_SIZE: f32 = 0.45;

/// Matched fraction of the preview's consuming print. Mid-range, so the
/// impact ring shows neither its floor nor its ceiling.
pub const PREVIEW_MATCHED_FRACTION: f32 = 0.6;

/// Buy share of the preview's summarized print. Lopsided rather than even, so
/// the two sectors are visibly unequal and the mark reads as a proportion.
pub const PREVIEW_SUMMARY_BUY_SHARE: f32 = 0.62;

/// A muted hairline keeps execution order readable behind the volume discs:
/// its width in pixels and its unmultiplied RGBA colour.
pub const PATH_WIDTH_PX: f32 = 0.8;
pub const PATH_RGBA: [u8; 4] = [142, 166, 177, 95];

/// FLOW caption text size, in points.
pub const FLOW_CAPTION_FONT_PX: f32 = 10.0;
/// FLOW caption distance from the history pane's left edge, in pixels.
pub const FLOW_CAPTION_INSET_PX: f32 = 8.0;
/// Width the FLOW caption leaves free of wrapped text: its inset on both sides, in pixels.
pub const FLOW_CAPTION_WRAP_MARGIN_PX: f32 = 2.0 * FLOW_CAPTION_INSET_PX;

/// Chart width below which the tape header legend hides, in pixels.
pub const LEGEND_TAPE_MIN_CHART_WIDTH_PX: f32 = 90.0;
/// Chart width below which the chart legend hides, in pixels.
pub const LEGEND_MIN_CHART_WIDTH_PX: f32 = 150.0;
/// Gap between a legend glyph and its label, in pixels.
pub const LEGEND_GLYPH_GAP_PX: f32 = 5.0;
/// Space a legend entry keeps after its label, in pixels.
pub const LEGEND_ENTRY_PADDING_PX: f32 = 10.0;
/// Legend panel distance from the chart corner, in pixels.
pub const LEGEND_OUTER_MARGIN_PX: f32 = 6.0;
/// Legend panel padding around its entries, in pixels.
pub const LEGEND_INNER_MARGIN_PX: f32 = 7.0;
/// Narrowest legend panel, in pixels.
pub const LEGEND_MIN_PANEL_WIDTH_PX: f32 = 120.0;
/// Narrowest legend content column, in pixels.
pub const LEGEND_MIN_CONTENT_WIDTH_PX: f32 = 100.0;
/// Legend row height in the tape header, in pixels.
pub const LEGEND_TAPE_ROW_HEIGHT_PX: f32 = 14.0;
/// Legend row height on the chart, in pixels.
pub const LEGEND_ROW_HEIGHT_PX: f32 = 17.0;
/// Gap between legend entries and rows, in pixels.
pub const LEGEND_ENTRY_GAP_PX: f32 = 3.0;
/// Legend panel corner radius, in pixels.
pub const LEGEND_CORNER_RADIUS_PX: f32 = 4.0;
/// Legend label size, in points.
pub const LEGEND_FONT_PX: f32 = 10.0;
/// Height of one legend entry, in pixels: its label centres in it, its glyph on its midline.
pub const LEGEND_ENTRY_HEIGHT_PX: f32 = 14.0;
/// Width of the liquidity ramp glyph, in pixels.
pub const LEGEND_HEAT_WIDTH_PX: f32 = 42.0;
/// Height of the liquidity ramp glyph, in pixels.
pub const LEGEND_HEAT_HEIGHT_PX: f32 = 8.0;
/// Width of a buy or sell aggression dot glyph, in pixels.
pub const LEGEND_DOT_WIDTH_PX: f32 = 12.0;
/// Width of a depletion, L2 reduction or L2 gap band glyph, in pixels.
pub const LEGEND_BAND_WIDTH_PX: f32 = 18.0;

/// Gap between the retained-edge line and its label, in pixels.
pub const RETAINED_EDGE_LABEL_GAP_PX: f32 = 4.0;
/// Retained-edge label distance below the lane top, in pixels.
pub const RETAINED_EDGE_LABEL_TOP_PX: f32 = 20.0;
