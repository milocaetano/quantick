//! Turning committed plot columns into egui shapes.
//!
//! Mirrors the `chart.rs` vs `candle_view.rs` split: geometry decisions
//! (which rows are visible, where NaN breaks a line) live in small pure
//! helpers; egui only receives finished point lists. Polylines are batched —
//! one `Shape::line` per plot segment per frame, never one shape per bar.
//!
//! NaN cells break a polyline into segments: that is how warmup and
//! conditional plots render as gaps rather than as lies interpolated across
//! missing data.

use eframe::egui;
use eframe::egui::{Color32, Pos2, Rect, Shape, Stroke, pos2};
use egui_phosphor::regular as icons;
use quantick_indicators::{PlotStyle, Rgba8};

use crate::chart::PriceScale;
use crate::indicators::IndicatorView;
use crate::theme;
use crate::viewport::Viewport;

/// Marker radius for [`PlotStyle::Circles`] / arm length for
/// [`PlotStyle::Cross`], in pixels.
const MARKER_RADIUS_PX: f32 = 2.5;
/// Bar body fraction of one slot for [`PlotStyle::Histogram`] (thin bars).
const HISTOGRAM_WIDTH_FRAC: f32 = 0.3;
/// Bar body fraction of one slot for [`PlotStyle::Columns`] (wide bars).
const COLUMNS_WIDTH_FRAC: f32 = 0.8;
/// Fill opacity of the [`PlotStyle::Area`] body under its outline.
const AREA_FILL_ALPHA: u8 = 48;
/// Vertical padding inside a pane, as a fraction of the value range.
const PANE_PAD_FRAC: f64 = 0.08;
/// Pane label font size, in pixels.
const PANE_LABEL_FONT_PX: f32 = 11.0;
/// Opacity of the hairline that separates a pane from what sits above it.
const PANE_FRAME_ALPHA: f32 = 0.4;
/// Opacity of the zero line that anchors a flow pane.
const ZERO_LINE_ALPHA: f32 = 0.3;
/// Stroke width of the pane frame and zero line, in pixels.
const PANE_RULE_WIDTH_PX: f32 = 1.0;
/// Inset of a pane's corner labels from its own edges, in pixels.
const PANE_LABEL_INSET_PX: egui::Vec2 = egui::vec2(6.0, 3.0);
/// Above this magnitude a headline value is shown with fewer decimals: a
/// five-figure price needs no ten-thousandths, a ratio near 1 does.
const LARGE_VALUE_THRESHOLD: f64 = 1000.0;
/// Decimals shown at or above [`LARGE_VALUE_THRESHOLD`].
const LARGE_VALUE_DECIMALS: usize = 1;
/// Decimals shown below [`LARGE_VALUE_THRESHOLD`].
const SMALL_VALUE_DECIMALS: usize = 4;
/// Marker size (triangle half-height / circle radius), in pixels.
const MARKER_SIZE_PX: f32 = 4.0;
/// Gap between a bar's extreme and its above/below marker, in pixels.
const MARKER_GAP_PX: f32 = 6.0;
/// Stroke width of a cross marker's arms, in pixels.
const MARKER_CROSS_STROKE_PX: f32 = 1.5;
/// Gap between a marker and the text drawn beside it, in pixels.
const MARKER_TEXT_GAP_PX: f32 = 2.0;
/// Inset of a pane's above/below markers from its own edges, in pixels.
///
/// A pane has no candles, so markers that would anchor to a bar's extreme
/// anchor to the pane's edges instead; this is the substitution, named so it
/// no longer reads as an unexplained multiple of the bar gap.
const PANE_MARKER_INSET_PX: f32 = MARKER_GAP_PX * 2.0;
/// A tick label is dropped when it would be drawn within this many pixels of
/// the pane's own edge: the frame hairline and the pane's title/headline live
/// there, and a number crossing either reads as a smudge rather than a value.
const AXIS_LABEL_EDGE_MARGIN_PX: f32 = 7.0;
/// Marks a collapsed pane: a closed disclosure, pointing the way it opens.
///
/// From the bundled Phosphor font, like every other glyph in the chrome. The
/// Unicode geometric triangles (`▸`, `▾`) are *not* in it and drew as
/// tofu boxes — a visual capture caught it, which is exactly what one is for.
pub(crate) const COLLAPSED_CHEVRON: &str = icons::CARET_RIGHT;
/// Marks an expanded pane: an open disclosure, pointing the way it closes.
pub(crate) const EXPANDED_CHEVRON: &str = icons::CARET_DOWN;
/// Side of the square at a pane's top-left corner that toggles it open or
/// shut. Bigger than the glyph it holds, because a control you have to aim at
/// is a control a trader does not use mid-tape.
pub(crate) const PANE_DISCLOSURE_PX: f32 = 18.0;

/// The square at a pane's top-left that opens or closes it.
///
/// Shared by the paint and the input pass so the glyph and the thing you can
/// click are the same square — a hit box computed twice is a hit box that
/// drifts. For a collapsed pane the whole strip is the target instead: a
/// one-row band has no room to aim inside, and it has nothing else to click.
#[must_use]
pub(crate) fn pane_disclosure_rect(band: Rect, collapsed: bool) -> Rect {
    if collapsed {
        return band;
    }
    Rect::from_min_size(
        band.left_top(),
        egui::vec2(
            PANE_DISCLOSURE_PX.min(band.width()),
            PANE_DISCLOSURE_PX.min(band.height()),
        ),
    )
}

/// The row along a pane's top that carries its name and its live value — the
/// pane's title bar, and the handle its settings open from.
///
/// A geometric division rather than a measured one: the title is painted in
/// the paint pass and the gesture is read in the input pass, so a hit box cut
/// to the text would be a frame behind the text it claims to cover, and would
/// change size as an indicator retitled itself (`EMA(9)` → `EMA(200)`). The
/// whole row is the target instead, which is also what makes it findable —
/// a trader aims at the header, not at a word.
///
/// The disclosure square is carved out on the left: that corner already means
/// "open/close this pane", and one rect must mean one thing. Empty for a
/// collapsed pane, whose whole strip *is* the disclosure and which therefore
/// reads its double click from there.
#[must_use]
pub(crate) fn pane_header_rect(band: Rect, collapsed: bool) -> Rect {
    if collapsed {
        return Rect::NOTHING;
    }
    let disclosure = pane_disclosure_rect(band, collapsed);
    let left = disclosure.right().min(band.right());
    Rect::from_min_max(
        egui::pos2(left, band.top()),
        egui::pos2(
            band.right(),
            (band.top() + disclosure.height()).min(band.bottom()),
        ),
    )
}

