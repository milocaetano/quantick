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
) {
    for view in overlays {
        draw_view_plots(painter, view, x, |v| scale.y(v), start, end, partial_slot);
    }
}

/// Draw one pane indicator into its own rect: subtle frame, its plots on an
/// auto-fitted value scale, a zero line when zero is in range, and the
/// label + last value so the pane reads without a y-axis (v1).
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_pane(
    painter: &egui::Painter,
    pane: Rect,
    view: &IndicatorView,
    x: &PlotX<'_>,
    start: usize,
    end: usize,
    partial_slot: Option<usize>,
    background: Color32,
) {
    painter.rect_filled(pane, egui::Rounding::ZERO, background);
    painter.line_segment(
        [pane.left_top(), pane.right_top()],
        Stroke::new(
            PANE_RULE_WIDTH_PX,
            theme::TEXT_MUTED.gamma_multiply(PANE_FRAME_ALPHA),
        ),
    );

    let Some((lo, hi)) = value_range(view, start, end) else {
        painter.text(
            pane.center(),
            egui::Align2::CENTER_CENTER,
            format!("{} — warming up", view.label()),
            egui::FontId::proportional(PANE_LABEL_FONT_PX),
            theme::TEXT_MUTED,
        );
        return;
    };
    let span = (hi - lo).max(f64::EPSILON);
    let pad = span * PANE_PAD_FRAC;
    let scale = PriceScale::from_range(lo - pad, hi + pad, pane.top(), pane.bottom());

    let clipped = painter.with_clip_rect(pane);
    // A zero line anchors flow panes (cvd, delta) visually.
    if lo < 0.0 && hi > 0.0 {
        let y = scale.y(0.0);
        clipped.line_segment(
            [pos2(pane.left(), y), pos2(pane.right(), y)],
            Stroke::new(
                PANE_RULE_WIDTH_PX,
                theme::TEXT_MUTED.gamma_multiply(ZERO_LINE_ALPHA),
            ),
        );
    }

    draw_view_plots(&clipped, view, x, |v| scale.y(v), start, end, partial_slot);

    painter.text(
        pane.left_top() + PANE_LABEL_INSET_PX,
        egui::Align2::LEFT_TOP,
        view.label(),
        egui::FontId::proportional(PANE_LABEL_FONT_PX),
        theme::TEXT_MUTED,
    );
    if let Some(last) = last_value(view) {
        painter.text(
            pane.right_top() + egui::vec2(-PANE_LABEL_INSET_PX.x, PANE_LABEL_INSET_PX.y),
            egui::Align2::RIGHT_TOP,
            format_value(last),
            egui::FontId::monospace(PANE_LABEL_FONT_PX),
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
fn draw_view_plots(
    painter: &egui::Painter,
    view: &IndicatorView,
    x: &PlotX<'_>,
    y_of: impl Fn(f64) -> f32,
    start: usize,
    end: usize,
    partial_slot: Option<usize>,
) {
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
        let color = color32(spec.base_color);
        let stroke = Stroke::new(spec.width, color);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indicator_worker::SlotId;
    use quantick_indicators::{IndicatorDescriptor, PlotId, PlotSpec, PreviewFrame, Rgba8};

    fn view(columns: Vec<Vec<f64>>, preview: Option<Vec<f64>>) -> IndicatorView {
        IndicatorView {
            slot: SlotId(0),
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
                    })
                    .collect(),
                inputs: Vec::new(),
            },
            columns,
            preview: preview.map(|values| PreviewFrame { values }),
            error: None,
            hidden: false,
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
    fn format_value_drops_decimals_only_at_price_scale() {
        assert_eq!(format_value(1234.5678), "1234.6");
        assert_eq!(format_value(0.12345), "0.1235");
        assert_eq!(format_value(-9999.99), "-10000.0");
    }
}
