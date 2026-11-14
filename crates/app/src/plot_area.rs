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

/// Room right of the price gutter, for labels wider than it. The canvas meets
/// its header, its rail and the pane bottom: a border there only eats room.
const PLOT_RIGHT_PAD_PX: f32 = 16.0;

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
#[cfg(test)]
pub fn plot_split(
    area: egui::Rect,
    live_strip_width: f32,
    pane_sizing: &[crate::indicators::PaneSizing],
) -> PlotAreas {
    plot_split_with_metrics(area, live_strip_width, pane_sizing, Default::default())
}

pub fn plot_split_with_metrics(
    area: egui::Rect,
    live_strip_width: f32,
    pane_sizing: &[crate::indicators::PaneSizing],
    metrics: quantick_appearance::AxisMetrics,
) -> PlotAreas {
    let plot = area.with_max_x(area.right() - PLOT_RIGHT_PAD_PX);
    let strip_width = live_strip_width.max(0.0);
    let gutter_x = (plot.right() - metrics.gutter).max(plot.left() + 20.0);
    let split_x = (gutter_x - strip_width).max(plot.left() + 20.0);
    let split_y = (plot.bottom() - metrics.strip).max(plot.top() + 20.0);
    let body = egui::Rect::from_min_max(plot.min, egui::pos2(split_x, split_y));
    let (chart, indicator_panes) =
        crate::indicators::split_panes_with_header(body, pane_sizing, metrics.indicator_header);
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
        capsules: [None; 2],
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
    /// The layer capsules floating over the chart, as last painted.
    pub capsules: [Option<egui::Rect>; 2],
    /// The gutter band beside each pane, in the same order: where that pane's
    /// value labels are drawn and where its own zoom gesture lives.
    pub pane_gutters: Vec<egui::Rect>,
    /// Present only while the strip is shown: between `chart` and `price_gutter`,
    /// or beside a tape on the trail, against the divider (`ChartPane::lay_out`).
    pub live_strip: Option<egui::Rect>,
    pub price_gutter: egui::Rect,
    pub time_strip: egui::Rect,
}

/// Format a UTC epoch-millisecond timestamp as `HH:MM:SS` in the display
/// timezone `tz`, for the time axis.
pub fn fmt_time(ms: i64, tz: TzOffset) -> String {
    crate::chart::time_label(ms, tz, crate::chart::TimeLabelFormat::Full)
}

/// The pointer's time tag for a bar opening at `open_ms`: below a day the
/// instant to the second with its date, `Fri 09 Oct '26 10:57:03`, and on a
/// calendar chart the date of its UTC bucket,
/// as the axis labels name it — `Wed 07 Oct 2026` for a day or a week,
/// `Oct 2026` for a month or several. A clock time alone there would read
/// the same `21:00:00` on every bar west of UTC.
#[must_use]
pub fn fmt_pointer_time(open_ms: i64, interval_ms: Option<i64>, tz: TzOffset) -> String {
    let Some(interval) = interval_ms.filter(|interval| calendar_label_sample(*interval).is_some())
    else {
        return quantick_chart::day_turn::pointer_time_label(open_ms, tz);
    };
    let day = quantick_civil::CivilDate::from_ms(
        quantick_engine::time_bucket::time_bucket_start(open_ms, interval),
        TzOffset::new(0),
    );
    if quantick_engine::time_bucket::calendar_months(interval).is_some() {
        let (year, _, _) = day.ymd();
        let short = day.short();
        format!("{} {year:04}", short.get(3..).unwrap_or_default())
    } else {
        day.long()
    }
}

pub use quantick_chart::time_marks::calendar_label_sample;

#[cfg(test)]
mod tests;