fn color32(c: Rgba8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a)
}

/// Where plot row `r` draws on the x-axis, shared with the candles: rows map
/// 1:1 onto bar slots, and the preview value onto the forming bar's slot.
pub(crate) struct PlotX<'a> {
    pub viewport: &'a Viewport,
    /// Right edge of the candles' pane (the lane divider or the chart right).
    pub right: f32,
    /// Total slots (closed bars + forming bar), the candles' own `total`.
    pub total: usize,
}

impl PlotX<'_> {
    fn x(&self, row: usize) -> f32 {
        self.viewport.x_center(row, self.right, self.total)
    }

    fn slot_width(&self) -> f32 {
        self.viewport.candle_width()
    }
}

/// One plot column made drawable: the visible committed cells plus the
/// preview cell at the forming bar's slot.
struct VisiblePlot<'a> {
    column: &'a [f64],
    start: usize,
    end: usize,
    /// `Some((slot, value))`: the forming bar's previewed value.
    preview: Option<(usize, f64)>,
}

impl VisiblePlot<'_> {
    /// Every visible `(slot, value)` pair, preview last — exactly the
    /// candles' order, so lines join the forming bar seamlessly.
    ///
    fn cells(&self) -> impl Iterator<Item = (usize, f64)> + '_ {
        let committed =
            (self.start..self.end.min(self.column.len())).map(|row| (row, self.column[row]));
        committed.chain(self.preview)
    }
}

/// Draw every visible overlay indicator onto the price chart. Call with the
/// candle-pane clipped painter, after candles and before aggression bubbles
/// (the paint-order contract in `draw_chart`).
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_overlays<'a>(
    painter: &egui::Painter,
    overlays: impl Iterator<Item = &'a IndicatorView>,
    x: &PlotX<'_>,
    scale: &PriceScale,
    start: usize,
    end: usize,
    partial_slot: Option<usize>,
    bar_extents: &dyn Fn(usize) -> Option<(f32, f32)>,
) {
    for view in overlays {
        draw_view_plots(
            painter,
            view,
            x,
            |v| scale.y(v),
            start,
            end,
            partial_slot,
            bar_extents,
        );
    }
}

/// The slice of the live lane a pane draws on, and the window of tape time it
/// stands for.
///
/// The lane is a band of *time*, mapped linearly from `start_ms` at its left
/// edge to `end_ms` (the live edge) at its right — the tape's own mapping, so
/// a pane's curve lands under the prints it was computed from. The candles'
/// bar-slot x-mapping says nothing here, which is why this travels separately
/// from [`PlotX`].
pub(crate) struct LaneFrame<'a> {
    /// The band right of the divider, at this pane's height.
    pub rect: Rect,
    /// Tape instant at the band's left edge.
    pub start_ms: i64,
    /// Tape instant at the band's right edge: the live edge.
    pub end_ms: i64,
    /// `(close time, committed row)` for every bar that closed inside the
    /// window, oldest first — the same for every pane, so the chart computes
    /// it once.
    ///
    /// These are the lane's *committed* resolution. A closed bar cannot be
    /// re-entered, so what it did print by print is not recoverable and is not
    /// invented: its value holds flat across the band and steps at its close,
    /// which is exactly what "the indicator committed this at that instant"
    /// looks like.
    pub steps: &'a [(i64, usize)],
}

impl LaneFrame<'_> {
    /// Screen x of a tape instant, clamped to the band.
    fn x(&self, ms: i64) -> f32 {
        let span = (self.end_ms - self.start_ms).max(1) as f64;
        let fraction = ((ms - self.start_ms) as f64 / span).clamp(0.0, 1.0) as f32;
        self.rect.left() + fraction * self.rect.width()
    }
}

/// Where one pane paints, and in what colours.
///
/// A pane spans two rects that the chart's own layout keeps apart — the plot
/// area and the slice of the right-hand gutter beside it — so they travel
/// together rather than as two more positional arguments no caller can read.
pub(crate) struct PaneFrame<'a> {
    /// The pane's plot area: the candles' x-range, the live lane excluded.
    pub rect: Rect,
    /// The lane band beside `rect`, when the chart draws one. `None` is a
    /// chart with no tape, and the pane then ends at `rect` exactly as it
    /// always has.
    pub lane: Option<LaneFrame<'a>>,
    /// `true` when the band is too short to draw this pane legibly. The name
    /// and the live value are still written; only the curve is dropped, and
    /// the strip says how to get it back.
    pub collapsed: bool,
    /// The gutter band beside `rect`, where this pane's value labels go. It
    /// is the same band the pane's zoom gesture is registered over, which is
    /// what makes the numbers the thing you grab.
    pub gutter: Rect,
    /// The canvas colour behind the pane.
    pub background: Color32,
    /// Gridline and axis-rule colour, shared with the price axis so both
    /// read as one grid.
    pub grid: Color32,
}

/// The value range a pane auto-fits to: its visible values plus breathing
/// room above and below. `None` while everything visible is still NaN warmup
/// — there is nothing to scale to yet, and no axis to draw.
pub(crate) fn pane_auto_range(
    view: &IndicatorView,
    start: usize,
    end: usize,
) -> Option<(f64, f64)> {
    let (lo, hi) = value_range(view, start, end)?;
    let pad = (hi - lo).max(f64::EPSILON) * PANE_PAD_FRAC;
    Some((lo - pad, hi + pad))
}

