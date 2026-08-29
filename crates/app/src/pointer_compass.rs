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
/// Eight, because the claims on one axis are the pointer's tag, the last-price
/// chip and one per level a drawing declares; past that the axis is spoken for
/// several times over and one more claim changes nothing. Sized so the gather
/// allocates nothing per frame.
pub(crate) type AxisClaims = SmallVec<[f32; 8]>;

/// Whether a gridline label centred at `at` would share pixels with something
/// already written on this axis, and should therefore stand aside.
///
/// `extent` is the label's own size along the axis — its height on the price
/// axis, its width on the time strip. Two boxes of that extent touch once
/// their centres are within one extent, and a tag is a label plus its padding,
/// so the separation is measured off the font rather than picked. [`TAG_PAD`]'s
/// wider component is used for both axes: it errs by two pixels toward hiding
/// a label that would only just clear, which is the right direction — the
/// round number is the part nobody needs.
#[must_use]
pub(crate) fn claimed(claims: &AxisClaims, at: f32, extent: f32) -> bool {
    let clearance = extent + TAG_PAD.x;
    claims.iter().any(|claim| (claim - at).abs() < clearance)
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
/// Both halves are optional, and for different reasons — which is the honest
/// shape rather than an omission:
///
/// * the **price** is absent where the pointer is not over the price band. An
///   indicator pane has units of its own and a gutter of its own; writing the
///   candles' price for a pointer over a CVD curve would put a number on the
///   axis that nothing on screen is at.
/// * the **time** is absent where no bar is under the pointer. Past the newest
///   bar there is none, and on a tick or volume chart no interval to add
///   either, so nothing is marked rather than a clock being extrapolated that
///   the tape never wrote.
///
/// The two are read against different rects for the same reason: the time axis
/// runs under every pane and the price axis belongs to the candles alone.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PointerReadout {
    /// Where the pointer is, in screen pixels.
    pub position: egui::Pos2,
    /// The price at the pointer's height, when it is over the price band.
    pub price: Option<f64>,
    /// The bar under the pointer, when one is there.
    pub bar: Option<PointerBar>,
}

