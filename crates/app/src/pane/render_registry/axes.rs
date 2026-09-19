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

pub(in crate::pane) struct TimeStripPass<'a> {
    pub painter: &'a egui::Painter,
    pub strip: egui::Rect,
    pub start: usize,
    pub end: usize,
    pub total: usize,
    pub claims: &'a pointer_compass::AxisClaims,
    pub style: &'a ChartStyle,
    pub tz: TzOffset,
    pub divider_x: Option<f32>,
    pub viewport: &'a crate::viewport::Viewport,
    pub series: super::super::drawing_projection::PaneSeriesRead<'a>,
}
impl TimeStripPass<'_> {
    pub fn paint(&self) {
        let Self {
            painter,
            strip,
            start,
            end,
            total,
            claims,
            ..
        } = *self;
        painter.line_segment(
            [
                egui::pos2(strip.left(), strip.top()),
                egui::pos2(strip.right(), strip.top()),
            ],
            egui::Stroke::new(1.0_f32, grid_color(self.style)),
        );
        let font = egui::FontId::monospace(crate::chart::TIME_LABEL_FONT_PX);
        let y = strip.center().y;
        let visible = end.saturating_sub(start);
        if visible == 0 {
            return;
        }
        let (history_strip, _) = split_time_strip(strip, self.divider_x);

        // Measured, not counted. One layout per format per frame — monospace,
        // so a format's sample answers for every label written in it — and the
        // stride comes out of pixels rather than out of a fixed label count
        // that a narrower strip could not honour.
        let width_of = |format: crate::chart::TimeLabelFormat| {
            painter
                .layout_no_wrap(format.sample().to_owned(), font.clone(), theme::TEXT_MUTED)
                .size()
                .x
        };
        let format = crate::chart::time_label_format(history_strip.width(), width_of);
        let label_width = width_of(format);
        // The pointer's chip is always written in full, whatever this strip
        // thinned its own labels down to, so the two extents are asked for
        // separately: a narrow strip pairs a 30 px label with a 54 px chip.
        let chip_width = width_of(crate::chart::TimeLabelFormat::Full);
        // Per *bar*, not per slot: the walk below steps a bar at a time and
        // labels the bar it lands on, so how far apart two labels end up is
        // how far apart two bars are.
        let stride = crate::chart::time_label_stride(self.viewport.px_per_bar(), label_width);

        let mut index = start;
        while index < end {
            if let Some(bar) = self.series.closed_bar(index) {
                let x = self.viewport.x_center(index, history_strip.right(), total);
                // The whole label, not just its centre: a label centred a few
                // pixels from the end drew its other half over the gutter.
                if crate::chart::label_fits(
                    x,
                    label_width,
                    history_strip.left(),
                    history_strip.right(),
                ) && !pointer_compass::claimed(
                    x,
                    label_width,
                    chip_width,
                    claims.iter().copied(),
                ) {
                    painter.text(
                        egui::pos2(x, y),
                        egui::Align2::CENTER_CENTER,
                        crate::plot_area::fmt_time_as(bar.open_time, self.tz, format),
                        font.clone(),
                        theme::TEXT_MUTED,
                    );
                }
            }
            index = index.saturating_add(stride);
        }
    }
}