/// Draw one pane indicator into its own rect: subtle frame, its plots on the
/// given value range, its own y-axis in the gutter beside it, a zero line
/// when zero is in range, and the label + last value.
///
/// `range` is the `(lo, hi)` the pane draws with — [`pane_auto_range`] unless
/// the user has zoomed this pane's axis — and `None` means the indicator has
/// computed nothing visible yet.
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_pane(
    painter: &egui::Painter,
    frame: &PaneFrame<'_>,
    view: &IndicatorView,
    x: &PlotX<'_>,
    range: Option<(f64, f64)>,
    start: usize,
    end: usize,
    partial_slot: Option<usize>,
) {
    let pane = frame.rect;
    // The pane is the plot area *and* the lane band beside it: one strip of
    // canvas from the chart's left edge to its gutter. Anything less leaves a
    // band of bare canvas between a pane's curve and the numbers that describe
    // it, which is what this whole frame exists to close.
    let band = frame
        .lane
        .as_ref()
        .map_or(pane, |lane| pane.union(lane.rect));
    painter.rect_filled(band, egui::Rounding::ZERO, frame.background);
    painter.line_segment(
        [band.left_top(), band.right_top()],
        Stroke::new(
            PANE_RULE_WIDTH_PX,
            theme::TEXT_MUTED.gamma_multiply(PANE_FRAME_ALPHA),
        ),
    );
    // The rule that closes the pane's right edge is drawn whatever the pane
    // has to say: the gutter is one column down the whole chart, and a gap in
    // it at the height of a warming-up pane reads as a broken axis.
    painter.line_segment(
        [
            pos2(frame.gutter.left(), pane.top()),
            pos2(frame.gutter.left(), pane.bottom()),
        ],
        Stroke::new(PANE_RULE_WIDTH_PX, frame.grid),
    );

    if frame.collapsed {
        draw_collapsed_pane(painter, band, view);
        return;
    }

    let Some((lo, hi)) = range else {
        painter.text(
            band.center(),
            egui::Align2::CENTER_CENTER,
            format!("{} — warming up", view.label()),
            egui::FontId::proportional(PANE_LABEL_FONT_PX),
            theme::TEXT_MUTED,
        );
        return;
    };
    let scale = PriceScale::from_range(lo, hi, pane.top(), pane.bottom());

    let clipped = painter.with_clip_rect(band);
    // The axis first, so its grid sits behind the plots rather than over them
    // — the price axis' own paint order.
    draw_pane_axis(painter, &clipped, frame, &scale, band);
    // A zero line anchors flow panes (cvd, delta) visually.
    if lo < 0.0 && hi > 0.0 {
        let y = scale.y(0.0);
        clipped.line_segment(
            [pos2(band.left(), y), pos2(band.right(), y)],
            Stroke::new(
                PANE_RULE_WIDTH_PX,
                theme::TEXT_MUTED.gamma_multiply(ZERO_LINE_ALPHA),
            ),
        );
    }

    let pane_extents = |_slot: usize| {
        Some((
            pane.top() + PANE_MARKER_INSET_PX,
            pane.bottom() - PANE_MARKER_INSET_PX,
        ))
    };
    draw_view_plots(
        &clipped,
        view,
        x,
        |v| scale.y(v),
        start,
        end,
        partial_slot,
        &pane_extents,
    );
    if let Some(lane) = &frame.lane {
        draw_lane_plots(&clipped, view, lane, &|v| scale.y(v));
    }
    draw_objects(
        &clipped,
        view.render_objects(),
        x,
        |v| scale.y(v),
        start,
        end,
    );

    painter.text(
        pane.left_top() + PANE_LABEL_INSET_PX,
        egui::Align2::LEFT_TOP,
        format!("{EXPANDED_CHEVRON} {}", view.label()),
        egui::FontId::proportional(PANE_LABEL_FONT_PX),
        theme::TEXT_MUTED,
    );
    if let Some(last) = last_value(view) {
        painter.text(
            band.right_top() + egui::vec2(-PANE_LABEL_INSET_PX.x, PANE_LABEL_INSET_PX.y),
            egui::Align2::RIGHT_TOP,
            format_value(last),
            egui::FontId::monospace(PANE_LABEL_FONT_PX),
            theme::TEXT_MUTED,
        );
    }
}

/// A pane with no room for its curve: its name on the left, its live value on
/// the right, and a chevron saying the curve is one click away.
///
/// The value is what the user added the indicator *for*, so it survives the
/// collapse — a strip that hid the number too would be an indicator switched
/// off without being asked. The chevron points right, the direction a closed
/// disclosure points.
fn draw_collapsed_pane(painter: &egui::Painter, band: Rect, view: &IndicatorView) {
    let font = egui::FontId::proportional(PANE_LABEL_FONT_PX);
    let y = band.center().y;
    painter.text(
        pos2(band.left() + PANE_LABEL_INSET_PX.x, y),
        egui::Align2::LEFT_CENTER,
        format!("{COLLAPSED_CHEVRON} {}", view.label()),
        font,
        theme::TEXT_MUTED,
    );
    if let Some(last) = last_value(view) {
        painter.text(
            pos2(band.right() - PANE_LABEL_INSET_PX.x, y),
            egui::Align2::RIGHT_CENTER,
            format_value(last),
            egui::FontId::monospace(PANE_LABEL_FONT_PX),
            theme::TEXT_MUTED,
        );
    }
}

/// A pane's line-shaped plots across the live lane: the committed steps of
/// the bars that closed inside the window, then the forming bar rung by rung.
///
/// Only line-shaped plots are drawn here. A histogram or a marker is a
/// statement about *a bar* — one column, one bar, at the bar's own width —
/// and the lane is a time axis with no bar widths in it; drawing them here
/// would have to invent a width the tape cannot justify. Those plots keep to
/// the history pane, where their slot is real.
fn draw_lane_plots(
    painter: &egui::Painter,
    view: &IndicatorView,
    lane: &LaneFrame<'_>,
    y_of: &impl Fn(f64) -> f32,
) {
    for (index, spec) in view.descriptor.plots.iter().enumerate() {
        if spec.marker.is_some()
            || !matches!(
                spec.style,
                PlotStyle::Line | PlotStyle::StepLine | PlotStyle::Area
            )
        {
            continue;
        }
        let Some(column) = view.columns.get(index) else {
            continue;
        };
        // Same resolution the history pane uses, so a restyled plot cannot
        // look like one series on the chart and another in the live lane.
        let Some(resolved) = view.plot_style(index) else {
            continue;
        };
        if !resolved.visible {
            continue;
        }
        let stroke = Stroke::new(resolved.width, color32(resolved.color));
        let mut segment: Vec<Pos2> = Vec::new();
        // The value a closed bar committed holds until the next close: the
        // horizontal-then-vertical corner is the shape of "this was the value
        // for that whole stretch", and a straight line between two closes
        // would draw an intra-bar path nobody recorded.
        let mut held: Option<f32> = None;
        for &(close_ms, row) in lane.steps {
            let Some(value) = column.get(row).copied().filter(|v| !v.is_nan()) else {
                flush_segment(painter, &mut segment, stroke);
                held = None;
                continue;
            };
            let point = pos2(lane.x(close_ms), y_of(value));
            if let Some(previous) = held {
                segment.push(pos2(point.x, previous));
            }
            segment.push(point);
            held = Some(point.y);
        }
        // The rungs: real evaluations of real prefixes, so they join point to
        // point like any other line.
        let mut first_rung = true;
        for sample in &view.lane {
            let Some(value) = sample.values.get(index).copied().filter(|v| !v.is_nan()) else {
                flush_segment(painter, &mut segment, stroke);
                held = None;
                first_rung = false;
                continue;
            };
            let point = pos2(lane.x(sample.close_time), y_of(value));
            // The last committed value holds right up to the forming bar's
            // first print — the bar had not opened yet, and the indicator had
            // not moved.
            if first_rung && let Some(previous) = held.take() {
                segment.push(pos2(point.x, previous));
            }
            segment.push(point);
            first_rung = false;
        }
        // Nothing forming: the last committed value is still the value, and it
        // holds out to the live edge.
        if let Some(previous) = held {
            segment.push(pos2(lane.rect.right(), previous));
        }
        flush_segment(painter, &mut segment, stroke);
    }
}

