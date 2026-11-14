//! The pointer's compass: what the two axes say about where the mouse is.
//!
//! The bars on this chart are cut by ticks, volume or dollars, not by the
//! clock. That is the whole point of the project — and it costs the trader the
//! one thing a time chart gives away for free, which is knowing *when* the
//! candle under the pointer happened. The grid cannot answer it, because the
//! grid is not evenly spaced in time; the time strip's labels answer it only
//! every few bars. So the pointer carries its own answer with it: a tick and a
//! value on each axis, following the mouse while it is over the chart and gone
//! the moment it leaves.
//!
//! It is deliberately *not* a crosshair. The crosshair tool draws two rules
//! across the canvas, which is a mode a trader arms to line two things up; this
//! is chrome that costs nothing to read and never crosses market data. The two
//! coexist by division of labour: the crosshair owns the cross, this owns the
//! axis tags, and the pointer render pass hands the price half over
//! while the crosshair is armed so one price can never be tagged twice.
//!
//! Two switches, one per axis, because the two answers are wanted separately:
//! a scalper reading a level wants the price and not the clock, and a trader
//! reconstructing a session wants the clock. Both live on [`ChartLayer`], so
//! they persist, ride `QUANTICK_CHART_LAYERS` and appear in the layer menu
//! like every other layer — and each is also offered on its own axis's
//! right-click menu, which is where a trader looks for something about that
//! axis.
//!
//! **Paint from data.** Everything drawn here is described by
//! [`PointerReadout`], which is produced by [`readout`] and asserted on
//! directly in tests — no test reads pixels to find out what the axis said,
//! and the control plane's cursor scope answers from the same resolver
//! (`ChartPane::control_pointer_hit`), so a client and a trader are never told
//! two different bars.
//!
//! [`ChartLayer`]: crate::chart_layers::ChartLayer

use eframe::egui;
use smallvec::SmallVec;

use crate::chart::{self, PriceScale};
use crate::theme;
use crate::timezone::TzOffset;
pub(crate) use quantick_chart::price_tag::{self, claimed, price_tag_claims};

/// How far the tick leading into a tag reaches out of the chart, in pixels.
///
/// Short on purpose: it exists to join the pointer's height to the chip
/// beside it, so the eye does not have to measure. Longer would draw a second
/// gridline the chart never asked for.
const TICK_PX: f32 = 5.0;

/// Padding around a tag's text, in pixels — the last-price chip's own, so the
/// three marks that can share this axis are the same size.
const TAG_PAD: egui::Vec2 = egui::vec2(price_tag::TAG_PAD_X_PX, 1.0);

/// Corner radius of a tag.
const TAG_ROUNDING: f32 = 2.0;

/// How a price is written on the price axis, wherever it is written.
///
/// One owner for every mark that shares this gutter — the round-number
/// labels, the last-price chip, the crosshair's tag, the compass's, and the
/// level a drawing declares. Five spellings of one price an inch apart is how
/// a gutter starts disagreeing with itself, and it is also why the precision
/// is a single edit away: this is two decimals because every instrument the
/// app has shipped against quotes in halves or cents, and an instrument finer
/// than that needs this function changed rather than five call sites found.
#[must_use]
pub(crate) fn price_text(price: f64) -> String {
    format!("{price:.2}")
}

/// Coordinates on an axis already spoken for by a chip — a `y` on the price
/// axis, an `x` on the time strip.
///
/// A gridline label is the lowest-priority thing an axis says: it names what a
/// round number is worth, which is exactly what a chip landing on it is
/// already saying about a price or an instant that *matters*. Two of them
/// sharing a row is not two facts, it is an unreadable row — a capture of the
/// compass found `09:33:1` and `09:34:26` interleaved on the time axis, which
/// reads as neither.
///
/// Four, and no more: the chips an axis draws *itself* — the pointer's tag,
/// the crosshair's and the last price's, two rows while it carries progress.
/// The levels a drawing declares are chained in at the check rather than
/// copied in here, which is what keeps this from spilling onto the heap once
/// per frame on a chart with a few levels drawn on it.
pub(crate) type AxisClaims = SmallVec<[f32; 4]>;

