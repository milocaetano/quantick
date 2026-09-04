//! Where the plot's pixels are, and what the time axis calls them.
//!
//! The geometry two very large modules have to agree on, kept in neither of
//! them. [`crate::app`] composes the window, [`crate::pane`] draws inside it
//! and [`crate::bands`] measures the result; all three need the same rects and
//! the same axis labels, so the rects and the labels live here, below all
//! three. That is the whole reason this module exists: with `plot_split` and
//! [`PlotAreas`] declared in `app`, `pane` and `bands` had to import the
//! composition root that imports them, and the four modules could only ever
//! move together.
//!
//! Nothing here reads application state. Given a rect it returns rects; given
//! an instant it returns a string. It depends on [`crate::indicators`] for the
//! pane band it carves and on [`crate::chart`] and [`crate::timezone`] for the
//! label format, and on nothing above itself.

use eframe::egui;

use crate::timezone::TzOffset;

/// Width of the right-hand price-axis gutter, in pixels (§5 zone 9).
const AXIS_GUTTER: f32 = 64.0;
/// Height of the bottom time-axis strip, in pixels (§5 zone 6).
const TIME_STRIP: f32 = 24.0;

/// Split the padded plot area into the candle chart, the indicator panes, the
/// optional live strip, the right price gutter and the bottom time strip, so
/// the input handler and the renderer agree on the boundaries.
/// `live_strip_width` of zero means the strip is off and the chart runs
/// straight into the gutter, exactly as it did before the strip existed.
///
/// `pane_sizing` carries one entry per *visible* pane indicator, top to
/// bottom: the band they claim is carved here, once, rather than by each
/// caller — a chart rect that two call sites disagree about is two price
/// scales for the same pixels. Sizing rather than a count, because how tall a
/// pane is and whether it has room to be drawn at all is the same decision.
pub fn plot_split(
    area: egui::Rect,
    live_strip_width: f32,
    pane_sizing: &[crate::indicators::PaneSizing],
) -> PlotAreas {
    let plot = area.shrink(16.0);
    let strip_width = live_strip_width.max(0.0);
    let gutter_x = (plot.right() - AXIS_GUTTER).max(plot.left() + 20.0);
    let split_x = (gutter_x - strip_width).max(plot.left() + 20.0);
    let split_y = (plot.bottom() - TIME_STRIP).max(plot.top() + 20.0);
    let body = egui::Rect::from_min_max(plot.min, egui::pos2(split_x, split_y));
    let (chart, indicator_panes) = crate::indicators::split_panes(body, pane_sizing);
    // The gutter is banded exactly like the body it labels: the candles' price
    // scale owns the height of the candles and not a pixel more, so a drag
    // over a pane's numbers can only ever move that pane.
    let band = |top: f32, bottom: f32| {
        egui::Rect::from_min_max(egui::pos2(gutter_x, top), egui::pos2(plot.right(), bottom))
    };
    let pane_gutters = indicator_panes
        .iter()
        .map(|pane| band(pane.rect.top(), pane.rect.bottom()))
        .collect();
    PlotAreas {
        chart,
        indicator_panes,
        pane_gutters,
        live_strip: (strip_width > 0.0).then(|| {
            egui::Rect::from_min_max(
                egui::pos2(split_x, plot.top()),
                egui::pos2(gutter_x, split_y),
            )
        }),
        price_gutter: band(plot.top(), chart.bottom()),
        time_strip: egui::Rect::from_min_max(
            egui::pos2(plot.left(), split_y),
            egui::pos2(split_x, plot.bottom()),
        ),
    }
}

/// Whether a pointer at `x` is on the lane divider's own resize handle.
///
/// The one strip of the tape a chart gesture may not have: the resize drag and
/// the pan must never both fire on the same pixel. Everything else in the band
/// is the candles' — the tape is pinned to the live edge and does not pan, so a
/// drag across it has no second meaning to protect, and reserving a third of
/// the canvas to guard a ten-pixel handle is a dead zone rather than a guard.
/// Without a lane there is no handle, and every pixel is the candles'.
#[must_use]
pub fn gesture_hits_lane_divider(divider_x: Option<f32>, x: f32, half_width: f32) -> bool {
    divider_x.is_some_and(|divider| (x - divider).abs() <= half_width)
}

/// Split the bottom time strip at the lane's divider: the candles' own time
/// axis on the left, the lane's on the right.
///
/// Each pane zooms from the strip under it, which is the only place a zoom
/// gesture can say *which* time axis it means. Without a divider the whole
/// strip belongs to the candles, exactly as it did before the lane had a zoom.
pub fn split_time_strip(
    strip: egui::Rect,
    divider_x: Option<f32>,
) -> (egui::Rect, Option<egui::Rect>) {
    let Some(divider) = divider_x.filter(|x| strip.x_range().contains(*x)) else {
        return (strip, None);
    };
    (
        egui::Rect::from_min_max(strip.min, egui::pos2(divider, strip.bottom())),
        Some(egui::Rect::from_min_max(
            egui::pos2(divider, strip.top()),
            strip.max,
        )),
    )
}