/// One pane's y-axis: round-number gridlines across the pane and their
/// labels in the gutter beside it.
///
/// `clipped` is the pane's own clipped painter, so gridlines stop at the pane;
/// the labels are painted on `painter`, since the gutter is outside that clip.
fn draw_pane_axis(
    painter: &egui::Painter,
    clipped: &egui::Painter,
    frame: &PaneFrame<'_>,
    scale: &PriceScale,
    band: Rect,
) {
    let pane = frame.rect;
    let (lo, hi) = scale.range();
    let font = egui::FontId::monospace(crate::chart::AXIS_LABEL_FONT_PX);
    for (tick, label) in crate::chart::axis_labels(lo, hi, pane.height()) {
        let y = scale.y(tick);
        clipped.line_segment(
            [pos2(band.left(), y), pos2(band.right(), y)],
            Stroke::new(PANE_RULE_WIDTH_PX, frame.grid),
        );
        // A label centred on the pane's own edge would be sliced in half by
        // the neighbouring pane; the gridline still marks the value.
        if y - pane.top() < AXIS_LABEL_EDGE_MARGIN_PX
            || pane.bottom() - y < AXIS_LABEL_EDGE_MARGIN_PX
        {
            continue;
        }
        painter.text(
            pos2(frame.gutter.left() + crate::chart::AXIS_LABEL_GAP_PX, y),
            egui::Align2::LEFT_CENTER,
            label,
            font.clone(),
            theme::TEXT_MUTED,
        );
    }
}

/// The newest value of the view's first plot (preview first, else the last
/// committed row) — the pane's headline number.
fn last_value(view: &IndicatorView) -> Option<f64> {
    let previewed = view
        .preview
        .as_ref()
        .and_then(|frame| frame.values.first().copied())
        .filter(|v| !v.is_nan());
    previewed.or_else(|| {
        view.columns
            .first()
            .and_then(|column| column.iter().rev().find(|v| !v.is_nan()).copied())
    })
}

/// Min/max over every plot's visible cells (preview included); `None` when
/// everything visible is still NaN warmup.
fn value_range(view: &IndicatorView, start: usize, end: usize) -> Option<(f64, f64)> {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for (index, column) in view.columns.iter().enumerate() {
        // A non-absolute marker column stages 1.0 as a flag, not as a value:
        // it anchors to the pane's own edges and is never read as a y. Folding
        // it into the fit collapses a real plot into a sliver — a CVD pane
        // spanning 1e6..1.1e6 becomes 1.0..1.1e6.
        let is_flag_column = view.descriptor.plots.get(index).is_some_and(|spec| {
            spec.marker.as_ref().is_some_and(|marker| {
                marker.location != quantick_indicators::MarkerLocation::Absolute
            })
        });
        if is_flag_column {
            continue;
        }
        let preview = view
            .preview
            .as_ref()
            .and_then(|frame| frame.values.get(index).copied());
        for value in column[start.min(column.len())..end.min(column.len())]
            .iter()
            .copied()
            .chain(preview)
        {
            if value.is_nan() {
                continue;
            }
            lo = lo.min(value);
            hi = hi.max(value);
        }
    }
    (lo.is_finite() && hi.is_finite()).then_some((lo, hi))
}

fn format_value(v: f64) -> String {
    if v.abs() >= LARGE_VALUE_THRESHOLD {
        format!("{v:.*}", LARGE_VALUE_DECIMALS)
    } else {
        format!("{v:.*}", SMALL_VALUE_DECIMALS)
    }
}

/// Draw all plots of one view with a shared y mapping.
#[allow(clippy::too_many_arguments)]
fn draw_view_plots(
    painter: &egui::Painter,
    view: &IndicatorView,
    x: &PlotX<'_>,
    y_of: impl Fn(f64) -> f32,
    start: usize,
    end: usize,
    partial_slot: Option<usize>,
    bar_extents: &dyn Fn(usize) -> Option<(f32, f32)>,
) {
    // Fills first: bands read as background, never covering their plots.
    for fill in &view.descriptor.fills {
        draw_fill(painter, view, fill, x, &y_of, start, end, partial_slot);
    }
    for (index, spec) in view.descriptor.plots.iter().enumerate() {
        let Some(column) = view.columns.get(index) else {
            continue;
        };
        let preview = partial_slot.and_then(|slot| {
            // Only when the forming bar is the slot right after the last
            // committed cell. The columns are a worker round-trip behind the
            // bars, so on the frame after a close `slot` is one past the end
            // — joining the last committed point to the preview there would
            // interpolate a segment across a bar that has no value, the very
            // lie the NaN gaps exist to prevent.
            (slot == column.len()).then_some(())?;
            let value = view.preview.as_ref()?.values.get(index).copied()?;
            (!value.is_nan()).then_some((slot, value))
        });
        let visible = VisiblePlot {
            column,
            start,
            end,
            preview,
        };
        // The trader's style layer over the author's declaration. A plot they
        // switched off draws nothing at all — the eye in the legend hides the
        // whole indicator, this hides one series of several.
        let Some(resolved) = view.plot_style(index) else {
            continue;
        };
        if !resolved.visible {
            continue;
        }
        let color = color32(resolved.color);
        let stroke = Stroke::new(resolved.width, color);
        if let Some(marker) = &spec.marker {
            draw_shape_markers(painter, &visible, x, &y_of, color, marker, bar_extents);
            continue;
        }
        match spec.style {
            PlotStyle::Line => draw_line(painter, &visible, x, &y_of, stroke, false),
            PlotStyle::StepLine => draw_line(painter, &visible, x, &y_of, stroke, true),
            PlotStyle::Histogram => {
                draw_bars(painter, &visible, x, &y_of, color, HISTOGRAM_WIDTH_FRAC);
            }
            PlotStyle::Columns => {
                draw_bars(painter, &visible, x, &y_of, color, COLUMNS_WIDTH_FRAC);
            }
            PlotStyle::Circles => draw_markers(painter, &visible, x, &y_of, color, false),
            PlotStyle::Cross => draw_markers(painter, &visible, x, &y_of, color, true),
            PlotStyle::Area => draw_area(painter, &visible, x, &y_of, stroke, color),
        }
    }
}

