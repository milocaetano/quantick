//! The order-flow renderer's tunables: sizes, gaps, alphas and colours its
//! painters share. One file, so retuning a look touches one place.

use eframe::egui;

/// Label font size as a fraction of the bubble radius, and the range it is
/// held to: too small to read is pointless, too large stops fitting inside.
pub(super) const LABEL_FONT_SCALE: f32 = 0.68;
/// See [`LABEL_FONT_SCALE`].
pub(super) const LABEL_MIN_FONT_PX: f32 = 8.0;
/// See [`LABEL_FONT_SCALE`].
pub(super) const LABEL_MAX_FONT_PX: f32 = 11.0;

/// How far a laid-out label may spill past the radius before it is dropped.
/// Wider than tall: a bubble is a circle, and text is a horizontal band across
/// its middle, where there is more room.
pub(super) const LABEL_MAX_WIDTH_SCALE: f32 = 1.78;
/// See [`LABEL_MAX_WIDTH_SCALE`].
pub(super) const LABEL_MAX_HEIGHT_SCALE: f32 = 1.45;
/// Drop shadow that keeps a label legible over any fill colour.
pub(super) const LABEL_SHADOW_OFFSET_PX: egui::Vec2 = egui::vec2(1.0, 1.0);
/// See [`LABEL_SHADOW_OFFSET_PX`].
pub(super) const LABEL_SHADOW_ALPHA: u8 = 190;

/// Angle a pie starts at: straight up. Screen y grows downward, so a positive
/// sweep from here runs clockwise, the direction a pie chart is read in.
pub(crate) const PIE_START_ANGLE: f32 = -std::f32::consts::FRAC_PI_2;

/// Gap between a folded bubble's disc and the ring that marks it as a fold,
/// in points. Wide enough to read as a separate ring at dot size, narrow
/// enough that two neighbouring folds do not run into each other.
pub(super) const FOLD_RING_GAP: f32 = 2.0;
/// Stroke width of that ring.
pub(super) const FOLD_RING_WIDTH: f32 = 1.0;

/// Its alpha. Below the rim's, because a fold ring is a caveat about the mark
/// and not part of the mark: it has to be findable without competing with the
/// pressure the bubble is there to show.
pub(super) const FOLD_RING_ALPHA: f32 = 0.55;

/// FLOW volume uses its own palette, distinct from candle direction and native Tape.
pub(super) const FLOW_BUY: egui::Color32 = egui::Color32::from_rgb(112, 185, 244);
pub(super) const FLOW_SELL: egui::Color32 = egui::Color32::from_rgb(232, 175, 99);

/// Dash and gap, in pixels, of the line dividing the forming bar's candle from
/// its live lane. Fine and airy: it marks where the present begins, and a solid
/// rule there would read as a wall in the data.
pub(super) const LANE_DIVIDER_DASH_PX: f32 = 3.0;
/// See [`LANE_DIVIDER_DASH_PX`].
pub(super) const LANE_DIVIDER_GAP_PX: f32 = 5.0;

/// Dash and gap of the live-time line. Tighter than the divider's, so the two
/// never read as the same mark even where they nearly touch.
pub(super) const LANE_NOW_DASH_PX: f32 = 6.0;
/// See [`LANE_NOW_DASH_PX`].
pub(super) const LANE_NOW_GAP_PX: f32 = 3.0;
/// Stroke width shared by both lane marks.
pub(super) const LANE_MARK_WIDTH_PX: f32 = 1.0;

/// Pixel slack for deciding that a gap boundary coincides with the chart edge.
/// Gap bounds arrive as normalized floats scaled into screen space, so an
/// exact comparison would miss by a rounding bit and draw a stray frame line.
pub(super) const GAP_EDGE_EPSILON: f32 = 0.5;

/// Gap between the leading span's label and the divider it annotates. Small
/// enough that the text reads as belonging to the line rather than floating.
pub(super) const GAP_LABEL_INSET_PX: f32 = 6.0;

