//! The equity curve, and the card that reads one point of it.
//!
//! Cumulative points against trade number, walked once into a polyline the
//! window paints under the tiles. The walk is separate from the paint so the
//! hover card can name the trade under the pointer without re-deriving the
//! running total.

use eframe::egui;
use rust_decimal::Decimal;

use super::{
    CURVE_FILL_ALPHA, CURVE_GRID_LINE_ALPHA, CURVE_GRID_RESERVE_PX, CURVE_GUTTER_PX,
    CURVE_MAX_H_PX, CURVE_MAX_POINTS, ReportView,
};
use crate::paper_calendar::fmt_offset_minute;
use crate::paper_chrome::{caption, fmt_decimal, fmt_points, fmt_signed_points, position_word};
use crate::theme;

/// The realized equity curve, `E_k` by trade index — the closing order
/// that defines the drawdown, so calling the axis "time" would misstate
/// what is plotted. One quiet line; a diverging fill against the zero
/// baseline answers "was I ever under water?" at a glance; the deepest
/// drawdown is annotated so the number in the grid below is locatable.
pub(super) fn draw_equity_curve(ui: &mut egui::Ui, view: &ReportView, floor: f32) {
    let n = view.rows.len();
    if n == 0 {
        return;
    }
    ui.label(caption("REALIZED EQUITY"));
    let height = (ui.available_height() - CURVE_GRID_RESERVE_PX).clamp(floor, CURVE_MAX_H_PX);
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::hover(),
    );
    if !ui.is_rect_visible(rect) {
        return;
    }
    // The walk E_0..E_n was cut with the view; the frame only plots it.
    let (low, high) = (view.equity.low, view.equity.high);
    let curve = CurveFrame {
        painter: ui.painter_at(rect),
        rect,
        plot: egui::Rect::from_min_max(
            egui::pos2(rect.left() + CURVE_GUTTER_PX, rect.top() + 4.0),
            egui::pos2(rect.right() - 4.0, rect.bottom() - 14.0),
        ),
        n,
        low,
        span: (high - low).max(f32::EPSILON),
        equity: &view.equity.plot,
    };
    curve.grid(high);
    let stride = n.div_ceil(CURVE_MAX_POINTS).max(1);
    curve.fill_and_line(stride);
    if let Some((peak, trough)) = view.report.max_drawdown_span {
        curve.drawdown(peak as usize, trough as usize, view);
    }
    curve.axis_words(stride);
    // Hover: snap to the nearest trade index and answer in the ledger's
    // vocabulary, so the two surfaces never disagree.
    if let Some(pointer) = response.hover_pos()
        && curve.plot.contains(pointer)
    {
        curve.hover(pointer, view);
    }
}

/// One frame of the curve: where it is painted and how a trade index and
/// an equity value map into it. Built once per frame; the passes below
/// paint in the order the eye reads them — grid, fill, baseline and line,
/// drawdown, axis words, hover.
struct CurveFrame<'a> {
    painter: egui::Painter,
    rect: egui::Rect,
    /// The plotting area inside `rect`, past the tick gutter.
    plot: egui::Rect,
    /// Trades plotted; the walk holds `n + 1` points.
    n: usize,
    low: f32,
    span: f32,
    equity: &'a [f32],
}