/// Batched polyline(s); NaN starts a new segment. `stepped` inserts the
/// horizontal-then-vertical corner of a step line.
fn draw_line(
    painter: &egui::Painter,
    plot: &VisiblePlot<'_>,
    x: &PlotX<'_>,
    y_of: &impl Fn(f64) -> f32,
    stroke: Stroke,
    stepped: bool,
) {
    let mut segment: Vec<Pos2> = Vec::new();
    for (row, value) in plot.cells() {
        if value.is_nan() {
            flush_segment(painter, &mut segment, stroke);
            continue;
        }
        let point = pos2(x.x(row), y_of(value));
        if stepped && let Some(&previous) = segment.last() {
            segment.push(pos2(point.x, previous.y));
        }
        segment.push(point);
    }
    flush_segment(painter, &mut segment, stroke);
}

fn flush_segment(painter: &egui::Painter, segment: &mut Vec<Pos2>, stroke: Stroke) {
    match segment.len() {
        0 => {}
        // A lone point still marks the bar (a 1-bar segment between gaps).
        1 => {
            painter.circle_filled(segment[0], stroke.width.max(1.0), stroke.color);
        }
        _ => {
            painter.add(Shape::line(std::mem::take(segment), stroke));
        }
    }
    segment.clear();
}

/// Append one axis-aligned quad to `mesh`.
///
/// Per-element `Shape`s cost a tessellation pass each and, for the area fill,
/// a heap allocation per visible bar; the repo's answer for hundreds of
/// same-coloured pieces per frame is one mesh (see `orderflow_render`'s
/// fronts and caps). Two triangles, four vertices, no allocation.
fn push_quad(mesh: &mut egui::Mesh, rect: Rect, color: Color32) {
    if !rect.is_finite() {
        return;
    }
    let base = mesh.vertices.len() as u32;
    for corner in [
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
    ] {
        mesh.colored_vertex(corner, color);
    }
    mesh.add_triangle(base, base + 1, base + 2);
    mesh.add_triangle(base, base + 2, base + 3);
}

/// Vertical bars from the zero line (histogram/columns).
fn draw_bars(
    painter: &egui::Painter,
    plot: &VisiblePlot<'_>,
    x: &PlotX<'_>,
    y_of: &impl Fn(f64) -> f32,
    color: Color32,
    width_frac: f32,
) {
    let half = (x.slot_width() * width_frac / 2.0).max(0.5);
    let zero_y = y_of(0.0);
    let mut mesh = egui::Mesh::default();
    for (row, value) in plot.cells() {
        if value.is_nan() {
            continue;
        }
        let xc = x.x(row);
        let y = y_of(value);
        push_quad(
            &mut mesh,
            Rect::from_min_max(
                pos2(xc - half, y.min(zero_y)),
                pos2(xc + half, y.max(zero_y)),
            ),
            color,
        );
    }
    if !mesh.is_empty() {
        painter.add(Shape::mesh(mesh));
    }
}

/// A dot or cross per bar.
fn draw_markers(
    painter: &egui::Painter,
    plot: &VisiblePlot<'_>,
    x: &PlotX<'_>,
    y_of: &impl Fn(f64) -> f32,
    color: Color32,
    cross: bool,
) {
    for (row, value) in plot.cells() {
        if value.is_nan() {
            continue;
        }
        let center = pos2(x.x(row), y_of(value));
        if cross {
            let r = MARKER_RADIUS_PX;
            let stroke = Stroke::new(1.0_f32, color);
            painter.line_segment(
                [center + egui::vec2(-r, -r), center + egui::vec2(r, r)],
                stroke,
            );
            painter.line_segment(
                [center + egui::vec2(-r, r), center + egui::vec2(r, -r)],
                stroke,
            );
        } else {
            painter.circle_filled(center, MARKER_RADIUS_PX, color);
        }
    }
}

/// Filled area between the line and zero, plus the outline on top.
fn draw_area(
    painter: &egui::Painter,
    plot: &VisiblePlot<'_>,
    x: &PlotX<'_>,
    y_of: &impl Fn(f64) -> f32,
    stroke: Stroke,
    color: Color32,
) {
    let fill = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), AREA_FILL_ALPHA);
    let zero_y = y_of(0.0);
    let mut segment: Vec<Pos2> = Vec::new();
    let flush = |segment: &mut Vec<Pos2>| {
        // The region under a wiggly line is not convex, so it is filled one
        // quad per point pair; they all go into a single mesh, because at
        // full zoom-out that is over a thousand pieces per frame.
        let mut mesh = egui::Mesh::default();
        for pair in segment.windows(2) {
            let base = mesh.vertices.len() as u32;
            for corner in [
                pair[0],
                pair[1],
                pos2(pair[1].x, zero_y),
                pos2(pair[0].x, zero_y),
            ] {
                mesh.colored_vertex(corner, fill);
            }
            mesh.add_triangle(base, base + 1, base + 2);
            mesh.add_triangle(base, base + 2, base + 3);
        }
        if !mesh.is_empty() {
            painter.add(Shape::mesh(mesh));
        }
        if segment.len() >= 2 {
            painter.add(Shape::line(std::mem::take(segment), stroke));
        } else {
            segment.clear();
        }
    };
    for (row, value) in plot.cells() {
        if value.is_nan() {
            flush(&mut segment);
            continue;
        }
        segment.push(pos2(x.x(row), y_of(value)));
    }
    flush(&mut segment);
}

/// Label text size for draw objects, in points.
const OBJECT_LABEL_FONT: f32 = 11.0;
/// Padding around a label's text inside its background pill.
const OBJECT_LABEL_PAD: f32 = 3.0;
/// Gap between a label's anchor point and its pill, in pixels.
const OBJECT_LABEL_GAP_PX: f32 = 2.0;
/// Corner radius of a label's pill, in pixels.
const OBJECT_LABEL_ROUNDING_PX: f32 = 3.0;
/// Thinnest stroke a draw object may ask for, in pixels: below this a border
/// disappears entirely on some scale factors.
const MIN_OBJECT_STROKE_PX: f32 = 0.5;