/// Resolve the pointer against the geometry a frame is painting.
///
/// `panes` is everything the shared time axis runs under — the candles and the
/// indicator band below them — and `price_band` is the candles alone. A
/// pointer outside `panes` is not over the chart at all and answers `None`,
/// which is what makes both marks disappear when the mouse leaves rather than
/// freezing where it was last seen.
#[must_use]
pub(crate) fn readout(
    pointer: Option<egui::Pos2>,
    panes: egui::Rect,
    price_band: egui::Rect,
    scale: &PriceScale,
    bar: Option<PointerBar>,
) -> Option<PointerReadout> {
    let position = pointer?;
    if !panes.contains(position) {
        return None;
    }
    Some(PointerReadout {
        position,
        price: price_band
            .contains(position)
            .then(|| scale.price_at(position.y)),
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
    let Some(price) = readout.price else {
        return;
    };
    let y = readout.position.y;
    painter.line_segment(
        [egui::pos2(axis_x, y), egui::pos2(axis_x + TICK_PX, y)],
        egui::Stroke::new(1.0_f32, theme::TEXT_PRIMARY),
    );
    paint_price_tag(
        painter,
        axis_x,
        y,
        format!("{price:.2}"),
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
    let Some(bar) = readout.bar else {
        return;
    };
    let x = readout.position.x.clamp(strip.left(), strip.right());
    painter.line_segment(
        [
            egui::pos2(x, strip.top()),
            egui::pos2(x, strip.top() + TICK_PX),
        ],
        egui::Stroke::new(1.0_f32, theme::TEXT_PRIMARY),
    );
    // Always the full instant, whatever density the strip's own labels are
    // written at. The strip drops seconds when it runs out of room for six
    // labels; there is only ever one of these, and a trader who points at a
    // bar to ask when it happened is asking to the second.
    let text = crate::app::fmt_time_as(bar.open_time_unix_ms, tz, TimeLabelFormat::Full);
    let galley = painter.layout_no_wrap(
        text,
        egui::FontId::monospace(chart::TIME_LABEL_FONT_PX),
        theme::TEXT_PRIMARY,
    );
    let size = galley.size();
    let left = (x - size.x / 2.0).clamp(
        strip.left() + TAG_PAD.x,
        (strip.right() - size.x - TAG_PAD.x).max(strip.left() + TAG_PAD.x),
    );
    let text_pos = egui::pos2(left, strip.center().y - size.y / 2.0);
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

    /// The candles, and the indicator band under them. The shared time axis
    /// runs under both; the price axis belongs to the candles alone.
    const PANES: egui::Rect = egui::Rect {
        min: egui::pos2(0.0, 0.0),
        max: egui::pos2(400.0, 300.0),
    };
    const PRICE_BAND: egui::Rect = egui::Rect {
        min: egui::pos2(0.0, 0.0),
        max: egui::pos2(400.0, 200.0),
    };

    fn scale() -> PriceScale {
        PriceScale::from_range(100.0, 200.0, PRICE_BAND.top(), PRICE_BAND.bottom())
    }

    fn read(at: egui::Pos2, bar: Option<PointerBar>) -> Option<PointerReadout> {
        readout(Some(at), PANES, PRICE_BAND, &scale(), bar)
    }

    /// The rule the captures found: a chip and a gridline label on the same
    /// row read as neither. The round number stands aside — it is the part
    /// nobody needs, and the chip is the answer somebody asked for.
    #[test]
    fn a_label_under_a_chip_stands_aside() {
        let mut claims = AxisClaims::new();
        claims.push(400.0);
        let extent = 14.0_f32;
        assert!(claimed(&claims, 400.0, extent), "dead on the chip");
        assert!(
            claimed(&claims, 408.0, extent),
            "half a label away, touching"
        );
        assert!(
            !claimed(&claims, 430.0, extent),
            "two labels clear, so the round number is worth writing"
        );
    }

    /// No claims, nothing hidden: an axis with no chip on it labels every
    /// gridline exactly as it always did.
    #[test]
    fn an_axis_nothing_claims_hides_nothing() {
        let claims = AxisClaims::new();
        for at in [0.0_f32, 123.0, 999.0] {
            assert!(!claimed(&claims, at, 14.0));
        }
    }

    /// Over the candles, the height is a price and the axis can say which.    /// Over the candles, the height is a price and the axis can say which.
    #[test]
    fn the_pointer_over_the_candles_reads_a_price() {
        let hit = read(egui::pos2(10.0, 100.0), None).expect("over the chart");
        let price = hit.price.expect("the candles' own band");
        assert!(
            (price - 150.0).abs() < 1e-9,
            "half way down a 100..200 scale is 150, got {price}"
        );
    }

    /// The marks follow the mouse and stop existing when it leaves — the whole
    /// difference between a compass and a stale annotation.
    #[test]
    fn a_pointer_off_the_chart_reads_nothing() {
        assert!(
            readout(None, PANES, PRICE_BAND, &scale(), None).is_none(),
            "no pointer over this pane at all"
        );
        assert!(
            read(egui::pos2(500.0, 100.0), None).is_none(),
            "out in the gutter, past the canvas"
        );
        assert!(
            read(egui::pos2(200.0, 400.0), None).is_none(),
            "below the panes, on the time strip itself"
        );
    }

    /// An indicator pane has units of its own and a gutter of its own, so the
    /// price axis says nothing for a pointer over it — but the time axis runs
    /// under every pane, so the clock still answers.
    #[test]
    fn an_indicator_pane_answers_the_clock_and_not_the_price() {
        let bar = PointerBar {
            slot: 7,
            open_time_unix_ms: 1_700_000_000_000,
        };
        let hit = read(egui::pos2(200.0, 250.0), Some(bar)).expect("still over the chart");
        assert_eq!(
            hit.price, None,
            "the candles' price would be a number nothing on screen is at"
        );
        assert_eq!(hit.bar, Some(bar), "and the shared time axis still answers");
    }

    /// The time half is absent where no bar is, and the readout says so by
    /// carrying `None` rather than a number nobody can stand behind.
    #[test]
    fn empty_canvas_past_the_newest_bar_reads_a_price_but_no_time() {
        let hit = read(egui::pos2(390.0, 50.0), None).expect("over the chart");
        assert!(hit.price.is_some(), "the height is still a price");
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
        assert!(hit.price.is_some());
    }

    /// The scale is the one the axis labels are drawn from, upside down
    /// included: a compass that reported the upright price on an inverted
    /// chart would be worse than no compass.
    #[test]
    fn the_reading_follows_the_chart_upside_down() {
        let inverted = scale().with_inverted(true);
        let hit = readout(
            Some(egui::pos2(10.0, 0.0)),
            PANES,
            PRICE_BAND,
            &inverted,
            None,
        )
        .expect("over the chart");
        let price = hit.price.expect("the candles' own band");
        assert!(
            (price - 100.0).abs() < 1e-9,
            "the top of an inverted 100..200 scale is the low, got {price}"
        );
    }
}
