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
use crate::paper_chrome::{caption, fmt_decimal, fmt_points, position_word};
use crate::theme;
use quantick_paper::report::EquityWalk;

/// The realized equity curve, `E_k` by trade index — the closing order
/// that defines the drawdown, so calling the axis "time" would misstate
/// what is plotted. One quiet line; a diverging fill against the zero
/// baseline answers "was I ever under water?" at a glance; the deepest
/// drawdown is annotated so the number in the grid below is locatable.
/// Units that never add up stack one curve each, sharing the height.
pub(super) fn draw_equity_curve(ui: &mut egui::Ui, view: &ReportView, floor: f32) {
    if view.rows.is_empty() {
        return;
    }
    let count = view.equity.walks().len() as f32;
    let height = ((ui.available_height() - CURVE_GRID_RESERVE_PX) / count)
        .clamp(floor / count, CURVE_MAX_H_PX / count);
    for walk in view.equity.walks() {
        draw_walk(ui, view, walk, height);
    }
}

fn draw_walk(ui: &mut egui::Ui, view: &ReportView, walk: &EquityWalk, height: f32) {
    let palette = theme::get(ui.ctx());
    let n = view.rows.len();
    ui.label(caption(&palette, &walk.caption()));
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::hover(),
    );
    if !ui.is_rect_visible(rect) {
        return;
    }
    // The walk E_0..E_n was cut with the view; the frame only plots it.
    let (low, high) = (walk.low, walk.high);
    let curve = CurveFrame {
        palette: palette.clone(),
        painter: ui.painter_at(rect),
        rect,
        plot: egui::Rect::from_min_max(
            egui::pos2(rect.left() + CURVE_GUTTER_PX, rect.top() + 4.0),
            egui::pos2(rect.right() - 4.0, rect.bottom() - 14.0),
        ),
        n,
        low,
        span: (high - low).max(f32::EPSILON),
        walk,
    };
    curve.grid(high);
    let stride = walk.stride(CURVE_MAX_POINTS);
    curve.fill_and_line(stride);
    if let Some((peak, trough, _)) = walk.drawdown {
        curve.drawdown(peak, trough);
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
    palette: std::sync::Arc<theme::Palette>,
    painter: egui::Painter,
    rect: egui::Rect,
    /// The plotting area inside `rect`, past the tick gutter.
    plot: egui::Rect,
    /// Trades plotted; the walk holds `n + 1` points.
    n: usize,
    low: f32,
    span: f32,
    walk: &'a EquityWalk,
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
        let palette = &self.palette;
        let plot = self.plot;
        let mut ticks = vec![self.low, 0.0, high];
        ticks.dedup_by(|a, b| (*a - *b).abs() < f32::EPSILON);
        for tick in ticks {
            let y = self.y_at(tick);
            self.painter.line_segment(
                [egui::pos2(plot.left(), y), egui::pos2(plot.right(), y)],
                egui::Stroke::new(
                    1.0_f32,
                    theme::with_opacity(palette.control(), CURVE_GRID_LINE_ALPHA),
                ),
            );
            self.painter.text(
                egui::pos2(plot.left() - 6.0, y),
                egui::Align2::RIGHT_CENTER,
                fmt_points(Decimal::from_f64_retain(f64::from(tick)).unwrap_or_default()),
                palette.mono_font(10.0),
                palette.text_faint(),
            );
        }
    }

    /// Diverging fill to the zero baseline: gains and losses each in their
    /// own area colour (`paper::equity_area`), split where the walk crosses
    /// zero (`EquityWalk::fill_pieces`). Then the zero baseline over the
    /// fill, and the line over everything.
    fn fill_and_line(&self, stride: usize) {
        let (palette, plot, equity) = (&self.palette, self.plot, &self.walk.plot);
        let at = |(k, value): (f32, f32)| {
            let x = plot.left() + plot.width() * k / (self.n as f32);
            egui::pos2(x, self.y_at(value))
        };
        for piece in self.walk.fill_pieces(stride) {
            let area = quantick_appearance::paper::equity_area(palette.resolved.dark, piece.gain);
            let fill = theme::with_opacity(*palette.get(area), CURVE_FILL_ALPHA);
            let corners = piece.corners.into_iter().map(at).collect();
            self.painter.add(egui::Shape::convex_polygon(
                corners,
                fill,
                egui::Stroke::NONE,
            ));
        }
        let zero_y = self.y_at(0.0);
        self.painter.line_segment(
            [
                egui::pos2(plot.left(), zero_y),
                egui::pos2(plot.right(), zero_y),
            ],
            egui::Stroke::new(1.0_f32, palette.border()),
        );
        let drawn = self.walk.drawn(stride).into_iter();
        self.painter.add(egui::Shape::line(
            drawn.map(|k| at((k as f32, equity[k]))).collect(),
            egui::Stroke::new(1.5_f32, palette.text_primary()),
        ));
        if self.n == 1 {
            self.painter.circle_filled(
                egui::pos2(self.x_at(1), self.y_at(equity[1])),
                2.0,
                palette.text_primary(),
            );
            self.painter.text(
                plot.center(),
                egui::Align2::CENTER_CENTER,
                "one trade — no curve yet",
                palette.mono_font(10.0),
                palette.text_faint(),
            );
        }
    }

    /// The deepest drawdown, locatable: its peak level dotted, the drop
    /// solid, the number on a chip at the drop's midpoint.
    fn drawdown(&self, peak: usize, trough: usize) {
        let palette = &self.palette;
        if trough > self.n {
            return;
        }
        let painter = &self.painter;
        let peak_y = self.y_at(self.walk.plot[peak]);
        let trough_x = self.x_at(trough);
        let trough_y = self.y_at(self.walk.plot[trough]);
        painter.extend(egui::Shape::dashed_line(
            &[
                egui::pos2(self.x_at(peak), peak_y),
                egui::pos2(trough_x, peak_y),
            ],
            egui::Stroke::new(1.0_f32, palette.sell()),
            2.0,
            3.0,
        ));
        painter.line_segment(
            [egui::pos2(trough_x, peak_y), egui::pos2(trough_x, trough_y)],
            egui::Stroke::new(1.0_f32, palette.sell()),
        );
        let label = self.walk.drawdown_text();
        let chip = quantick_appearance::paper::drawdown_chip(palette.resolved.dark);
        let (fill, ink) = (*palette.get(chip.fill), *palette.get(chip.ink));
        let galley = painter.layout_no_wrap(label, palette.mono_font(10.0), ink);
        let center = egui::pos2(
            (self.x_at(peak) + trough_x) / 2.0,
            (peak_y + trough_y) / 2.0,
        );
        let bg = egui::Rect::from_center_size(center, galley.size() + egui::vec2(8.0, 4.0));
        painter.rect_filled(bg, egui::Rounding::same(2.0), fill);
        painter.galley(bg.min + egui::vec2(4.0, 2.0), galley, ink);
    }

    /// Axis words: first and last trade index, with the unit between them,
    /// and the downsampling note when the line skips trades.
    fn axis_words(&self, stride: usize) {
        let palette = &self.palette;
        let (painter, plot, rect) = (&self.painter, self.plot, self.rect);
        painter.text(
            egui::pos2(plot.left(), rect.bottom()),
            egui::Align2::LEFT_BOTTOM,
            "1",
            palette.mono_font(10.0),
            palette.text_faint(),
        );
        painter.text(
            egui::pos2(plot.right(), rect.bottom()),
            egui::Align2::RIGHT_BOTTOM,
            self.n.to_string(),
            palette.mono_font(10.0),
            palette.text_faint(),
        );
        painter.text(
            egui::pos2(plot.center().x, rect.bottom()),
            egui::Align2::CENTER_BOTTOM,
            "trades",
            palette.mono_font(10.0),
            palette.text_faint(),
        );
        if stride > 1 {
            painter.text(
                egui::pos2(plot.left() + 4.0, plot.top() + 2.0),
                egui::Align2::LEFT_TOP,
                format!(
                    "curve downsampled to ~{CURVE_MAX_POINTS} points; every metric uses every trade"
                ),
                palette.body_font(10.0),
                palette.text_support(),
            );
        }
    }

    /// The crosshair at the trade nearest the pointer, and its card.
    fn hover(&self, pointer: egui::Pos2, view: &ReportView) {
        let palette = &self.palette;
        let (n, plot) = (self.n, self.plot);
        let k = (((pointer.x - plot.left()) / plot.width()) * (n as f32))
            .round()
            .clamp(0.0, n as f32) as usize;
        let x = self.x_at(k);
        self.painter.line_segment(
            [egui::pos2(x, plot.top()), egui::pos2(x, plot.bottom())],
            egui::Stroke::new(1.0_f32, palette.text_faint()),
        );
        let head = if k == 0 {
            "start".to_owned()
        } else {
            let row = &view.rows[k - 1];
            format!(
                "#{k} · {} · {} {} · {}",
                row.symbol,
                position_word(row.trade.side),
                fmt_decimal(row.trade.quantity),
                view.trade_text(k - 1),
            )
        };
        let lines = [
            head,
            format!("equity {}", self.walk.text(k)),
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
    let palette = theme::get(painter.ctx());
    let font = palette.mono_font(10.0);
    let galleys: Vec<_> = lines
        .iter()
        .filter(|line| !line.is_empty())
        .map(|line| painter.layout_no_wrap(line.clone(), font.clone(), palette.text_primary()))
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
    painter.rect_filled(card, egui::Rounding::same(4.0), palette.tag_bg());
    let mut y = card.top() + 5.0;
    for galley in galleys {
        let advance = galley.size().y;
        painter.galley(
            egui::pos2(card.left() + 6.0, y),
            galley,
            palette.text_primary(),
        );
        y += advance;
    }
}