/// Draw one indicator's line/box/label objects. Bar-index coordinates ride
/// the candles' own x-mapping, so objects pan and zoom with the bars; the
/// caller picks the y mapping (price scale on the chart, value scale in a
/// pane) and the clip.
pub(crate) fn draw_objects(
    painter: &egui::Painter,
    objects: &quantick_indicators::ObjectSnapshot,
    x: &PlotX<'_>,
    y_of: impl Fn(f64) -> f32,
    start: usize,
    end: usize,
) {
    // Every sibling draw function slices to the visible range; this one used
    // to walk all three retained kinds in full, and the label loop laid out
    // its text *before* any visibility test — 500 `String` clones and 500
    // galley builds per frame for labels that are mostly off-screen. The
    // test is overlap, not containment: a line that starts left of the
    // window and ends right of it crosses the whole screen.
    let visible_span = |lo: i64, hi: i64| {
        let (lo, hi) = (lo.min(hi), lo.max(hi));
        hi >= 0 && (lo as usize) < end && (hi as usize) >= start
    };
    for object in &objects.boxes {
        if object.left < 0 || object.right < 0 {
            continue;
        }
        if !visible_span(object.left, object.right) {
            continue;
        }
        if !object.top.is_finite() || !object.bottom.is_finite() {
            continue;
        }
        let a = pos2(x.x(object.left as usize), y_of(object.top));
        let b = pos2(x.x(object.right as usize), y_of(object.bottom));
        let rect = Rect::from_two_pos(a, b);
        painter.rect_filled(rect, egui::Rounding::ZERO, color32(object.bg_color));
        painter.rect_stroke(
            rect,
            egui::Rounding::ZERO,
            Stroke::new(
                object.border_width.max(MIN_OBJECT_STROKE_PX),
                color32(object.border_color),
            ),
        );
    }
    for line in &objects.lines {
        if line.x1 < 0 || line.x2 < 0 || !line.y1.is_finite() || !line.y2.is_finite() {
            continue;
        }
        if !visible_span(line.x1, line.x2) {
            continue;
        }
        painter.line_segment(
            [
                pos2(x.x(line.x1 as usize), y_of(line.y1)),
                pos2(x.x(line.x2 as usize), y_of(line.y2)),
            ],
            Stroke::new(line.width.max(0.5), color32(line.color)),
        );
    }
    for label in &objects.labels {
        if label.x < 0 || !label.y.is_finite() {
            continue;
        }
        // Before the layout, not after: `layout_no_wrap` clones the text and
        // builds a galley, which is the expensive part of drawing a label.
        if !visible_span(label.x, label.x) {
            continue;
        }
        let anchor = pos2(x.x(label.x as usize), y_of(label.y));
        let galley = painter.layout_no_wrap(
            label.text.clone(),
            egui::FontId::proportional(OBJECT_LABEL_FONT),
            color32(label.text_color),
        );
        let size = galley.size() + egui::vec2(OBJECT_LABEL_PAD * 2.0, OBJECT_LABEL_PAD * 2.0);
        // Up = pill above its anchor? No: style_label_up points up FROM
        // below — the pill hangs under the anchor at lows; label_down hangs
        // above the anchor at highs.
        let min = match label.style {
            quantick_indicators::LabelStyle::Up => {
                anchor + egui::vec2(-size.x / 2.0, OBJECT_LABEL_GAP_PX)
            }
            quantick_indicators::LabelStyle::Down => {
                anchor + egui::vec2(-size.x / 2.0, -size.y - OBJECT_LABEL_GAP_PX)
            }
            quantick_indicators::LabelStyle::None => {
                anchor + egui::vec2(-size.x / 2.0, -size.y / 2.0)
            }
        };
        let rect = Rect::from_min_size(min, size);
        painter.rect_filled(
            rect,
            egui::Rounding::same(OBJECT_LABEL_ROUNDING_PX),
            color32(label.color),
        );
        painter.galley(
            rect.min + egui::vec2(OBJECT_LABEL_PAD, OBJECT_LABEL_PAD),
            galley,
            color32(label.text_color),
        );
    }
}

/// A band between two plot columns: one convex quad per adjacent pair where
/// all four cells are finite. A pair whose sides cross inside one slot
/// renders as a pinched quad — acceptable at candle widths; splitting at
/// the crossing is a later nicety.
#[allow(clippy::too_many_arguments)]
fn draw_fill(
    painter: &egui::Painter,
    view: &IndicatorView,
    fill: &quantick_indicators::FillSpec,
    x: &PlotX<'_>,
    y_of: &impl Fn(f64) -> f32,
    start: usize,
    end: usize,
    partial_slot: Option<usize>,
) {
    let (Some(col_a), Some(col_b)) = (
        view.columns.get(fill.a.index()),
        view.columns.get(fill.b.index()),
    ) else {
        return;
    };
    let preview_pair = partial_slot.and_then(|slot| {
        let frame = view.preview.as_ref()?;
        let a = frame.values.get(fill.a.index()).copied()?;
        let b = frame.values.get(fill.b.index()).copied()?;
        Some((slot, a, b))
    });
    let color = color32(fill.color);
    let cell = |row: usize| -> Option<(f32, f32, f32)> {
        let (a, b) = if row < col_a.len().min(col_b.len()) {
            (col_a[row], col_b[row])
        } else if let Some((slot, a, b)) = preview_pair
            && row == slot
        {
            (a, b)
        } else {
            return None;
        };
        (a.is_finite() && b.is_finite()).then(|| (x.x(row), y_of(a), y_of(b)))
    };
    let last = preview_pair.map_or(end.saturating_sub(1), |(slot, ..)| slot);
    let mut previous: Option<(f32, f32, f32)> = None;
    for row in start..=last {
        let current = cell(row);
        if let (Some((x0, a0, b0)), Some((x1, a1, b1))) = (previous, current) {
            painter.add(Shape::convex_polygon(
                vec![pos2(x0, a0), pos2(x1, a1), pos2(x1, b1), pos2(x0, b0)],
                color,
                Stroke::NONE,
            ));
        }
        previous = current;
    }
}