/// How far down the canvas the stack above the key may push it, as a share
/// of the canvas height.
///
/// Past this the key would be reading as part of the chart rather than as its
/// key — and a canvas whose top half is chips has no room for it at all, so it
/// stands down instead of printing over them. Chrome yields to the chart;
/// nothing it says is data (the layers keep drawing, and the trader can bring
/// it back from the right-click menu).
pub(super) const MAX_LEGEND_TOP_INSET_FRAC: f32 = 0.5;
/// The key's hairline, included in its published painted footprint.
pub(super) const LEGEND_BORDER_WIDTH_PX: f32 = 0.75;

/// Normalized sizes of the two sample prints in the settings preview: one
/// near full size and one routine print, so the radius range is visible.
pub(super) const PREVIEW_LARGE_PRINT_SIZE: f32 = 0.85;
/// See [`PREVIEW_LARGE_PRINT_SIZE`].
pub(super) const PREVIEW_SMALL_PRINT_SIZE: f32 = 0.45;

/// Matched fraction of the preview's consuming print. Mid-range, so the
/// impact ring shows neither its floor nor its ceiling.
pub(super) const PREVIEW_MATCHED_FRACTION: f32 = 0.6;

/// Buy share of the preview's summarized print. Lopsided rather than even, so
/// the two sectors are visibly unequal and the mark reads as a proportion.
pub(super) const PREVIEW_SUMMARY_BUY_SHARE: f32 = 0.62;

/// A muted hairline keeps execution order readable behind the volume discs:
/// its width in pixels and its unmultiplied RGBA colour.
pub(super) const PATH_WIDTH_PX: f32 = 0.8;
pub(super) const PATH_RGBA: [u8; 4] = [142, 166, 177, 95];

/// FLOW caption text size, in points.
pub(super) const FLOW_CAPTION_FONT_PX: f32 = 10.0;
/// FLOW caption distance from the history pane's left edge, in pixels.
pub(super) const FLOW_CAPTION_INSET_PX: f32 = 8.0;
/// Width the FLOW caption leaves free of wrapped text: its inset on both sides, in pixels.
pub(super) const FLOW_CAPTION_WRAP_MARGIN_PX: f32 = 2.0 * FLOW_CAPTION_INSET_PX;

/// Text size of the first-contour quantity label, in points.
pub(super) const FLOW_OPENING_LABEL_FONT_PX: f32 = 11.0;
/// Drop-shadow offset behind the first-contour quantity label, in pixels.
pub(super) const FLOW_OPENING_LABEL_SHADOW_OFFSET: egui::Vec2 = egui::vec2(1.0, 1.0);

/// Where the inspection heading starts inside the card, in pixels.
pub(super) const INSPECTION_TEXT_INSET: egui::Vec2 = egui::vec2(8.0, 4.0);
/// Horizontal padding of the FLOW inspection card: the text inset on both sides, in pixels.
pub(super) const INSPECTION_PADDING_X_PX: f32 = 2.0 * INSPECTION_TEXT_INSET.x;
/// Gap between the bottom of the inspection heading and its detail rows, in pixels.
pub(super) const INSPECTION_ROW_GAP_PX: f32 = 4.0;
/// Detail rows' distance below the card top, beyond the heading's own height, in pixels.
pub(super) const INSPECTION_DETAIL_TOP_PX: f32 = INSPECTION_TEXT_INSET.y + INSPECTION_ROW_GAP_PX;
/// Space below the inspection detail rows, in pixels.
pub(super) const INSPECTION_BOTTOM_PX: f32 = 6.0;
/// Card height beyond its text: top inset, row gap and bottom space, in pixels.
pub(super) const INSPECTION_PADDING_Y_PX: f32 = INSPECTION_DETAIL_TOP_PX + INSPECTION_BOTTOM_PX;
/// Narrowest text column worth an inspection card; below it none is drawn, in pixels.
pub(super) const INSPECTION_MIN_TEXT_WIDTH_PX: f32 = 80.0;
/// Inspection card heading size, in points.
pub(super) const INSPECTION_HEADING_FONT_PX: f32 = 11.0;
/// Inspection card detail rows size, in points.
pub(super) const INSPECTION_DETAIL_FONT_PX: f32 = 10.0;
/// Gap between the pointer and a card placed above it, in pixels.
pub(super) const INSPECTION_GAP_ABOVE_PX: f32 = 8.0;
/// Offset of a card placed right of and below the pointer, in pixels.
pub(super) const INSPECTION_POINTER_OFFSET_PX: f32 = 12.0;
/// Inspection card corner radius, in pixels.
pub(super) const INSPECTION_CORNER_RADIUS_PX: f32 = 4.0;