pub(in crate::pane) struct LaneTimeAxisPass<'a> {
    pub painter: &'a egui::Painter,
    pub lane_strip: Option<egui::Rect>,
    pub window_ms: i64,
    pub tape_age: Option<quantick_orderflow::TapeAge>,
}
impl LaneTimeAxisPass<'_> {
    pub fn paint(&self) {
        let Self {
            painter,
            lane_strip,
            window_ms,
            tape_age,
        } = *self;
        let Some(strip) = lane_strip else {
            return;
        };
        // Clipped to the strip, because both labels are sized from the text
        // rather than from the room: a lane narrow enough to make the warning
        // wider than its own strip would otherwise push it left, over the
        // candles' own time labels. The tape's axis may run out of room; it
        // may not spill into the pane beside it.
        let painter = &painter.with_clip_rect(strip);
        let font = egui::FontId::monospace(super::super::LANE_AXIS_FONT_PX);
        // The warning is its own text, pinned to the right end of the strip,
        // and the window keeps the centre it has always had. One label growing
        // a suffix would re-centre itself every time a quiet stretch started
        // and ended — a caption sliding under a tape being read for flow. The
        // right end is also where it belongs: directly under the edge the
        // missing marks should have reached.
        let warning = quantick_orderflow::lane_lag_label(window_ms, tape_age)
            .map(|lag| painter.layout_no_wrap(lag, font.clone(), theme::WARN));
        // Room the warning denies the window label. Doubled, because the window
        // keeps the strip's own centre: a centred label grows by half its
        // width towards each end, so it reaches the warning after only half
        // the distance, and subtracting the warning once would let a
        // mid-width lane pass this check and draw the two on top of each
        // other. Two gaps rather than one for the same reason — one holds the
        // warning off the strip's edge, and the other is the space between
        // the two labels, which is what the constant is for. Reserving a
        // single gap left them legal at zero pixels apart.
        let taken = warning.as_ref().map_or(0.0, |galley| {
            2.0 * (galley.size().x + 2.0 * super::super::LANE_AXIS_GAP_PX)
        });
        let window_label = format!("tape · {}", quantick_orderflow::format_window_ms(window_ms));
        let window_galley = painter.layout_no_wrap(window_label, font, theme::TEXT_MUTED);
        // A strip too narrow keeps the urgent label and drops this one. The
        // window is a setting the trader chose and can read from the tape's
        // own menu; how old the newest mark is exists nowhere else.
        //
        // It applies with no warning up too, so a lane narrower than this
        // label draws no axis at all rather than a clipped one. Half a word
        // under a tape is not a shorter way of saying the same thing.
        if window_galley.size().x + taken <= strip.width() {
            painter.galley(
                egui::Align2::CENTER_CENTER
                    .align_size_within_rect(window_galley.size(), strip)
                    .min,
                window_galley,
                theme::TEXT_MUTED,
            );
        }
        if let Some(galley) = warning {
            // Right, under the edge the missing marks should have reached —
            // unless it does not fit, and then hard left instead.
            //
            // The clip decides *which end* gets cut, and for this label that
            // is the difference between a shortened sentence and a wrong
            // number. Right-aligned, a 40 px strip cuts the head off
            // "no print for 1 min 30 s" and leaves "30 s" sitting in warn
            // colour: a ninety-second hole read as three. Left-aligned the cut
            // lands on the tail, where a clipped word is visibly a clipped
            // word. A caption that runs out of room may say less; it may not
            // say something else.
            let fits = galley.size().x + 2.0 * super::super::LANE_AXIS_GAP_PX <= strip.width();
            let align = if fits {
                egui::Align2::RIGHT_CENTER
            } else {
                egui::Align2::LEFT_CENTER
            };
            painter.galley(
                align
                    .align_size_within_rect(
                        galley.size(),
                        strip.shrink2(egui::vec2(super::super::LANE_AXIS_GAP_PX, 0.0)),
                    )
                    .min,
                galley,
                theme::WARN,
            );
        }
    }
}

pub(in crate::pane) struct AxisMarksPass<'a> {
    pub painter: &'a egui::Painter,
    pub chart_rect: egui::Rect,
    pub axis_x: f32,
    pub scale: &'a PriceScale,
    pub levels: &'a [super::super::PriceAxisLevel],
    pub newest: Option<&'a quantick_engine::Bar>,
    pub last_price_visible: bool,
    pub style: &'a ChartStyle,
}
impl AxisMarksPass<'_> {
    pub fn paint(&self, registry: &super::RenderRegistry) {
        self.draw_drawing_axis_tags();
        if self.last_price_visible
            && let Some(bar) = self.newest
        {
            registry.last_price(&mut LastPricePass {
                painter: self.painter,
                chart_rect: self.chart_rect,
                axis_x: self.axis_x,
                scale: self.scale,
                bar,
                style: self.style,
            });
        }
    }
    fn draw_drawing_axis_tags(&self) {
        let Self {
            painter,
            chart_rect,
            axis_x,
            levels,
            ..
        } = *self;
        for level in levels {
            if level.y < chart_rect.top() || level.y > chart_rect.bottom() {
                continue;
            }
            // The object's own colour, in the last-price chip's language,
            // because it is the same kind of statement: a price this chart is
            // telling you about, at the height it sits. The ink is *computed*
            // rather than borrowed from that chip — the last price wears one
            // of two saturated colours and a drawing wears whatever the
            // trader picked, dark navy included.
            pointer_compass::paint_price_tag(
                painter,
                axis_x,
                level.y,
                pointer_compass::price_text(level.price),
                level.color,
                theme::ink_on(level.color),
            );
        }
    }
}