/// How tall a tag's second line is at its full size, as `price_tag_claims` claims it.
pub(crate) fn second_line_height(painter: &egui::Painter) -> f32 {
    let palette = theme::get(painter.ctx());
    let font = egui::FontId::monospace(price_tag::SECOND_LINE_PX);
    let line = painter.layout_no_wrap("0".to_owned(), font, palette.text_muted());
    line.size().y
}

/// The pointer's time chip on the time strip: where it is centred and how
/// wide its text is, as [`time_tag`] laid it out this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct TimeChip {
    pub centre: f32,
    pub width: f32,
}

/// Whether a strip label `label` wide centred at `at` would share pixels with
/// the pointer's chip, when there is one. The one width both the day
/// separator and the time labels stand aside for: the chip's real text.
#[must_use]
pub(crate) fn time_claimed(chip: Option<TimeChip>, at: f32, label: f32) -> bool {
    chip.is_some_and(|chip| claimed(at, label, chip.width, [chip.centre]))
}

/// Where the pointer's time chip will sit on `strip`, how wide it is, and
/// its text: the full tag, or below a day the widest shorter true form that
/// fits a strip too narrow for it.
///
/// Asked by the strip before it writes its own labels and by the paint that
/// puts the chip there, so the coordinate an axis stands aside for and the one
/// a chip lands on are the same number by construction. `None` when there is
/// no bar under the pointer and therefore no chip.
#[must_use]
pub(crate) fn time_tag(
    painter: &egui::Painter,
    strip: egui::Rect,
    readout: &PointerReadout,
    tz: TzOffset,
) -> Option<(TimeChip, String)> {
    let palette = theme::axes(painter.ctx());
    let bar = readout.bar?;
    let full = crate::plot_area::fmt_pointer_time(bar.open_time_unix_ms, bar.interval_ms, tz);
    let font = palette.axis_font(chart::TIME_LABEL_FONT_PX);
    let galley = painter.layout_no_wrap(full.clone(), font, palette.text_primary());
    // Monospace: one character of the full tag measures every shorter form.
    let char_width = galley.size().x / full.chars().count().max(1) as f32;
    let room = strip.width() - 2.0 * TAG_PAD.x;
    let fits = |text: &str| text.chars().count() as f32 * char_width <= room;
    let text = match bar
        .interval_ms
        .and_then(crate::plot_area::calendar_label_sample)
    {
        Some(_) => full,
        None => quantick_chart::day_turn::pointer_time_fitting(bar.open_time_unix_ms, tz, fits),
    };
    let width = text.chars().count() as f32 * char_width;
    // Flush left where even the shortest form overflows; the paint clips it.
    let padded = width + 2.0 * TAG_PAD.x;
    let centre = chart::chip_centre(readout.position.x, padded, strip.left(), strip.right());
    Some((TimeChip { centre, width }, text))
}

pub(crate) use quantick_chart::pane_series::PointerBar;

/// What the axes have to say about the pointer's position.
///
/// The price is unconditional and the time is not, and that asymmetry is the
/// honest one rather than an omission: a height on the price axis *is* a
/// price, whatever is drawn at it, while a time belongs to a bar. Past the
/// newest bar there is none, and on a tick or volume chart no interval to add
/// either, so nothing is marked rather than a clock being extrapolated that
/// the tape never wrote.
///
/// Both halves describe the world and neither is gated on a switch: a mark
/// being off is not a statement about what is under the pointer, and the
/// control plane's cursor scope reads the same resolver. The switches decide
/// what is *painted*, in `PointerCompass::resolve`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PointerReadout {
    /// Where the pointer is, in screen pixels.
    pub position: egui::Pos2,
    /// The price at the pointer's height on this pane's price scale.
    pub price: f64,
    /// The bar under the pointer, when one is there.
    pub bar: Option<PointerBar>,
}