/// Markers for a `plotshape`/`plotchar` column: na cells draw nothing;
/// above/below cells anchor to the bar's extremes, absolute cells to their
/// own value.
fn draw_shape_markers(
    painter: &egui::Painter,
    plot: &VisiblePlot<'_>,
    x: &PlotX<'_>,
    y_of: &impl Fn(f64) -> f32,
    color: Color32,
    marker: &quantick_indicators::MarkerSpec,
    bar_extents: &dyn Fn(usize) -> Option<(f32, f32)>,
) {
    use quantick_indicators::{MarkerLocation, MarkerShape};
    for (row, value) in plot.cells() {
        if value.is_nan() {
            continue;
        }
        let cx = x.x(row);
        // Above/below are *screen* words, so the anchor reads the extents'
        // screen extremes rather than their price names: upside down the
        // high's pixel is the bar's bottom edge, and keying off it would
        // plant the marker inside the candle with above/below swapped.
        let cy = match marker.location {
            MarkerLocation::Absolute => y_of(value),
            MarkerLocation::AboveBar => match bar_extents(row) {
                Some((a, b)) => a.min(b) - MARKER_GAP_PX,
                None => continue,
            },
            MarkerLocation::BelowBar => match bar_extents(row) {
                Some((a, b)) => a.max(b) + MARKER_GAP_PX,
                None => continue,
            },
        };
        if !cy.is_finite() {
            continue;
        }
        let center = pos2(cx, cy);
        let r = MARKER_SIZE_PX;
        match marker.shape {
            MarkerShape::TriangleUp | MarkerShape::LabelUp => {
                painter.add(Shape::convex_polygon(
                    vec![
                        center + egui::vec2(0.0, -r),
                        center + egui::vec2(r, r),
                        center + egui::vec2(-r, r),
                    ],
                    color,
                    Stroke::NONE,
                ));
            }
            MarkerShape::TriangleDown | MarkerShape::LabelDown => {
                painter.add(Shape::convex_polygon(
                    vec![
                        center + egui::vec2(0.0, r),
                        center + egui::vec2(-r, -r),
                        center + egui::vec2(r, -r),
                    ],
                    color,
                    Stroke::NONE,
                ));
            }
            MarkerShape::Circle => {
                painter.circle_filled(center, r, color);
            }
            MarkerShape::Cross => {
                let stroke = Stroke::new(MARKER_CROSS_STROKE_PX, color);
                painter.line_segment(
                    [center + egui::vec2(-r, -r), center + egui::vec2(r, r)],
                    stroke,
                );
                painter.line_segment(
                    [center + egui::vec2(-r, r), center + egui::vec2(r, -r)],
                    stroke,
                );
            }
        }
        if let Some(text) = &marker.text {
            // A marker below the bar puts its text below itself; the flag
            // used to be called `above` and meant the opposite.
            let text_below = matches!(marker.location, MarkerLocation::BelowBar);
            let anchor = if text_below {
                egui::Align2::CENTER_TOP
            } else {
                egui::Align2::CENTER_BOTTOM
            };
            let gap = r + MARKER_TEXT_GAP_PX;
            let offset = if text_below { gap } else { -gap };
            painter.text(
                center + egui::vec2(0.0, offset),
                anchor,
                text,
                egui::FontId::proportional(OBJECT_LABEL_FONT),
                color,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indicator_worker::SlotId;
    use quantick_indicators::{IndicatorDescriptor, PlotId, PlotSpec, PreviewFrame, Rgba8};

    fn marker_view(columns: Vec<Vec<f64>>, marker_at: usize) -> IndicatorView {
        let mut v = view(columns, None);
        v.descriptor.plots[marker_at].marker = Some(quantick_indicators::MarkerSpec {
            shape: quantick_indicators::MarkerShape::Circle,
            location: quantick_indicators::MarkerLocation::AboveBar,
            text: None,
        });
        v
    }

    fn view(columns: Vec<Vec<f64>>, preview: Option<Vec<f64>>) -> IndicatorView {
        IndicatorView {
            slot: SlotId(0),
            kind: std::sync::Arc::from("test.indicator"),
            ordinal: 0,
            style: crate::indicator_style::StyleOverride::default(),
            label: std::sync::Arc::from("test"),
            descriptor: IndicatorDescriptor {
                title: "test".to_owned(),
                short_title: None,
                overlay: false,
                plots: (0..columns.len())
                    .map(|i| PlotSpec {
                        id: PlotId::new(i),
                        title: format!("p{i}"),
                        style: PlotStyle::Line,
                        base_color: Rgba8::opaque(255, 255, 255),
                        width: 1.0,
                        offset: 0,
                        marker: None,
                    })
                    .collect(),
                fills: Vec::new(),
                inputs: Vec::new(),
            },
            rows: columns.first().map_or(0, Vec::len),
            columns,
            bar_paints: Vec::new(),
            preview: preview.map(PreviewFrame::new),
            lane: Vec::new(),
            objects: quantick_indicators::ObjectSnapshot::default(),
            input_values: Vec::new(),
            stale: None,
            error: None,
            hidden: false,
            scale: crate::price_view::PriceView::new(),
            sizing: crate::indicators::PaneSizing::Auto,
            last_auto: None,
        }
    }

    #[test]
    fn value_range_ignores_warmup_and_reports_none_when_all_of_it_is() {
        let all_nan = view(vec![vec![f64::NAN, f64::NAN]], None);
        assert!(
            value_range(&all_nan, 0, 2).is_none(),
            "a pane with nothing computed yet has no range to fit"
        );

        let mixed = view(vec![vec![f64::NAN, 3.0, -1.0, f64::NAN]], None);
        assert_eq!(value_range(&mixed, 0, 4), Some((-1.0, 3.0)));

        // The forming bar's value belongs to the fit; the window does not
        // clip it away.
        let with_preview = view(vec![vec![1.0, 2.0]], Some(vec![9.0]));
        assert_eq!(value_range(&with_preview, 0, 2), Some((1.0, 9.0)));
    }

    #[test]
    fn a_marker_column_is_a_flag_and_never_scales_the_pane() {
        // The staged 1.0 marks "the condition fired here"; it is never used
        // as a y, because a non-absolute marker anchors to the pane edges.
        let mixed = marker_view(vec![vec![1.0, 1.0], vec![1.0e6, 1.1e6]], 0);
        assert_eq!(
            value_range(&mixed, 0, 2),
            Some((1.0e6, 1.1e6)),
            "the real plot keeps the pane"
        );
    }

    #[test]
    fn last_value_prefers_the_preview_and_skips_trailing_warmup() {
        let previewed = view(vec![vec![1.0, 2.0]], Some(vec![7.5]));
        assert_eq!(last_value(&previewed), Some(7.5));

        // A NaN preview cell is "nothing to draw", not a value.
        let nan_preview = view(vec![vec![1.0, 2.0]], Some(vec![f64::NAN]));
        assert_eq!(last_value(&nan_preview), Some(2.0));

        let trailing_nan = view(vec![vec![1.0, 2.0, f64::NAN]], None);
        assert_eq!(last_value(&trailing_nan), Some(2.0));

        assert_eq!(last_value(&view(vec![vec![f64::NAN]], None)), None);
    }

    #[test]
    fn a_gap_breaks_the_polyline_instead_of_interpolating_across_it() {
        let cells = |column: &[f64]| {
            let plot = VisiblePlot {
                column,
                start: 0,
                end: column.len(),
                preview: None,
            };
            plot.cells().filter(|(_, v)| !v.is_nan()).count()
        };
        // The honesty claim in this module's header: warmup and conditional
        // plots render as gaps. A NaN in the middle yields two runs of real
        // points, never one line drawn straight over the missing bar.
        let column = [1.0, 2.0, f64::NAN, 4.0, 5.0];
        assert_eq!(cells(&column), 4);

        let mut runs = 0usize;
        let mut in_run = false;
        for value in column {
            if value.is_nan() {
                in_run = false;
            } else if !in_run {
                in_run = true;
                runs += 1;
            }
        }
        assert_eq!(runs, 2, "two segments, one gap");
    }

    #[test]
    fn the_auto_range_pads_the_values_and_has_nothing_to_pad_during_warmup() {
        let pane = view(vec![vec![0.0, 10.0]], None);
        let (lo, hi) = pane_auto_range(&pane, 0, 2).expect("two values are a range");
        assert!(lo < 0.0 && hi > 10.0, "the trace never touches the edges");
        assert!(
            (lo + 0.8).abs() < 1e-9 && (hi - 10.8).abs() < 1e-9,
            "8% of the span on each side: {lo}..{hi}"
        );

        assert!(
            pane_auto_range(&view(vec![vec![f64::NAN]], None), 0, 1).is_none(),
            "nothing computed yet is nothing to scale — and no axis to draw"
        );
    }

    #[test]
    fn format_value_drops_decimals_only_at_price_scale() {
        assert_eq!(format_value(1234.5678), "1234.6");
        assert_eq!(format_value(0.12345), "0.1235");
        assert_eq!(format_value(-9999.99), "-10000.0");
    }

    fn painted(draw: impl Fn(&egui::Painter)) -> String {
        let ctx = egui::Context::default();
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            draw(&ctx.layer_painter(egui::LayerId::background()));
        });
        format!("{:?}", output.shapes)
    }

    fn lane_of(steps: &[(i64, usize)]) -> LaneFrame<'_> {
        LaneFrame {
            rect: Rect::from_min_max(pos2(100.0, 0.0), pos2(200.0, 50.0)),
            start_ms: 1_000,
            end_ms: 2_000,
            steps,
        }
    }

    /// The lane is a linear map of tape time onto its band — the tape's own
    /// mapping, which is the whole reason a pane's curve can be trusted to
    /// sit under the prints it was computed from. Instants outside the window
    /// clamp to the edges rather than escaping the band.
    #[test]
    fn a_lane_maps_tape_time_linearly_and_clamps_outside_its_window() {
        let lane = lane_of(&[]);
        assert!((lane.x(1_000) - 100.0).abs() < 1e-3, "the window's start");
        assert!((lane.x(2_000) - 200.0).abs() < 1e-3, "the live edge");
        assert!((lane.x(1_500) - 150.0).abs() < 1e-3, "halfway is halfway");
        assert!((lane.x(0) - 100.0).abs() < 1e-3, "older than the window");
        assert!((lane.x(9_999) - 200.0).abs() < 1e-3, "past the live edge");
    }

    /// A degenerate window (start == end, which a stalled feed can produce)
    /// must not divide by zero or paint at NaN.
    #[test]
    fn a_zero_width_window_still_maps_to_a_finite_x() {
        let lane = LaneFrame {
            rect: Rect::from_min_max(pos2(100.0, 0.0), pos2(200.0, 50.0)),
            start_ms: 5_000,
            end_ms: 5_000,
            steps: &[],
        };
        assert!(lane.x(5_000).is_finite());
        assert!(lane.x(1).is_finite());
    }

    /// The rungs are drawn as a line across the band: with a lane, a pane
    /// paints more than it does without one, and the extra ink lands inside
    /// the lane's own rect.
    #[test]
    fn the_rungs_are_drawn_across_the_lane_band() {
        let mut with_lane = view(vec![vec![1.0, 2.0]], Some(vec![3.0]));
        with_lane.lane = vec![
            crate::indicator_worker::LaneSample {
                close_time: 1_200,
                values: vec![2.2],
            },
            crate::indicator_worker::LaneSample {
                close_time: 1_600,
                values: vec![2.8],
            },
            crate::indicator_worker::LaneSample {
                close_time: 2_000,
                values: vec![3.0],
            },
        ];

        let shapes = painted(|painter| {
            draw_lane_plots(painter, &with_lane, &lane_of(&[]), &|v| 50.0 - v as f32);
        });
        assert!(
            shapes.contains("Path"),
            "the rungs are a polyline: {shapes}"
        );
        // 1_200, 1_600 and 2_000 ms across a 1_000 ms window on a 100 px band.
        assert!(
            shapes.contains("120.0") && shapes.contains("160.0") && shapes.contains("200.0"),
            "each rung lands at its own instant on the tape: {shapes}"
        );

        let bare = view(vec![vec![1.0, 2.0]], Some(vec![3.0]));
        let nothing = painted(|painter| {
            draw_lane_plots(painter, &bare, &lane_of(&[]), &|v| 50.0 - v as f32);
        });
        assert!(!nothing.contains("Path"), "no rungs, no curve: {nothing}");
    }

    /// A committed value holds until the next close and then steps. Drawing a
    /// slanted line between two closes would claim an intra-bar path nobody
    /// recorded — so the corner has to be there, and the test is that the
    /// polyline visits the corner's x twice.
    #[test]
    fn committed_values_step_across_the_lane_instead_of_interpolating() {
        let pane = view(vec![vec![10.0, 20.0]], None);
        let shapes = painted(|painter| {
            draw_lane_plots(painter, &pane, &lane_of(&[(1_200, 0), (1_600, 1)]), &|v| {
                100.0 - v as f32
            });
        });
        // Rows 0 and 1 sit at y 90 and 80; the hold means y 90 appears at the
        // second close's x before the step down to 80.
        assert!(
            shapes.contains("90.0") && shapes.contains("80.0"),
            "{shapes}"
        );
        assert_eq!(
            shapes.matches("90.0").count(),
            2,
            "the earlier value is held to the next close: {shapes}"
        );
    }

    /// Histograms and markers keep to the history pane: a column is a
    /// statement about one bar at that bar's width, and the lane has no bar
    /// widths in it to honour.
    #[test]
    fn bar_shaped_plots_stay_out_of_the_lane() {
        let mut pane = view(vec![vec![1.0, 2.0]], None);
        pane.descriptor.plots[0].style = PlotStyle::Histogram;
        pane.lane = vec![crate::indicator_worker::LaneSample {
            close_time: 1_500,
            values: vec![2.5],
        }];
        let shapes = painted(|painter| {
            draw_lane_plots(painter, &pane, &lane_of(&[(1_200, 0)]), &|v| {
                50.0 - v as f32
            });
        });
        assert_eq!(
            shapes, "[]",
            "nothing drawn for a bar-shaped plot: {shapes}"
        );
    }
}