impl CurveFrame<'_> {
    fn x_at(&self, k: usize) -> f32 {
        self.plot.left() + self.plot.width() * (k as f32) / (self.n as f32)
    }

    fn y_at(&self, value: f32) -> f32 {
        self.plot.bottom() - (value - self.low) / self.span * self.plot.height()
    }

    /// Gridlines + y ticks at the floor, zero and the ceiling.
    fn grid(&self, high: f32) {
        let plot = self.plot;
        let mut ticks = vec![self.low, 0.0, high];
        ticks.dedup_by(|a, b| (*a - *b).abs() < f32::EPSILON);
        for tick in ticks {
            let y = self.y_at(tick);
            self.painter.line_segment(
                [egui::pos2(plot.left(), y), egui::pos2(plot.right(), y)],
                egui::Stroke::new(
                    1.0_f32,
                    egui::Color32::from_rgba_unmultiplied(
                        theme::CONTROL.r(),
                        theme::CONTROL.g(),
                        theme::CONTROL.b(),
                        CURVE_GRID_LINE_ALPHA,
                    ),
                ),
            );
            self.painter.text(
                egui::pos2(plot.left() - 6.0, y),
                egui::Align2::RIGHT_CENTER,
                fmt_points(Decimal::from_f64_retain(f64::from(tick)).unwrap_or_default()),
                egui::FontId::monospace(10.0),
                theme::TEXT_FAINT,
            );
        }
    }

    /// Diverging fill to the zero baseline: gains ground in BUY, losses in
    /// SELL, split exactly at each crossing. Then the zero baseline over the
    /// fill, and the line over everything.
    fn fill_and_line(&self, stride: usize) {
        let (n, equity) = (self.n, self.equity);
        let zero_y = self.y_at(0.0);
        let mut drawn_points = vec![egui::pos2(self.x_at(0), self.y_at(equity[0]))];
        let mut previous = 0usize;
        let mut next = stride;
        while previous < n {
            let k = next.min(n);
            let (a, b) = (equity[previous], equity[k]);
            self.fill_step(self.x_at(previous), self.x_at(k), a, b, zero_y);
            drawn_points.push(egui::pos2(self.x_at(k), self.y_at(b)));
            previous = k;
            next += stride;
        }
        let plot = self.plot;
        self.painter.line_segment(
            [
                egui::pos2(plot.left(), zero_y),
                egui::pos2(plot.right(), zero_y),
            ],
            egui::Stroke::new(1.0_f32, theme::BORDER),
        );
        self.painter.add(egui::Shape::line(
            drawn_points,
            egui::Stroke::new(1.5_f32, theme::TEXT_PRIMARY),
        ));
        if n == 1 {
            self.painter.circle_filled(
                egui::pos2(self.x_at(1), self.y_at(equity[1])),
                2.0,
                theme::TEXT_PRIMARY,
            );
            self.painter.text(
                plot.center(),
                egui::Align2::CENTER_CENTER,
                "one trade — no curve yet",
                egui::FontId::monospace(10.0),
                theme::TEXT_FAINT,
            );
        }
    }

    /// The fill under one step of the walk, from `a` at `x0` to `b` at `x1`.
    fn fill_step(&self, x0: f32, x1: f32, a: f32, b: f32, zero_y: f32) {
        let fill_of = |value: f32| {
            let color = if value >= 0.0 {
                theme::BUY
            } else {
                theme::SELL
            };
            egui::Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), CURVE_FILL_ALPHA)
        };
        if a == 0.0 && b == 0.0 {
            // Nothing to fill on the baseline itself.
        } else if (a >= 0.0) == (b >= 0.0) {
            self.painter.add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(x0, self.y_at(a)),
                    egui::pos2(x1, self.y_at(b)),
                    egui::pos2(x1, zero_y),
                    egui::pos2(x0, zero_y),
                ],
                fill_of(if a == 0.0 { b } else { a }),
                egui::Stroke::NONE,
            ));
        } else {
            // The step crosses zero: split it at the exact crossing.
            let t = a / (a - b);
            let xc = x0 + (x1 - x0) * t;
            self.painter.add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(x0, self.y_at(a)),
                    egui::pos2(xc, zero_y),
                    egui::pos2(x0, zero_y),
                ],
                fill_of(a),
                egui::Stroke::NONE,
            ));
            self.painter.add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(xc, zero_y),
                    egui::pos2(x1, self.y_at(b)),
                    egui::pos2(x1, zero_y),
                ],
                fill_of(b),
                egui::Stroke::NONE,
            ));
        }
    }

    /// The deepest drawdown, locatable: its peak level dotted, the drop
    /// solid, the number on a chip at the drop's midpoint.
    fn drawdown(&self, peak: usize, trough: usize, view: &ReportView) {
        if trough > self.n {
            return;
        }
        let painter = &self.painter;
        let peak_y = self.y_at(self.equity[peak]);
        let trough_x = self.x_at(trough);
        let trough_y = self.y_at(self.equity[trough]);
        painter.extend(egui::Shape::dashed_line(
            &[
                egui::pos2(self.x_at(peak), peak_y),
                egui::pos2(trough_x, peak_y),
            ],
            egui::Stroke::new(1.0_f32, theme::SELL),
            2.0,
            3.0,
        ));
        painter.line_segment(
            [egui::pos2(trough_x, peak_y), egui::pos2(trough_x, trough_y)],
            egui::Stroke::new(1.0_f32, theme::SELL),
        );
        let label = format!("-{} pts", fmt_points(view.report.max_drawdown_points));
        let galley = painter.layout_no_wrap(label, egui::FontId::monospace(10.0), theme::CHIP_INK);
        let center = egui::pos2(
            (self.x_at(peak) + trough_x) / 2.0,
            (peak_y + trough_y) / 2.0,
        );
        let bg = egui::Rect::from_center_size(center, galley.size() + egui::vec2(8.0, 4.0));
        painter.rect_filled(bg, egui::Rounding::same(2.0), theme::SELL);
        painter.galley(bg.min + egui::vec2(4.0, 2.0), galley, theme::CHIP_INK);
    }

    /// Axis words: first and last trade index, with the unit between them,
    /// and the downsampling note when the line skips trades.
    fn axis_words(&self, stride: usize) {
        let (painter, plot, rect) = (&self.painter, self.plot, self.rect);
        painter.text(
            egui::pos2(plot.left(), rect.bottom()),
            egui::Align2::LEFT_BOTTOM,
            "1",
            egui::FontId::monospace(10.0),
            theme::TEXT_FAINT,
        );
        painter.text(
            egui::pos2(plot.right(), rect.bottom()),
            egui::Align2::RIGHT_BOTTOM,
            self.n.to_string(),
            egui::FontId::monospace(10.0),
            theme::TEXT_FAINT,
        );
        painter.text(
            egui::pos2(plot.center().x, rect.bottom()),
            egui::Align2::CENTER_BOTTOM,
            "trades",
            egui::FontId::monospace(10.0),
            theme::TEXT_FAINT,
        );
        if stride > 1 {
            painter.text(
                egui::pos2(plot.left() + 4.0, plot.top() + 2.0),
                egui::Align2::LEFT_TOP,
                format!(
                    "curve downsampled to ~{CURVE_MAX_POINTS} points; every metric uses every trade"
                ),
                egui::FontId::proportional(10.0),
                theme::TEXT_SUPPORT,
            );
        }
    }

    /// The crosshair at the trade nearest the pointer, and its card.
    fn hover(&self, pointer: egui::Pos2, view: &ReportView) {
        let (n, plot) = (self.n, self.plot);
        let k = (((pointer.x - plot.left()) / plot.width()) * (n as f32))
            .round()
            .clamp(0.0, n as f32) as usize;
        let x = self.x_at(k);
        self.painter.line_segment(
            [egui::pos2(x, plot.top()), egui::pos2(x, plot.bottom())],
            egui::Stroke::new(1.0_f32, theme::TEXT_FAINT),
        );
        let head = if k == 0 {
            "start".to_owned()
        } else {
            let row = &view.rows[k - 1];
            format!(
                "#{k} · {} · {} {} · {} pts",
                row.symbol,
                position_word(row.trade.side),
                fmt_decimal(row.trade.quantity),
                fmt_signed_points(row.trade.pnl_points),
            )
        };
        let lines = [
            head,
            format!(
                "equity {} pts",
                fmt_signed_points(
                    Decimal::from_f64_retain(f64::from(self.equity[k])).unwrap_or_default()
                )
            ),
            if k == 0 {
                String::new()
            } else {
                // The curve's stamp reads on the same clock as the list
                // under it, not on UTC: two dates for one trade in one
                // window is the confusion this goal exists to end.
                fmt_offset_minute(view.rows[k - 1].trade.closed_ms, view.tz)
            },
        ];
        draw_hover_card(&self.painter, self.rect, pointer, &lines);
    }
}