/// Resolve the pointer against the geometry a frame is painting.
///
/// `chart` is the candles' own area — which the live lane is part of, since it
/// shares the price axis, and the indicator band below is not, since it does
/// not. That is also as far as the pointer is reported: the pane learns where
/// the mouse is from its own canvas response, so a pointer over an indicator
/// pane is `None` here and gets no compass. Extending the time half down there
/// — the clock is genuinely shared by every pane — needs a pointer the pane
/// does not currently receive, and inventing a second source for it would put
/// a mark under an open menu.
///
/// A pointer outside `chart`, or absent entirely, answers `None`: that is what
/// makes both marks disappear when the mouse leaves rather than freezing where
/// it was last seen.
#[must_use]
pub(crate) fn readout(
    pointer: Option<egui::Pos2>,
    chart: egui::Rect,
    scale: &PriceScale,
    bar: Option<PointerBar>,
) -> Option<PointerReadout> {
    let position = pointer?;
    if !chart.contains(position) {
        return None;
    }
    Some(PointerReadout {
        position,
        price: scale.price_at(position.y),
        bar,
    })
}

/// Write one value onto the price axis at height `y`: a tick out of the chart
/// and a chip carrying `text`.
///
/// The one owner of where a price sits on this axis. The last-price chip, the
/// crosshair's tag and the pointer's own all come through here, so three marks
/// on one axis can never disagree about a pixel — they used to be three copies
/// of the same eight lines, each free to drift.
///
/// `axis_x` is the gutter's left edge, which is the chart's right edge
/// normally and the live strip's right edge while the strip sits between them.
/// A `second` line sits under the price, one point smaller, on the same chip —
/// the last price's tag carrying the forming bar's progress; the price stays
/// centred on `y` and the chip grows downward. A line too wide for the gutter
/// is set smaller until it fits.
pub(crate) fn paint_price_tag(
    painter: &egui::Painter,
    axis_x: f32,
    y: f32,
    text: String,
    second: Option<String>,
    fill: egui::Color32,
    ink: egui::Color32,
) {
    let palette = theme::axes(painter.ctx());
    let font = |size| palette.axis_font(size);
    let galley = painter.layout_no_wrap(text, font(chart::AXIS_LABEL_FONT_PX), ink);
    let under = second.map(|text| {
        let line = painter.layout_no_wrap(text.clone(), font(price_tag::SECOND_LINE_PX), ink);
        match price_tag::second_line_font_px(line.size().x) {
            size if size < price_tag::SECOND_LINE_PX => {
                painter.layout_no_wrap(text, font(size), ink)
            }
            _ => line,
        }
    });
    let text_pos = egui::pos2(axis_x + chart::AXIS_LABEL_GAP_PX, y - galley.size().y / 2.0);
    let under_size = under.as_ref().map_or(egui::Vec2::ZERO, |line| line.size());
    let size = egui::vec2(
        galley.size().x.max(under_size.x),
        galley.size().y + under_size.y,
    );
    let background = egui::Rect::from_min_size(text_pos - TAG_PAD, size + TAG_PAD * 2.0);
    if fill.a() > 0 {
        painter.rect_filled(background, egui::Rounding::same(TAG_ROUNDING), fill);
    }
    let under_pos = text_pos + egui::vec2(0.0, galley.size().y);
    painter.galley(text_pos, galley, ink);
    if let Some(line) = under {
        painter.galley(under_pos, line, ink);
    }
}

