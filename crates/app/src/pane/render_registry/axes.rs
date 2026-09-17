//! Axis contributions share the claims and geometry resolved for the current frame.
use super::super::{
    LAST_PRICE_CHIP_TEXT, LAST_PRICE_DASH_PX, LAST_PRICE_GAP_PX, LAST_PRICE_LINE_ALPHA,
    PointerCompass, PriceAxisClaims, grid_color,
};
use super::{Contribution, Package};
use crate::{
    chart::{self, PriceScale},
    plot_area::split_time_strip,
    pointer_compass,
    style::ChartStyle,
    theme,
    timezone::TzOffset,
};
use eframe::egui;
use rust_decimal::prelude::ToPrimitive as _;
pub(super) const PACKAGE: Package = Package {
    layers: &[
        quantick_layers::ChartLayer::Grid,
        quantick_layers::ChartLayer::LastPrice,
        quantick_layers::ChartLayer::Crosshair,
        quantick_layers::ChartLayer::PointerPrice,
        quantick_layers::ChartLayer::PointerTime,
    ],
    contributions: &[
        Contribution::Grid(grid),
        Contribution::LastPrice(last_price),
        Contribution::Crosshair(crosshair),
        Contribution::Pointer(pointer),
    ],
};
pub(in crate::pane) struct GridPass<'a, 'b> {
    pub painter: &'a egui::Painter,
    pub chart_rect: egui::Rect,
    pub axis_x: f32,
    pub scale: &'a PriceScale,
    pub claims: &'a PriceAxisClaims<'b>,
    pub style: &'a ChartStyle,
}
pub(in crate::pane) struct LastPricePass<'a> {
    pub painter: &'a egui::Painter,
    pub chart_rect: egui::Rect,
    pub axis_x: f32,
    pub scale: &'a PriceScale,
    pub bar: &'a quantick_engine::Bar,
    pub style: &'a ChartStyle,
}
pub(in crate::pane) struct CrosshairPass<'a> {
    pub painter: &'a egui::Painter,
    pub chart_rect: egui::Rect,
    pub axis_x: f32,
    pub scale: &'a PriceScale,
    pub pointer: Option<egui::Pos2>,
    pub armed: bool,
}
pub(in crate::pane) struct PointerPass<'a> {
    pub painter: &'a egui::Painter,
    pub compass: &'a PointerCompass,
    pub axis_x: f32,
    pub time_strip: egui::Rect,
    pub divider_x: Option<f32>,
    pub tz: TzOffset,
}
fn grid(p: &mut GridPass<'_, '_>) {
    let GridPass {
        painter,
        chart_rect,
        axis_x,
        scale,
        claims,
        style,
    } = *p;

    let grid = grid_color(style);
    let (lo, hi) = scale.range();
    let font = egui::FontId::monospace(chart::AXIS_LABEL_FONT_PX);
    // Measured once per frame, the way the time strip measures its own:
    // every label on this axis is one line of the same font, so one
    // layout answers for all of them.
    let label_height = painter
        .layout_no_wrap("0".to_owned(), font.clone(), theme::TEXT_MUTED)
        .size()
        .y;
    for tick in crate::chart::nice_ticks(lo, hi, 8) {
        let y = scale.y(tick);
        if y < chart_rect.top() || y > chart_rect.bottom() {
            continue;
        }
        // The *line* is drawn either way: a gridline under a chip is
        // still the grid, and hiding it would put a gap in the chart
        // wherever the pointer went.
        painter.line_segment(
            [
                egui::pos2(chart_rect.left(), y),
                egui::pos2(chart_rect.right(), y),
            ],
            egui::Stroke::new(1.0_f32, grid),
        );
        // The chips on this axis are the same font and padding as the
        // labels, so one extent answers for both — unlike the time strip,
        // where they differ.
        if pointer_compass::claimed(y, label_height, label_height, claims.heights()) {
            continue;
        }
        painter.text(
            egui::pos2(axis_x + chart::AXIS_LABEL_GAP_PX, y),
            egui::Align2::LEFT_CENTER,
            pointer_compass::price_text(tick),
            font.clone(),
            theme::TEXT_MUTED,
        );
    }
    // The axis dividing line.
    painter.line_segment(
        [
            egui::pos2(axis_x, chart_rect.top()),
            egui::pos2(axis_x, chart_rect.bottom()),
        ],
        egui::Stroke::new(1.0_f32, grid),
    );
}
fn last_price(p: &mut LastPricePass<'_>) {
    let LastPricePass {
        painter,
        chart_rect,
        axis_x,
        scale,
        bar,
        style,
    } = *p;

    let Some(price) = bar.close.to_f64() else {
        return;
    };
    let y = scale.y(price);
    if y < chart_rect.top() || y > chart_rect.bottom() {
        return;
    }
    // Same predicate and same two colours the candle wears, so the chip
    // and the bar it reports can never disagree about direction.
    let rgb = if crate::candle_view::is_bullish(bar) {
        style.candles.bull_outline
    } else {
        style.candles.bear_outline
    };
    let color = egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2]);

    // Runs through the live strip when one is shown (`axis_x` then sits
    // past it): the depth silhouette is read against this exact line.
    painter.extend(egui::Shape::dashed_line(
        &[egui::pos2(chart_rect.left(), y), egui::pos2(axis_x, y)],
        egui::Stroke::new(1.0_f32, color.gamma_multiply(LAST_PRICE_LINE_ALPHA)),
        LAST_PRICE_DASH_PX,
        LAST_PRICE_GAP_PX,
    ));

    // Same geometry as the crosshair tag and the compass's, because it is
    // the same code: one owner for where a price sits on this axis.
    pointer_compass::paint_price_tag(
        painter,
        axis_x,
        y,
        pointer_compass::price_text(price),
        color,
        LAST_PRICE_CHIP_TEXT,
    );
}
fn crosshair(p: &mut CrosshairPass<'_>) {
    let CrosshairPass {
        painter,
        chart_rect,
        axis_x,
        scale,
        pointer,
        armed,
    } = *p;

    if !armed {
        return;
    }
    let Some(pos) = pointer else {
        return;
    };
    if !chart_rect.contains(pos) {
        return;
    }
    let stroke = egui::Stroke::new(1.0_f32, theme::TEXT_FAINT);
    painter.line_segment(
        [
            egui::pos2(pos.x, chart_rect.top()),
            egui::pos2(pos.x, chart_rect.bottom()),
        ],
        stroke,
    );
    // Reaches the axis through the live strip when one is shown, so the
    // cursor height can be read against the depth silhouette too.
    painter.line_segment(
        [
            egui::pos2(chart_rect.left(), pos.y),
            egui::pos2(axis_x, pos.y),
        ],
        stroke,
    );

    // Price tag on the axis at the cursor height, through the axis's one
    // tag owner — the compass and the last-price chip write theirs the
    // same way, so the marks that share this gutter cannot drift apart.
    pointer_compass::paint_price_tag(
        painter,
        axis_x,
        pos.y,
        pointer_compass::price_text(scale.price_at(pos.y)),
        theme::TAG_BG,
        egui::Color32::WHITE,
    );
}
fn pointer(p: &mut PointerPass<'_>) {
    let PointerPass {
        painter,
        compass,
        axis_x,
        time_strip,
        divider_x,
        tz,
    } = *p;

    if compass.price {
        pointer_compass::paint_price_mark(painter, axis_x, &compass.readout);
    }
    if compass.time {
        let (history_strip, _) = split_time_strip(time_strip, divider_x);
        pointer_compass::paint_time_mark(painter, history_strip, &compass.readout, tz);
    }
}
