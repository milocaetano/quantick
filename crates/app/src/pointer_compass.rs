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
//! axis tags, and [`ChartPane::draw_pointer_compass`] hands the price half over
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
//! [`ChartPane::draw_pointer_compass`]: crate::pane::ChartPane::draw_pointer_compass

use eframe::egui;
use smallvec::SmallVec;

use crate::chart::{self, PriceScale, TimeLabelFormat};
use crate::theme;
use crate::timezone::TzOffset;

/// How far the tick leading into a tag reaches out of the chart, in pixels.
///
/// Short on purpose: it exists to join the pointer's height to the chip
/// beside it, so the eye does not have to measure. Longer would draw a second
/// gridline the chart never asked for.
const TICK_PX: f32 = 5.0;

/// Padding around a tag's text, in pixels — the last-price chip's own, so the
/// three marks that can share this axis are the same size.
const TAG_PAD: egui::Vec2 = egui::vec2(3.0, 1.0);

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
/// Two, and no more: this holds the chips an axis draws *itself* — the
/// pointer's tag and the last-price chip. The levels a drawing declares are
/// already gathered for painting, so they are chained in at the check rather
/// than copied in here, which is what keeps this from spilling onto the heap
/// once per frame on a chart with a few levels drawn on it.
pub(crate) type AxisClaims = SmallVec<[f32; 2]>;

/// Whether a gridline label centred at `at` would share pixels with a chip
/// already written on this axis, and should therefore stand aside.
///
/// `label` and `chip` are the two extents along the axis — heights on the
/// price axis, widths on the time strip. They are asked for separately
/// because they are not the same: the time strip drops its own labels to
/// `HH:MM` when it runs out of room while the pointer's chip stays `HH:MM:SS`,
/// so a rule that assumed one size left a 30 px label overlapping a 54 px chip
/// by a dozen pixels — the very interleaving this exists to remove.
///
/// Two boxes touch once their centres are within half of each extent, plus
/// the padding a chip wears over its text.
///
/// The claims arrive as an iterator so a caller can chain the axis's own chips
/// onto the levels it has already gathered, instead of building a third list
/// every frame. Each is the coordinate the chip will *actually* be drawn at —
/// not the pointer's, which the time strip clamps away from near its edges.
#[must_use]
pub(crate) fn claimed(
    at: f32,
    label: f32,
    chip: f32,
    claims: impl IntoIterator<Item = f32>,
) -> bool {
    let clearance = (label + chip) / 2.0 + TAG_PAD.x;
    claims
        .into_iter()
        .any(|claim| (claim - at).abs() < clearance)
}

/// Where the pointer's time chip will sit on `strip`, and how wide it is.
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
) -> Option<(f32, f32)> {
    readout.bar?;
    // Monospace, so one measurement of the format answers for every instant
    // written in it — the rule the strip's own labels are measured by.
    let width = painter
        .layout_no_wrap(
            TimeLabelFormat::Full.sample().to_owned(),
            egui::FontId::monospace(chart::TIME_LABEL_FONT_PX),
            theme::TEXT_PRIMARY,
        )
        .size()
        .x;
    let half = width / 2.0 + TAG_PAD.x;
    let centre = readout.position.x.clamp(
        strip.left() + half,
        (strip.right() - half).max(strip.left() + half),
    );
    Some((centre, width))
}

/// The bar under the pointer, and the instant it opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PointerBar {
    /// The slot, in the pane's composed slot space (venue prefix included).
    pub slot: usize,
    /// When that bar opened, in Unix milliseconds.
    pub open_time_unix_ms: i64,
}

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
/// what is *painted*, in `ChartPane::pointer_compass`.
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
pub(crate) fn paint_price_tag(
    painter: &egui::Painter,
    axis_x: f32,
    y: f32,
    text: String,
    fill: egui::Color32,
    ink: egui::Color32,
) {
    let galley = painter.layout_no_wrap(
        text,
        egui::FontId::monospace(chart::AXIS_LABEL_FONT_PX),
        ink,
    );
    let text_pos = egui::pos2(axis_x + chart::AXIS_LABEL_GAP_PX, y - galley.size().y / 2.0);
    let background = egui::Rect::from_min_size(text_pos - TAG_PAD, galley.size() + TAG_PAD * 2.0);
    painter.rect_filled(background, egui::Rounding::same(TAG_ROUNDING), fill);
    painter.galley(text_pos, galley, ink);
}