/// The pointer's price on the price axis: a tick out of the canvas and the
/// chip it leads to.
///
/// The tick is what makes the chip readable as *the pointer's* rather than as
/// one more axis label: it joins the height the mouse is at to the number
/// beside it, over the six pixels of gutter between them.
pub(crate) fn paint_price_mark(painter: &egui::Painter, axis_x: f32, readout: &PointerReadout) {
    let palette = theme::axes(painter.ctx());
    let y = readout.position.y;
    painter.line_segment(
        [egui::pos2(axis_x, y), egui::pos2(axis_x + TICK_PX, y)],
        egui::Stroke::new(1.0_f32, palette.text_primary()),
    );
    paint_price_tag(
        painter,
        axis_x,
        y,
        price_text(readout.price),
        None,
        palette.tag_bg(),
        palette.text_primary(),
    );
}

/// The time of the bar under the pointer, on the bottom time strip: a tick
/// down from the strip's top rule and the chip it leads to.
///
/// Nothing is drawn when no bar is under the pointer. `strip` is the history
/// segment of the time axis — the tape's segment past the lane divider reads
/// its own rolling window, not bar slots, so a bar time has no business being
/// written there.
///
/// The chip is clamped inside `strip` rather than hidden when it would
/// overhang, which is the opposite of the rule the periodic labels follow: a
/// label half in the gutter is noise nobody asked for, while this one is the
/// answer to a question the trader is asking right now by holding the mouse
/// where they are holding it.
pub(crate) fn paint_time_mark(
    painter: &egui::Painter,
    strip: egui::Rect,
    readout: &PointerReadout,
    tz: TzOffset,
) {
    let palette = theme::axes(painter.ctx());
    let Some((TimeChip { centre, .. }, text)) = time_tag(painter, strip, readout, tz) else {
        return;
    };
    // Never past the strip: on one narrower than the chip, the chip starts at
    // its left edge and is cut at its right.
    let painter = &painter.with_clip_rect(strip);
    painter.line_segment(
        [
            egui::pos2(centre, strip.top()),
            egui::pos2(centre, strip.top() + TICK_PX),
        ],
        egui::Stroke::new(1.0_f32, palette.text_primary()),
    );
    // Always the full instant, whatever density the strip's own labels are
    // written at. The strip drops seconds when it runs out of room for six
    // labels; there is only ever one of these, and a trader who points at a
    // bar to ask when it happened is asking to the second — or, on a day,
    // week or month chart, to the date. Shortened only when the strip is.
    let galley = painter.layout_no_wrap(
        text,
        palette.axis_font(chart::TIME_LABEL_FONT_PX),
        palette.text_primary(),
    );
    let size = galley.size();
    let text_pos = egui::pos2(centre - size.x / 2.0, strip.center().y - size.y / 2.0);
    let background = egui::Rect::from_min_size(text_pos - TAG_PAD, size + TAG_PAD * 2.0);
    painter.rect_filled(
        background,
        egui::Rounding::same(TAG_ROUNDING),
        palette.tag_bg(),
    );
    painter.galley(text_pos, galley, palette.text_primary());
}

/// What the pointer's compass will draw this frame, and where.
///
/// One decision, read twice: the axes consult it before labelling themselves
/// so they can leave the coordinate alone, and the paint pass draws exactly
/// what it says.
pub(crate) struct PointerCompass {
    pub readout: PointerReadout,
    /// The price half is drawn — its layer is on, the pointer is over the
    /// price band, and the crosshair is not already writing one.
    pub price: bool,
    /// The time half is drawn — its layer is on and a bar is under the
    /// pointer.
    pub time: bool,
}

impl PointerCompass {
    pub(crate) fn resolve(
        pointer: Option<egui::Pos2>,
        chart_rect: egui::Rect,
        scale: &PriceScale,
        bar: Option<PointerBar>,
        price_on: bool,
        time_on: bool,
        price_owned: bool,
    ) -> Option<Self> {
        if !price_on && !time_on {
            return None;
        }
        let readout = readout(pointer, chart_rect, scale, bar)?;
        Some(Self {
            price: price_on && !price_owned,
            time: time_on && readout.bar.is_some(),
            readout,
        })
    }
}

#[cfg(test)]
mod tests;
