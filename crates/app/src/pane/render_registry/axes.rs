//! Axis contributions share the claims and geometry resolved for the current frame.
use super::super::{
    LAST_PRICE_DASH_PX, LAST_PRICE_GAP_PX, LAST_PRICE_LINE_ALPHA, PointerCompass, PriceAxisClaims,
    grid_color,
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
use quantick_appearance::Token;
use quantick_chart::bar_progress::BarProgressReading;
use quantick_chart::candle_pixels::crisp_or_point as crisp;
use quantick_chart::time_marks::{self, BarAt, TimeMark};
use rust_decimal::prelude::ToPrimitive as _;
pub(super) const PACKAGE: Package = Package {
    layers: &[
        quantick_layers::ChartLayer::Grid,
        quantick_layers::ChartLayer::TimeGrid,
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
    pub snap_horizontal: bool,
    /// Where the time labels stand this frame; empty while the time grid is off.
    pub time_grid_xs: &'a [f32],
}
pub(in crate::pane) struct LastPricePass<'a> {
    pub painter: &'a egui::Painter,
    pub chart_rect: egui::Rect,
    pub axis_x: f32,
    pub scale: &'a PriceScale,
    pub bar: &'a quantick_engine::Bar,
    /// The forming bar's progress, printed under the price; `None` while off.
    pub progress: Option<BarProgressReading>,
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
    let palette = theme::axes(p.painter.ctx());
    let GridPass {
        painter,
        chart_rect,
        axis_x,
        scale,
        claims,
        style,
        snap_horizontal,
        time_grid_xs,
    } = *p;

    // Light draws each line on whole device pixels, as TradingView does.
    let ppp = painter.ctx().pixels_per_point();
    let line = |at: f32| crisp(snap_horizontal, at, ppp);
    if let Some(rgba) = style.canvas.time_grid_rgba() {
        let color = super::super::painting::color32(rgba);
        for &x in time_grid_xs {
            let (line_x, width) = line(x);
            let stroke = egui::Stroke::new(width, color);
            painter.vline(line_x, chart_rect.y_range(), stroke);
        }
    }
    let ink = axis_ink(&palette);
    let (lo, hi) = scale.range();
    let size = palette.resolved.chart_axes.price_label_size;
    let font = label_font(&palette, chart::AXIS_LABEL_FONT_PX, size);
    // Measured once per frame, the way the time strip measures its own:
    // every label on this axis is one line of the same font, so one
    // layout answers for all of them.
    let label_height = painter
        .layout_no_wrap("0".to_owned(), font.clone(), ink)
        .size()
        .y;
    for tick in price_grid_ticks(&palette, lo, hi, chart_rect.height()) {
        let y = scale.y(tick);
        if y < chart_rect.top() || y > chart_rect.bottom() {
            continue;
        }
        // The *line* is drawn either way: a gridline under a chip is
        // still the grid, and hiding it would put a gap in the chart
        // wherever the pointer went.
        let (line_y, width) = line(y);
        let grid = egui::Stroke::new(width, grid_color(style));
        painter.hline(chart_rect.x_range(), line_y, grid);
        // The chips on this axis are the same font and padding as the
        // labels, so one extent answers for both — unlike the time strip,
        // where they differ.
        if pointer_compass::claimed(y, label_height, label_height, claims.heights()) {
            continue;
        }
        // A label half off the pane reads as clipped, not as a price:
        // TradingView keeps only the labels whose whole text fits.
        let (top, bottom) = (chart_rect.top(), chart_rect.bottom());
        if !(top + label_height / 2.0..=bottom - label_height / 2.0).contains(&y) {
            continue;
        }
        painter.text(
            egui::pos2(axis_x + chart::AXIS_LABEL_GAP_PX, y),
            egui::Align2::LEFT_CENTER,
            pointer_compass::price_text(tick),
            font.clone(),
            ink,
        );
    }
    // The axis dividing line.
    let rule = axis_rule(&palette, style, Token::PriceAxisRule);
    let (rule_x, width) = crisp(!palette.resolved.dark, axis_x, ppp);
    painter.vline(rule_x, chart_rect.y_range(), egui::Stroke::new(width, rule));
}
fn last_price(p: &mut LastPricePass<'_>) {
    let LastPricePass {
        painter,
        chart_rect,
        axis_x,
        scale,
        bar,
        progress,
        style,
    } = *p;

    let Some(price) = bar.close.to_f64() else {
        return;
    };
    let y = scale.y(price);
    if y < chart_rect.top() || y > chart_rect.bottom() {
        return;
    }
    // Same predicate the candle uses, so the chip and the bar it reports can
    // never disagree about direction; the colours are the theme's choice.
    let theme = &theme::get(painter.ctx()).resolved;
    let up = crate::candle_view::is_bullish(bar);
    let rgb = quantick_chart::price_tag::last_price_rgb(theme, &style.candles, up);
    let color = egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2]);

    // Runs through the live strip when one is shown (`axis_x` then sits
    // past it): the depth silhouette is read against this exact line.
    painter.extend(egui::Shape::dashed_line(
        &[egui::pos2(chart_rect.left(), y), egui::pos2(axis_x, y)],
        egui::Stroke::new(1.0_f32, color.gamma_multiply(LAST_PRICE_LINE_ALPHA)),
        LAST_PRICE_DASH_PX,
        LAST_PRICE_GAP_PX,
    ));

    // A countdown changes with no print: ask for the frame its text turns on,
    // at most a quarter second away so a fast replay's clock keeps up.
    if let Some(ms) = progress.and_then(|reading| reading.repaint_after_ms()) {
        let after = std::time::Duration::from_millis(ms.unsigned_abs());
        painter.ctx().request_repaint_after(after);
    }
    // Same geometry as the crosshair tag and the compass's, because it is
    // the same code: one owner for where a price sits on this axis.
    pointer_compass::paint_price_tag(
        painter,
        axis_x,
        y,
        pointer_compass::price_text(price),
        progress.map(|reading| reading.tag_text()),
        color,
        theme::get(painter.ctx()).chip_ink(),
    );
}
fn crosshair(p: &mut CrosshairPass<'_>) {
    let palette = theme::get(p.painter.ctx());
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
    let stroke = egui::Stroke::new(1.0_f32, palette.text_faint());
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

    let axis_palette = theme::axes(painter.ctx());
    // Price tag on the axis at the cursor height, through the axis's one
    // tag owner — the compass and the last-price chip write theirs the
    // same way, so the marks that share this gutter cannot drift apart.
    pointer_compass::paint_price_tag(
        painter,
        axis_x,
        pos.y,
        pointer_compass::price_text(scale.price_at(pos.y)),
        None,
        axis_palette.tag_bg(),
        axis_palette.crosshair_ink(),
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

/// One frame's time labels, in the font they are written in.
pub(in crate::pane) type StripMarks = (egui::FontId, Vec<TimeMark>);
#[cfg(test)]
thread_local! {
    /// How many times this thread laid the time labels out.
    pub(crate) static MARK_RUNS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub(in crate::pane) struct TimeStripPass<'a> {
    pub painter: &'a egui::Painter,
    pub strip: egui::Rect,
    pub start: usize,
    pub end: usize,
    pub total: usize,
    /// The pointer's chip, as wide as its own text this frame.
    pub chip: Option<pointer_compass::TimeChip>,
    pub style: &'a ChartStyle,
    pub tz: TzOffset,
    pub candles_right: Option<f32>,
    pub viewport: &'a crate::viewport::Viewport,
    pub series: quantick_drawings::gesture::PaneSeriesRead<'a>,
}
impl TimeStripPass<'_> {
    /// The strip's labels this frame, in the font they are written in.
    ///
    /// Intraday charts label round instants the way TradingView does
    /// (`quantick_chart::time_marks`): `HH:MM`, the day number where a day
    /// turns, the month and the year where those do. A chart cut at a day or
    /// longer writes its UTC buckets' dates every few bars, as it always has.
    pub fn marks(&self) -> StripMarks {
        #[cfg(test)]
        MARK_RUNS.with(|runs| runs.set(runs.get() + 1));
        let palette = &theme::axes(self.painter.ctx());
        let size = palette.resolved.chart_axes.time_label_size;
        let font = label_font(palette, crate::chart::TIME_LABEL_FONT_PX, size);
        if self.end <= self.start {
            return (font, Vec::new());
        }
        let (history_strip, _) = split_time_strip(self.strip, self.candles_right);
        let width_of = |text: &str| {
            self.painter
                .layout_no_wrap(text.to_owned(), font.clone(), palette.text_muted())
                .size()
                .x
        };
        let bars: Vec<BarAt> = (self.start..self.end)
            .filter_map(|slot| {
                let bar = self.series.closed_bar(slot)?;
                let x = self
                    .viewport
                    .x_center(slot, history_strip.right(), self.total);
                Some(BarAt {
                    slot,
                    x,
                    open_ms: bar.open_time,
                })
            })
            .collect();
        let seed = self
            .start
            .checked_sub(1)
            .and_then(|slot| self.series.slot_open_time(slot));
        let interval = self.series.spec.spec().time_interval_ms();
        let (span, zoom) = (
            (history_strip.left(), history_strip.right()),
            self.viewport.px_per_bar(),
        );
        let marks = time_marks::strip_marks(&bars, seed, interval, self.tz, zoom, span, width_of);
        (font, marks)
    }

    /// The rule and `marks`' labels, as [`Self::marks`] laid them out.
    pub fn paint(&self, (font, marks): &StripMarks) {
        let palette = theme::axes(self.painter.ctx());
        let painter = self.painter;
        let strip = self.strip;
        let rule = axis_rule(&palette, self.style, Token::TimeAxisRule);
        let ppp = painter.ctx().pixels_per_point();
        let (rule_y, width) = crisp(!palette.resolved.dark, strip.top(), ppp);
        painter.line_segment(
            [
                egui::pos2(strip.left(), rule_y),
                egui::pos2(strip.right(), rule_y),
            ],
            egui::Stroke::new(width, rule),
        );
        let ink = axis_ink(&palette);
        let y = strip.center().y;
        for mark in marks {
            let galley = painter.layout_no_wrap(mark.text.clone(), font.clone(), ink);
            if pointer_compass::time_claimed(self.chip, mark.x, galley.size().x) {
                continue;
            }
            let at = egui::pos2(mark.x, y) - galley.size() / 2.0;
            painter.galley(at, galley, ink);
        }
    }
}

/// The price gridlines: a steady spacing when the theme sets one, else the
/// fixed ask of eight lines Dark has always drawn.
pub(in crate::pane) fn price_grid_ticks(
    palette: &crate::theme::Palette,
    lo: f64,
    hi: f64,
    height_px: f32,
) -> Vec<f64> {
    match palette.resolved.chart_axes.price_line_spacing {
        Some(spacing) => quantick_chart::price_grid::ticks(lo, hi, height_px, spacing),
        None => crate::chart::nice_ticks(lo, hi, 8),
    }
}

/// The axis font at `reference`, or at the size the theme fixes for it.
fn label_font(palette: &crate::theme::Palette, reference: f32, size: Option<f32>) -> egui::FontId {
    let mut font = palette.axis_font(reference);
    if let Some(size) = size {
        font.size = size;
    }
    font
}

/// Light themes write axis labels in their own ink; Dark keeps `text_muted`.
pub(in crate::pane) fn axis_ink(palette: &crate::theme::Palette) -> egui::Color32 {
    if palette.resolved.dark {
        palette.text_muted()
    } else {
        *palette.get(Token::AxisInk)
    }
}

/// Light themes rule their axes in their own colours; Dark rules them in the
/// chart grid's, so its rules come and go with the grid switch.
fn axis_rule(palette: &crate::theme::Palette, style: &ChartStyle, token: Token) -> egui::Color32 {
    if palette.resolved.dark {
        grid_color(style)
    } else {
        *palette.get(token)
    }
}

pub(in crate::pane) struct LaneTimeAxisPass<'a> {
    pub painter: &'a egui::Painter,
    pub lane_strip: Option<egui::Rect>,
    pub window_ms: i64,
    pub tape_age: Option<quantick_orderflow::TapeAge>,
    /// The live edge's instant and the zone, when the tape is the whole pane
    /// and its axis labels the tape's own clock.
    pub clock: Option<(
        i64,
        crate::timezone::TzOffset,
        quantick_orderflow::projection::TapeHorizontalGeometry,
    )>,
}
impl LaneTimeAxisPass<'_> {
    pub fn paint(&self) {
        let palette = theme::axes(self.painter.ctx());
        let Self {
            painter,
            lane_strip,
            window_ms,
            tape_age,
            clock,
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
        let font = palette.axis_font(super::super::LANE_AXIS_FONT_PX);
        // The warning is its own text, pinned to the right end of the strip,
        // and the window keeps the centre it has always had. One label growing
        // a suffix would re-centre itself every time a quiet stretch started
        // and ended — a caption sliding under a tape being read for flow. The
        // right end is also where it belongs: directly under the edge the
        // missing marks should have reached.
        let warning = quantick_orderflow::lane_lag_label(window_ms, tape_age)
            .map(|lag| painter.layout_no_wrap(lag, font.clone(), palette.warn()));
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
        let window_galley = painter.layout_no_wrap(window_label, font, axis_ink(&palette));
        // A strip too narrow keeps the urgent label and drops this one. The
        // window is a setting the trader chose and can read from the tape's
        // own menu; how old the newest mark is exists nowhere else.
        //
        // It applies with no warning up too, so a lane narrower than this
        // label draws no axis at all rather than a clipped one. Half a word
        // under a tape is not a shorter way of saying the same thing.
        let mut labelled = Vec::new();
        if window_galley.size().x + taken <= strip.width() {
            let rect =
                egui::Align2::CENTER_CENTER.align_size_within_rect(window_galley.size(), strip);
            labelled.push(rect);
            painter.galley(rect.min, window_galley, axis_ink(&palette));
        }
        if let Some((end_ms, tz, geometry)) = clock {
            if let Some(galley) = warning.as_ref() {
                let right = strip.right() - super::super::LANE_AXIS_GAP_PX;
                labelled.push(egui::Rect::from_min_max(
                    egui::pos2(right - galley.size().x, strip.top()),
                    egui::pos2(right, strip.bottom()),
                ));
            }
            paint_tape_clock(painter, strip, end_ms, window_ms, tz, &labelled, geometry);
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
                palette.warn(),
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
    pub progress: Option<BarProgressReading>,
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
                progress: self.progress,
                style: self.style,
            });
        }
    }
    fn draw_drawing_axis_tags(&self) {
        let palette = crate::theme::get(self.painter.ctx());
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
            // trader picked, dark navy included. A theme without label
            // plates paints the price in that colour over the axis instead.
            let (fill, ink) = palette.drawing_tag_paint(level.color);
            pointer_compass::paint_price_tag(
                painter,
                axis_x,
                level.y,
                pointer_compass::price_text(level.price),
                None,
                fill,
                ink,
            );
        }
    }
}

