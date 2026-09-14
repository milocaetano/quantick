//! The equity curve, and the card that reads one point of it.
//!
//! Cumulative points against trade number, walked once into a polyline the
//! window paints under the tiles. The walk is separate from the paint so the
//! hover card can name the trade under the pointer without re-deriving the
//! running total.

use eframe::egui;
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;

use super::{
    CURVE_FILL_ALPHA, CURVE_GRID_LINE_ALPHA, CURVE_GRID_RESERVE_PX, CURVE_GUTTER_PX,
    CURVE_MAX_H_PX, CURVE_MAX_POINTS, HistoryRow, ReportView,
};
use crate::paper_calendar::fmt_offset_minute;
use crate::paper_chrome::{caption, fmt_decimal, fmt_points, fmt_signed_points, position_word};
use crate::theme;

/// The realized-equity walk `E_0..E_n` (`E_0 = 0` before the first trade),
/// in the two shapes its two readers need: exact points for the trade
/// list's running total, and plot-ready `f32` with its bounds for the
/// curve. Computed once when the view is cut — a window holding a year of
/// trades must not walk them again sixty times a second.
pub(super) struct EquityWalk {
    /// `n + 1` exact running totals in points.
    pub(super) points: Vec<Decimal>,
    /// The same walk as the curve plots it.
    pub(super) plot: Vec<f32>,
    pub(super) low: f32,
    pub(super) high: f32,
}

