//! The values [`ChartPane::draw_chart`](super::ChartPane::draw_chart)
//! resolves before any layer goes down, each computed once by a named stage:
//! [`FrameStart`] (what the opening writes decided), [`FrameLayout`] (the
//! rects, the live lane and the visible slot range), the [`DrawFrame`] that
//! [`FrameLayout::resolve`] builds from the series and the price scale, and
//! [`CandleDress`] (how the candles are laid out for the footprint style
//! that will actually paint).
//!
//! Every one is a plain value or a borrow the frame already holds: nothing
//! here allocates, and nothing reads the pane past the fields it is handed.

use eframe::egui;
use quantick_engine::{Bar, BarFootprint};

use crate::chart;
use crate::orderflow_view::LiveLane;
use crate::plot_area::PlotAreas;
use crate::price_view::PriceView;
use crate::style::CandleStyle;

use super::PaneChrome;
use super::draw_frame::DrawFrame;
use super::footprint::PaneFootprint;

/// How often the forming bar's footprint ladder is re-snapshotted for
/// drawing, in seconds. ~10 Hz: the eye reads the pattern, not the ticking
/// digits, and a layout that repaints per print reflows under the pointer.
const LIVE_LADDER_REFRESH_S: f64 = 0.1;

/// What the frame's opening writes decided, for the stages after them.
pub(super) struct FrameStart {
    pub(super) canvas_background: egui::Color32,
    /// The footprint layer is unavailable on this feed.
    pub(super) footprint_blocked: bool,
    /// The footprint layer will paint — which is not the same as the
    /// ladders accumulating: a range profile turns those on alone.
    pub(super) footprint_paints: bool,
}

/// Snapshot the forming bar's ladder at ~10 Hz rather than per print;
/// between snapshots the drawn numbers hold still. Taken with the
/// accumulation switch, because it has two consumers — the footprint layer
/// and the range-profile drawings — and each reading the live ladder on its
/// own cadence would show two different bars.
pub(super) fn snapshot_live_ladder(
    footprint: &mut PaneFootprint,
    partial_ladder: Option<&BarFootprint>,
    closed_total: usize,
    now: f64,
) {
    match partial_ladder {
        Some(partial_ladder) => {
            let stale = footprint
                .live
                .as_ref()
                .is_none_or(|(taken, snapshot_slot, _)| {
                    *snapshot_slot != closed_total || now - *taken >= LIVE_LADDER_REFRESH_S
                });
            if stale {
                footprint.live = Some((now, closed_total, partial_ladder.clone()));
                footprint.live_version = footprint.live_version.wrapping_add(1);
            }
        }
        None => {
            if footprint.live.take().is_some() {
                footprint.live_version = footprint.live_version.wrapping_add(1);
            }
        }
    }
}

/// The forming bar's snapshotted ladder, as both of its consumers read it.
pub(super) fn live_ladder(footprint: &PaneFootprint) -> Option<&BarFootprint> {
    footprint.live.as_ref().map(|(_, _, ladder)| ladder)
}

/// Whether the window holds no bar at all — a scale still exists (see
/// [`FrameLayout::resolve`]), and the chrome says why the canvas is empty.
pub(super) fn nothing_in_view(frame: &DrawFrame<'_>) -> bool {
    frame.visible_prefix.is_empty()
        && frame.visible_state.is_empty()
        && frame.partial_visible.is_none()
}

/// The frame's geometry once the viewport is clamped: the plot split, the
/// live lane beside the candles and the slots in view. Computed by
/// `ChartPane::lay_out`, read by every stage after it.
pub(super) struct FrameLayout {
    pub(super) areas: PlotAreas,
    pub(super) chart_rect: egui::Rect,
    /// The candles' own pane: the chart rect left of the lane divider.
    pub(super) history_rect: egui::Rect,
    pub(super) live_lane: Option<LiveLane>,
    pub(super) total: usize,
    pub(super) closed_total: usize,
    pub(super) start: usize,
    pub(super) end: usize,
    /// One slot's width in pixels, read after the clamp.
    pub(super) cw: f32,
    /// Where the indicator panes' vertical guide goes: the pointer's x while
    /// it is over the chart.
    pub(super) indicator_guide_x: Option<f32>,
}

/// Both bar series the frame reads: the venue prefix, the engine's closed
/// bars and the forming one.
pub(super) struct Series<'a> {
    pub(super) prefix: &'a [Bar],
    pub(super) closed: &'a [Bar],
    pub(super) partial: Option<&'a Bar>,
}

impl FrameLayout {
    /// The width the live lane takes off the chart's right edge, `0` without
    /// one.
    pub(super) fn lane_width_px(&self) -> f32 {
        self.live_lane.map_or(0.0, |lane| lane.width_px)
    }