/// Chart width below which the tape header legend hides, in pixels.
pub(super) const LEGEND_TAPE_MIN_CHART_WIDTH_PX: f32 = 90.0;
/// Chart width below which the chart legend hides, in pixels.
pub(super) const LEGEND_MIN_CHART_WIDTH_PX: f32 = 150.0;
/// Gap between a legend glyph and its label, in pixels.
pub(super) const LEGEND_GLYPH_GAP_PX: f32 = 5.0;
/// Space a legend entry keeps after its label, in pixels.
pub(super) const LEGEND_ENTRY_PADDING_PX: f32 = 10.0;
/// Legend panel distance from the chart corner, in pixels.
pub(super) const LEGEND_OUTER_MARGIN_PX: f32 = 6.0;
/// Legend panel padding around its entries, in pixels.
pub(super) const LEGEND_INNER_MARGIN_PX: f32 = 7.0;
/// Narrowest legend panel, in pixels.
pub(super) const LEGEND_MIN_PANEL_WIDTH_PX: f32 = 120.0;
/// Narrowest legend content column, in pixels.
pub(super) const LEGEND_MIN_CONTENT_WIDTH_PX: f32 = 100.0;
/// Legend row height in the tape header, in pixels.
pub(super) const LEGEND_TAPE_ROW_HEIGHT_PX: f32 = 14.0;
/// Legend row height on the chart, in pixels.
pub(super) const LEGEND_ROW_HEIGHT_PX: f32 = 17.0;
/// Gap between legend entries and rows, in pixels.
pub(super) const LEGEND_ENTRY_GAP_PX: f32 = 3.0;
/// Legend panel corner radius, in pixels.
pub(super) const LEGEND_CORNER_RADIUS_PX: f32 = 4.0;
/// Legend label size, in points.
pub(super) const LEGEND_FONT_PX: f32 = 10.0;
/// Height of one legend entry, in pixels: its label centres in it, its glyph on its midline.
pub(super) const LEGEND_ENTRY_HEIGHT_PX: f32 = 14.0;
/// Width of the liquidity ramp glyph, in pixels.
pub(super) const LEGEND_HEAT_WIDTH_PX: f32 = 42.0;
/// Width of a buy or sell aggression dot glyph, in pixels.
pub(super) const LEGEND_DOT_WIDTH_PX: f32 = 12.0;
/// Width of a depletion, L2 reduction or L2 gap band glyph, in pixels.
pub(super) const LEGEND_BAND_WIDTH_PX: f32 = 18.0;

/// Canvas colour the settings preview dresses its sample against.
pub(super) const PREVIEW_CANVAS_BACKGROUND: egui::Color32 = egui::Color32::from_rgb(19, 23, 34);

/// Text size of the held-tape and retained-edge labels, in points.
pub(super) const PAST_TAPE_LABEL_FONT_PX: f32 = 11.0;
/// Where the held-tape label starts inside the lane, in pixels.
pub(super) const PAST_TAPE_LABEL_INSET: egui::Vec2 = egui::vec2(6.0, 4.0);
/// Alpha of the shade over the stretch before the retained tape begins (0-255).
pub(super) const PAST_TAPE_SHADE_ALPHA: u8 = 90;

/// Gap between the retained-edge line and its label, in pixels.
pub(super) const RETAINED_EDGE_LABEL_GAP_PX: f32 = 4.0;
/// Retained-edge label distance below the lane top, in pixels.
pub(super) const RETAINED_EDGE_LABEL_TOP_PX: f32 = 20.0;