impl EquityWalk {
    pub(super) fn of(rows: &[HistoryRow]) -> Self {
        let mut points = Vec::with_capacity(rows.len() + 1);
        let mut plot = Vec::with_capacity(rows.len() + 1);
        points.push(Decimal::ZERO);
        plot.push(0.0_f32);
        let (mut low, mut high) = (0.0_f32, 0.0_f32);
        let mut sum = Decimal::ZERO;
        for row in rows {
            sum = sum.saturating_add(row.trade.pnl_points);
            points.push(sum);
            let value = sum.to_f64().unwrap_or_default() as f32;
            low = low.min(value);
            high = high.max(value);
            plot.push(value);
        }
        Self {
            points,
            plot,
            low,
            high,
        }
    }
}

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
    let painter = ui.painter_at(rect);

    // The walk E_0..E_n was cut with the view; the frame only plots it.
    let equity = &view.equity.plot;
    let (low, high) = (view.equity.low, view.equity.high);
    let span = (high - low).max(f32::EPSILON);
    let plot = egui::Rect::from_min_max(
        egui::pos2(rect.left() + CURVE_GUTTER_PX, rect.top() + 4.0),
        egui::pos2(rect.right() - 4.0, rect.bottom() - 14.0),
    );
    let x_at = |k: usize| plot.left() + plot.width() * (k as f32) / (n as f32);
    let y_at = |value: f32| plot.bottom() - (value - low) / span * plot.height();
    let zero_y = y_at(0.0);

    // Gridlines + y ticks at the floor, zero and the ceiling.
    let mut ticks = vec![low, 0.0, high];
    ticks.dedup_by(|a, b| (*a - *b).abs() < f32::EPSILON);
    for tick in ticks {
        let y = y_at(tick);
        painter.line_segment(
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
        painter.text(
            egui::pos2(plot.left() - 6.0, y),
            egui::Align2::RIGHT_CENTER,
            fmt_points(Decimal::from_f64_retain(f64::from(tick)).unwrap_or_default()),
            egui::FontId::monospace(10.0),
            theme::TEXT_FAINT,
        );
    }

    // Diverging fill to the zero baseline: gains ground in BUY, losses in
    // SELL, split exactly at each crossing.
    let stride = n.div_ceil(CURVE_MAX_POINTS).max(1);
    let fill_of = |value: f32| {
        let color = if value >= 0.0 {
            theme::BUY
        } else {
            theme::SELL
        };
        egui::Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), CURVE_FILL_ALPHA)
    };
    let mut drawn_points = vec![egui::pos2(x_at(0), y_at(equity[0]))];
    let mut previous = 0usize;
    let mut next = stride;
    while previous < n {
        let k = next.min(n);
        let (a, b) = (equity[previous], equity[k]);
        let (x0, x1) = (x_at(previous), x_at(k));
        if a == 0.0 && b == 0.0 {
            // Nothing to fill on the baseline itself.
        } else if (a >= 0.0) == (b >= 0.0) {
            painter.add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(x0, y_at(a)),
                    egui::pos2(x1, y_at(b)),
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
            painter.add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(x0, y_at(a)),
                    egui::pos2(xc, zero_y),
                    egui::pos2(x0, zero_y),
                ],
                fill_of(a),
                egui::Stroke::NONE,
            ));
            painter.add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(xc, zero_y),
                    egui::pos2(x1, y_at(b)),
                    egui::pos2(x1, zero_y),
                ],
                fill_of(b),
                egui::Stroke::NONE,
            ));
        }
        drawn_points.push(egui::pos2(x1, y_at(b)));
        previous = k;
        next += stride;
    }

    // The zero baseline over the fill, then the line over everything.
    painter.line_segment(
        [
            egui::pos2(plot.left(), zero_y),
            egui::pos2(plot.right(), zero_y),
        ],
        egui::Stroke::new(1.0_f32, theme::BORDER),
    );
    painter.add(egui::Shape::line(
        drawn_points,
        egui::Stroke::new(1.5_f32, theme::TEXT_PRIMARY),
    ));
    if n == 1 {
        painter.circle_filled(
            egui::pos2(x_at(1), y_at(equity[1])),
            2.0,
            theme::TEXT_PRIMARY,
        );
        painter.text(
            plot.center(),
            egui::Align2::CENTER_CENTER,
            "one trade — no curve yet",
            egui::FontId::monospace(10.0),
            theme::TEXT_FAINT,
        );
    }

    // The deepest drawdown, locatable: its peak level dotted, the drop
    // solid, the number on a chip at the drop's midpoint.
    if let Some((peak, trough)) = view.report.max_drawdown_span {
        let (peak, trough) = (peak as usize, trough as usize);
        if trough <= n {
            let peak_y = y_at(equity[peak]);
            let trough_x = x_at(trough);
            let trough_y = y_at(equity[trough]);
            painter.extend(egui::Shape::dashed_line(
                &[egui::pos2(x_at(peak), peak_y), egui::pos2(trough_x, peak_y)],
                egui::Stroke::new(1.0_f32, theme::SELL),
                2.0,
                3.0,
            ));
            painter.line_segment(
                [egui::pos2(trough_x, peak_y), egui::pos2(trough_x, trough_y)],
                egui::Stroke::new(1.0_f32, theme::SELL),
            );
            let label = format!("-{} pts", fmt_points(view.report.max_drawdown_points));
            let galley =
                painter.layout_no_wrap(label, egui::FontId::monospace(10.0), theme::CHIP_INK);
            let center = egui::pos2((x_at(peak) + trough_x) / 2.0, (peak_y + trough_y) / 2.0);
            let bg = egui::Rect::from_center_size(center, galley.size() + egui::vec2(8.0, 4.0));
            painter.rect_filled(bg, egui::Rounding::same(2.0), theme::SELL);
            painter.galley(bg.min + egui::vec2(4.0, 2.0), galley, theme::CHIP_INK);
        }
    }

    // Axis words: first and last trade index, with the unit between them.
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
        n.to_string(),
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

    // Hover: snap to the nearest trade index and answer in the ledger's
    // vocabulary, so the two surfaces never disagree.
    if let Some(pointer) = response.hover_pos()
        && plot.contains(pointer)
    {
        let k = (((pointer.x - plot.left()) / plot.width()) * (n as f32))
            .round()
            .clamp(0.0, n as f32) as usize;
        let x = x_at(k);
        painter.line_segment(
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
                    Decimal::from_f64_retain(f64::from(equity[k])).unwrap_or_default()
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
        draw_hover_card(&painter, rect, pointer, &lines);
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