    /// The visible slices and the price scale, as the [`DrawFrame`] every
    /// painter reads, plus the auto-fitted range the next frame's input
    /// handler converts pixels with. `None` when nothing yields a scale.
    pub(super) fn resolve<'a>(
        &'a self,
        painter: &'a egui::Painter,
        series: Series<'a>,
        last_auto_range: Option<(f64, f64)>,
        price_view: &PriceView,
        canvas_background: egui::Color32,
    ) -> Option<(DrawFrame<'a>, (f64, f64))> {
        let Series {
            prefix,
            closed,
            partial,
        } = series;
        let Self {
            chart_rect,
            history_rect,
            total,
            closed_total,
            start,
            end,
            cw,
            ..
        } = *self;
        // The visible closed bars, plus the partial if it falls in view. With
        // a venue prefix the window can straddle both series, so it is two
        // slices — chained where they are read rather than copied into one.
        // Copying was 24-48 KB every frame for the life of the pane, including
        // the common case of following the live edge, where the seam is three
        // months off screen and the prefix half of the window is empty.
        let closed_start = start.min(closed_total);
        let closed_end = end.min(closed_total);
        let visible_prefix = &prefix[closed_start.min(prefix.len())..closed_end.min(prefix.len())];
        let visible_state = &closed[closed_start.saturating_sub(prefix.len())
            ..closed_end.saturating_sub(prefix.len()).min(closed.len())];
        let partial_visible = partial.filter(|_| closed_total >= start && closed_total < end);

        // Auto-fit the visible bars, then apply any manual price pan/zoom. A
        // window with no bars in it still gets a scale (the last one, then the
        // newest bar), because a chart that draws nothing at all is
        // indistinguishable from a hung app — which is exactly how the blank
        // frame after a rebuild read.
        let auto_scale = chart::price_window(
            visible_prefix.iter().chain(visible_state),
            partial_visible,
            last_auto_range,
            partial.or_else(|| closed.last()),
            chart_rect.top(),
            chart_rect.bottom(),
        )?;
        let auto_range = auto_scale.range();
        let scale = price_view.scale(auto_range, chart_rect.top(), chart_rect.bottom());
        let frame = DrawFrame {
            painter,
            areas: &self.areas,
            chart_rect,
            history_rect,
            right: history_rect.right(),
            total,
            start,
            end,
            closed_start,
            closed_total,
            scale,
            prefix,
            closed,
            partial,
            partial_visible,
            visible_prefix,
            visible_state,
            canvas_background,
            cw,
        };
        Some((frame, auto_range))
    }
}

/// How the candles sit under the footprint this frame: the body's
/// half-width, the lane a sidebar candle keeps, the half-width the ladder's
/// content spans, and the faded style the layer leaves the candle in.
pub(super) struct CandleDress {
    pub(super) half: f32,
    pub(super) candle_lane: f32,
    pub(super) content_half: f32,
    faded: Option<CandleStyle>,
}

impl CandleDress {
    /// How the candle behaves under the footprint is the *style's* answer,
    /// not the frame's: a style that draws inside the candle needs its
    /// interior, and one that draws in a box beside it needs the candle out
    /// of the way entirely. With the layer off, candles are untouched at any
    /// zoom.
    pub(super) fn resolve(
        footprint: &PaneFootprint,
        chrome: &PaneChrome<'_>,
        paints: bool,
        cw: f32,
    ) -> Self {
        let half = chrome.style.candles.body_half_width(cw);
        // The style that will actually draw, not the one that was asked for:
        // a style below its own zoom floor hands over, and the candle must be
        // laid out for whichever one paints. Asking the requested style put a
        // sidebar lane under a style that draws full width.
        let requested_style = footprint.config.as_ref().unwrap_or(chrome.footprint).style;
        let footprint_style = footprint.lod.effective_style(requested_style);
        let treatment = footprint_style.candle_treatment();
        // The lane a sidebar candle keeps at the left of its slot, and the
        // style the layer leaves the candle in. Both from one function, whose
        // whole point is that they answer to `paints` and never to the
        // accumulation switch — see `footprint_render::candle_dressing`.
        let (candle_lane, faded) =
            crate::footprint_render::candle_dressing(paints, treatment, cw, chrome.style.candles);
        // The half-width the footprint's content actually spans. The lane is
        // cut out of *this*, so the candle placed beside it has to be measured
        // from the same edge — measuring from the candle's own body width put
        // it inside the box the lane was reserved next to, where the opaque
        // plate then painted straight over it.
        let content_half = treatment.content_half_width(cw, half);
        Self {
            half,
            candle_lane,
            content_half,
            faded,
        }
    }

    /// The style the candles paint in: the faded one while the footprint
    /// dresses them, else the chart's own.
    pub(super) fn style<'s>(&'s self, base: &'s CandleStyle) -> &'s CandleStyle {
        self.faded.as_ref().unwrap_or(base)
    }
}