/// The pointer's price on the price axis: a tick out of the canvas and the
/// chip it leads to.
///
/// The tick is what makes the chip readable as *the pointer's* rather than as
/// one more axis label: it joins the height the mouse is at to the number
/// beside it, over the six pixels of gutter between them.
pub(crate) fn paint_price_mark(painter: &egui::Painter, axis_x: f32, readout: &PointerReadout) {
    let y = readout.position.y;
    painter.line_segment(
        [egui::pos2(axis_x, y), egui::pos2(axis_x + TICK_PX, y)],
        egui::Stroke::new(1.0_f32, theme::TEXT_PRIMARY),
    );
    paint_price_tag(
        painter,
        axis_x,
        y,
        price_text(readout.price),
        theme::TAG_BG,
        theme::TEXT_PRIMARY,
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
    let (Some(bar), Some((centre, _))) = (readout.bar, time_tag(painter, strip, readout)) else {
        return;
    };
    painter.line_segment(
        [
            egui::pos2(centre, strip.top()),
            egui::pos2(centre, strip.top() + TICK_PX),
        ],
        egui::Stroke::new(1.0_f32, theme::TEXT_PRIMARY),
    );
    // Always the full instant, whatever density the strip's own labels are
    // written at. The strip drops seconds when it runs out of room for six
    // labels; there is only ever one of these, and a trader who points at a
    // bar to ask when it happened is asking to the second.
    let text = crate::plot_area::fmt_time_as(bar.open_time_unix_ms, tz, TimeLabelFormat::Full);
    let galley = painter.layout_no_wrap(
        text,
        egui::FontId::monospace(chart::TIME_LABEL_FONT_PX),
        theme::TEXT_PRIMARY,
    );
    let size = galley.size();
    let text_pos = egui::pos2(centre - size.x / 2.0, strip.center().y - size.y / 2.0);
    let background = egui::Rect::from_min_size(text_pos - TAG_PAD, size + TAG_PAD * 2.0);
    painter.rect_filled(
        background,
        egui::Rounding::same(TAG_ROUNDING),
        theme::TAG_BG,
    );
    painter.galley(text_pos, galley, theme::TEXT_PRIMARY);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The candles' own area — the live lane included, since it shares the
    /// price axis, and the indicator band below excluded, since it does not.
    const CHART: egui::Rect = egui::Rect {
        min: egui::pos2(0.0, 0.0),
        max: egui::pos2(400.0, 200.0),
    };

    fn scale() -> PriceScale {
        PriceScale::from_range(100.0, 200.0, CHART.top(), CHART.bottom())
    }

    fn read(at: egui::Pos2, bar: Option<PointerBar>) -> Option<PointerReadout> {
        readout(Some(at), CHART, &scale(), bar)
    }

    /// The rule the captures found: a chip and a gridline label on the same
    /// row read as neither. The round number stands aside — it is the part
    /// nobody needs, and the chip is the answer somebody asked for.
    #[test]
    fn a_label_under_a_chip_stands_aside() {
        let label = 14.0_f32;
        let chip = [400.0_f32];
        assert!(claimed(400.0, label, label, chip), "dead on the chip");
        assert!(
            claimed(408.0, label, label, chip),
            "half a label away, touching"
        );
        assert!(
            !claimed(430.0, label, label, chip),
            "two labels clear, so the round number is worth writing"
        );
        // The axis's own chips and the levels a drawing declared are one
        // question asked of two lists, chained rather than copied together.
        let levels = [520.0_f32, 700.0];
        assert!(claimed(521.0, label, label, chip.into_iter().chain(levels)));
        assert!(!claimed(
            600.0,
            label,
            label,
            chip.into_iter().chain(levels)
        ));
    }

    /// The time strip thins its own labels to `HH:MM` when it runs out of
    /// room while the pointer's chip stays `HH:MM:SS`, so the two extents are
    /// asked for separately. A rule that took the label's size for both let a
    /// 30 px label sit a dozen pixels inside a 54 px chip — the interleaving
    /// this exists to remove, back again on exactly the narrow strip that
    /// needed it most.
    #[test]
    fn a_narrow_strips_short_label_still_clears_the_full_width_chip() {
        let (short, full) = (30.0_f32, 54.0_f32);
        let chip = [400.0_f32];
        assert!(
            claimed(358.0, short, full, chip),
            "42 px away, and the two boxes overlap by a dozen"
        );
        assert!(
            !claimed(358.0, short, short, chip),
            "which the old rule, measuring the label twice, allowed through"
        );
        assert!(claimed(400.0, short, full, chip));
        assert!(!claimed(340.0, short, full, chip), "clear of the chip");
    }

    /// No claims, nothing hidden: an axis with no chip on it labels every
    /// gridline exactly as it always did.
    #[test]
    fn an_axis_nothing_claims_hides_nothing() {
        for at in [0.0_f32, 123.0, 999.0] {
            assert!(!claimed(at, 14.0, 14.0, AxisClaims::new()));
        }
    }

    /// Over the candles, the height is a price and the axis can say which.
    #[test]
    fn the_pointer_over_the_candles_reads_a_price() {
        let hit = read(egui::pos2(10.0, 100.0), None).expect("over the chart");
        assert!(
            (hit.price - 150.0).abs() < 1e-9,
            "half way down a 100..200 scale is 150, got {}",
            hit.price
        );
    }

    /// The marks follow the mouse and stop existing when it leaves — the whole
    /// difference between a compass and a stale annotation.
    #[test]
    fn a_pointer_off_the_chart_reads_nothing() {
        assert!(
            readout(None, CHART, &scale(), None).is_none(),
            "no pointer over this pane at all"
        );
        assert!(
            read(egui::pos2(500.0, 100.0), None).is_none(),
            "out in the gutter, past the canvas"
        );
        assert!(
            read(egui::pos2(200.0, 400.0), None).is_none(),
            "below the canvas, on the time strip itself"
        );
    }

    /// The time half is absent where no bar is, and the readout says so by
    /// carrying `None` rather than a number nobody can stand behind.
    #[test]
    fn empty_canvas_past_the_newest_bar_reads_a_price_but_no_time() {
        let hit = read(egui::pos2(390.0, 50.0), None).expect("over the chart");
        assert!(hit.price.is_finite(), "the height is still a price");
        assert_eq!(hit.bar, None, "no bar under the pointer, so no instant");
    }

    /// And where a bar *is* under the pointer, both halves are there.
    #[test]
    fn a_bar_under_the_pointer_reads_its_opening_instant() {
        let bar = PointerBar {
            slot: 7,
            open_time_unix_ms: 1_700_000_000_000,
        };
        let hit = read(egui::pos2(120.0, 50.0), Some(bar)).expect("over the chart");
        assert_eq!(hit.bar, Some(bar));
    }

    /// The scale is the one the axis labels are drawn from, upside down
    /// included: a compass that reported the upright price on an inverted
    /// chart would be worse than no compass.
    #[test]
    fn the_reading_follows_the_chart_upside_down() {
        let inverted = scale().with_inverted(true);
        let hit =
            readout(Some(egui::pos2(10.0, 0.0)), CHART, &inverted, None).expect("over the chart");
        assert!(
            (hit.price - 100.0).abs() < 1e-9,
            "the top of an inverted 100..200 scale is the low, got {}",
            hit.price
        );
    }
}