/// The interactive regions of the plot, plus the optional live strip.
pub struct PlotAreas {
    /// The candle body, with the indicator pane band already taken out of it.
    /// Every consumer — renderer and input handler alike — reads the chart
    /// rect from here, which is what keeps the price scale a drawing is
    /// placed against identical to the one it is hit-tested against.
    pub chart: egui::Rect,
    /// Stacked indicator panes below the candles, top to bottom. Empty when
    /// no pane indicator is visible.
    pub indicator_panes: Vec<crate::indicators::PaneSlot>,
    /// The gutter band beside each pane, in the same order: where that pane's
    /// value labels are drawn and where its own zoom gesture lives.
    pub pane_gutters: Vec<egui::Rect>,
    /// Present only while the strip is shown; sits between `chart` and
    /// `price_gutter` and is not an input region.
    pub live_strip: Option<egui::Rect>,
    pub price_gutter: egui::Rect,
    pub time_strip: egui::Rect,
}

/// Format a UTC epoch-millisecond timestamp as `HH:MM:SS` in the display
/// timezone `tz`, for the time axis.
pub fn fmt_time(ms: i64, tz: TzOffset) -> String {
    fmt_time_as(ms, tz, crate::chart::TimeLabelFormat::Full)
}

/// The same instant written in a chosen [`TimeLabelFormat`] — what the time
/// axis calls when the strip is too narrow for the full form.
pub fn fmt_time_as(ms: i64, tz: TzOffset, format: crate::chart::TimeLabelFormat) -> String {
    let local = ms.saturating_add(tz.offset_ms());
    let secs = local.div_euclid(1000).rem_euclid(86_400);
    format.write(secs / 3600, (secs % 3600) / 60, secs % 60)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timezone::TzOffset;

    #[test]
    fn the_live_strip_carves_between_chart_and_gutter_only_when_shown() {
        let area = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1000.0, 600.0));

        let off = plot_split(area, 0.0, &[crate::indicators::PaneSizing::Auto; 0]);
        assert!(off.live_strip.is_none());
        assert_eq!(off.chart.right(), off.price_gutter.left());

        let on = plot_split(
            area,
            crate::live_strip::LIVE_STRIP_WIDTH_PX,
            &[crate::indicators::PaneSizing::Auto; 0],
        );
        let strip = on.live_strip.expect("strip rect");
        assert_eq!(on.chart.right(), strip.left());
        assert_eq!(strip.right(), on.price_gutter.left());
        assert_eq!(strip.width(), crate::live_strip::LIVE_STRIP_WIDTH_PX);
        // The strip pays with the chart's pixels: the gutter stays put, and
        // the time axis keeps spanning exactly the chart body.
        assert_eq!(on.price_gutter, off.price_gutter);
        assert_eq!(
            on.chart.width(),
            off.chart.width() - crate::live_strip::LIVE_STRIP_WIDTH_PX
        );
        assert_eq!(on.time_strip.right(), on.chart.right());
    }

    /// The pane band is carved once, inside `plot_split`, so the rect the
    /// renderer scales prices to is the rect the input handler hit-tests
    /// against. When the two disagreed, a drawing was placed where you
    /// clicked and then selected somewhere else — by 20% of the chart height
    /// per visible pane.
    #[test]
    fn the_pane_band_comes_out_of_every_callers_chart_rect() {
        let area = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1000.0, 600.0));

        let none = plot_split(area, 0.0, &[crate::indicators::PaneSizing::Auto; 0]);
        assert!(none.indicator_panes.is_empty());

        let one = plot_split(area, 0.0, &[crate::indicators::PaneSizing::Auto; 1]);
        let pane = *one
            .indicator_panes
            .first()
            .expect("one visible pane claims one rect");
        assert!(
            one.chart.height() < none.chart.height(),
            "the band is paid for out of the candles' pixels"
        );
        assert_eq!(one.chart.bottom(), pane.rect.top(), "no gap, no overlap");
        assert_eq!(pane.rect.bottom(), none.chart.bottom());
        assert_eq!(one.chart.width(), none.chart.width());
        // The axes keep their column; the time strip is untouched.
        assert_eq!(one.price_gutter.x_range(), none.price_gutter.x_range());
        assert_eq!(one.time_strip, none.time_strip);

        let three = plot_split(area, 0.0, &[crate::indicators::PaneSizing::Auto; 3]);
        assert_eq!(three.indicator_panes.len(), 3);
        assert!(three.chart.height() < one.chart.height());
    }

    /// The gutter is banded like the body it labels. Before it was, the whole
    /// column belonged to the candles: dragging the numbers beside a CVD pane
    /// stretched the *price* scale, and the pane — which had no axis at all —
    /// did not move.
    #[test]
    fn every_pane_owns_the_gutter_band_beside_it() {
        let area = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1000.0, 600.0));

        let none = plot_split(area, 0.0, &[crate::indicators::PaneSizing::Auto; 0]);
        assert!(none.pane_gutters.is_empty());
        assert_eq!(
            none.price_gutter.bottom(),
            none.chart.bottom(),
            "with no pane the gutter is the candles', top to bottom"
        );

        let two = plot_split(area, 0.0, &[crate::indicators::PaneSizing::Auto; 2]);
        assert_eq!(two.pane_gutters.len(), two.indicator_panes.len());
        assert_eq!(
            two.price_gutter.bottom(),
            two.chart.bottom(),
            "the candles' scale stops where the candles do"
        );
        for (pane, gutter) in two.indicator_panes.iter().zip(&two.pane_gutters) {
            assert_eq!(
                gutter.y_range(),
                pane.rect.y_range(),
                "band beside its pane"
            );
            assert_eq!(gutter.x_range(), two.price_gutter.x_range(), "one column");
        }
        // No pixel answers to two scales: the bands tile the gutter exactly.
        assert_eq!(two.price_gutter.bottom(), two.pane_gutters[0].top());
        assert_eq!(two.pane_gutters[0].bottom(), two.pane_gutters[1].top());

        // The strip pays out of the candles, not the gutter: the pane bands
        // keep the same column when the tape is shown.
        let with_strip = plot_split(
            area,
            crate::live_strip::LIVE_STRIP_WIDTH_PX,
            &[crate::indicators::PaneSizing::Auto; 2],
        );
        assert_eq!(with_strip.pane_gutters, two.pane_gutters);
    }

    /// Each pane zooms from the strip under it, and the split is exactly the
    /// divider — so a drag can never mean both time axes at once.
    #[test]
    fn the_time_strip_splits_at_the_lane_divider() {
        let strip = egui::Rect::from_min_max(egui::pos2(0.0, 580.0), egui::pos2(1000.0, 600.0));

        let (history, lane) = split_time_strip(strip, Some(700.0));
        let lane = lane.expect("the lane owns the strip under it");
        assert_eq!(history.left(), strip.left());
        assert_eq!(history.right(), 700.0);
        assert_eq!(lane.left(), 700.0);
        assert_eq!(lane.right(), strip.right());

        // Without a lane the candles keep the whole strip, exactly as before.
        assert_eq!(split_time_strip(strip, None), (strip, None));
        // A divider off the strip is not a split either.
        assert_eq!(split_time_strip(strip, Some(-5.0)), (strip, None));
    }

    /// The divider is the tape's, and nothing else in the band is.
    ///
    /// The predicate this replaced claimed the *whole* lane, so a third of the
    /// canvas took a press and did nothing with it: the trader pulled and the
    /// chart did not move, with nothing on screen saying why. The wheel had
    /// already been handed back to the candles for exactly that reason; this is
    /// the drag catching up.
    #[test]
    fn only_the_divider_handle_is_off_limits_to_the_pan() {
        let half = 5.0;
        assert!(gesture_hits_lane_divider(Some(700.0), 700.0, half));
        assert!(gesture_hits_lane_divider(Some(700.0), 695.0, half));
        assert!(gesture_hits_lane_divider(Some(700.0), 705.0, half));
        // The band beyond the handle belongs to the candles again.
        assert!(!gesture_hits_lane_divider(Some(700.0), 706.0, half));
        assert!(!gesture_hits_lane_divider(Some(700.0), 1_200.0, half));
        assert!(!gesture_hits_lane_divider(Some(700.0), 694.0, half));
        // No lane, no handle.
        assert!(!gesture_hits_lane_divider(None, 700.0, half));
    }

    #[test]
    fn fmt_time_in_utc() {
        // Epoch: 1970-01-01 00:00:00 UTC, then +1h 2m 3s.
        assert_eq!(fmt_time(0, TzOffset::new(0)), "00:00:00");
        assert_eq!(fmt_time(3_723_000, TzOffset::new(0)), "01:02:03");
    }

    #[test]
    fn fmt_time_applies_the_offset() {
        // UTC midnight shown in UTC−03:00 is 21:00 of the previous day.
        assert_eq!(fmt_time(0, TzOffset::new(-180)), "21:00:00");
        // UTC midnight in UTC+05:30 is 05:30.
        assert_eq!(fmt_time(0, TzOffset::new(330)), "05:30:00");
    }
}