/// The tape's own clock under a tape-only pane: round instants of its window
/// at their place on the strip, newest at the right edge, none over a label
/// already drawn there.
fn paint_tape_clock(
    painter: &egui::Painter,
    strip: egui::Rect,
    end_ms: i64,
    window_ms: i64,
    tz: crate::timezone::TzOffset,
    labelled: &[egui::Rect],
    geometry: quantick_orderflow::projection::TapeHorizontalGeometry,
) {
    let palette = theme::axes(painter.ctx());
    let format = crate::chart::TimeLabelFormat::Full;
    let font = palette.axis_font(crate::chart::TIME_LABEL_FONT_PX);
    let label_width = painter
        .layout_no_wrap(format.sample().to_owned(), font.clone(), axis_ink(&palette))
        .size()
        .x;
    let slot = label_width + 2.0 * super::super::LANE_AXIS_GAP_PX;
    let room = (geometry.span_px / slot).floor();
    if window_ms <= 0 || !room.is_finite() || room < 1.0 {
        return;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let max_labels = room as usize;
    let start_ms = end_ms.saturating_sub(window_ms);
    #[allow(clippy::cast_precision_loss)]
    let x_of =
        |instant: i64| strip.left() + geometry.x((instant - start_ms) as f64 / window_ms as f64);
    // Fed only the labels painted: the first in a new day names it, seeded
    // by the window's start so the turn is named once the old day scrolls off.
    let mut ticks = quantick_chart::day_turn::TickLabels::new(tz, format, Some(start_ms));
    for instant in quantick_orderflow::lane_time_ticks(end_ms, window_ms, max_labels) {
        let x = x_of(instant);
        let rect = egui::Rect::from_center_size(
            egui::pos2(x, strip.center().y),
            egui::vec2(label_width, strip.height()),
        );
        if !crate::chart::label_fits(x, label_width, strip.left(), strip.right())
            || labelled.iter().any(|taken| taken.intersects(rect))
        {
            continue;
        }
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            ticks.label_at(instant),
            font.clone(),
            axis_ink(&palette),
        );
    }
}