/// A small TAG_BG hover card with up to three lines, clamped into `rect`.
fn draw_hover_card(
    painter: &egui::Painter,
    rect: egui::Rect,
    pointer: egui::Pos2,
    lines: &[String],
) {
    let font = egui::FontId::monospace(10.0);
    let galleys: Vec<_> = lines
        .iter()
        .filter(|line| !line.is_empty())
        .map(|line| painter.layout_no_wrap(line.clone(), font.clone(), theme::TEXT_PRIMARY))
        .collect();
    if galleys.is_empty() {
        return;
    }
    let width = galleys
        .iter()
        .map(|galley| galley.size().x)
        .fold(0.0_f32, f32::max)
        + 12.0;
    let height: f32 = galleys.iter().map(|galley| galley.size().y).sum::<f32>() + 10.0;
    let mut origin = pointer + egui::vec2(10.0, -height - 6.0);
    origin.x = origin.x.min(rect.right() - width).max(rect.left());
    origin.y = origin.y.max(rect.top());
    let card = egui::Rect::from_min_size(origin, egui::vec2(width, height));
    painter.rect_filled(card, egui::Rounding::same(4.0), theme::TAG_BG);
    let mut y = card.top() + 5.0;
    for galley in galleys {
        let advance = galley.size().y;
        painter.galley(
            egui::pos2(card.left() + 6.0, y),
            galley,
            theme::TEXT_PRIMARY,
        );
        y += advance;
    }
}
