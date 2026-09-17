//! Resolve projection gates/window once and sweep only intersecting history.
use super::model::PriceWindow;
use crate::config::{DisplayGrouping, HeatmapConfig};
use crate::grouping::{EffectiveGrouping, GroupedLiquidity, GroupingWindow, sweep_grouped_runs};
use crate::history::{CoverageSegment, LiquidityHistory};
use crate::timeline::BarTimeline;
use rust_decimal::Decimal;

pub(super) enum WindowResolution {
    Disabled(EffectiveGrouping),
    Empty(EffectiveGrouping),
    Ready(ResolvedWindow),
}

pub(super) struct ResolvedWindow {
    pub(super) effective_grouping: EffectiveGrouping,
    pub(super) time_start: i64,
    pub(super) time_end: i64,
    pub(super) depth_enabled: bool,
    retained_start: i64,
    open_run_end_ms: i64,
}

pub(super) struct WindowLiquidity {
    pub(super) coverage: Vec<CoverageSegment>,
    pub(super) grouped: GroupedLiquidity,
}

pub(super) fn resolve_window(
    history: &LiquidityHistory,
    timeline: &BarTimeline,
    prices: PriceWindow,
) -> WindowResolution {
    let config = history.config();
    let effective_grouping = EffectiveGrouping::resolve(
        config.display_grouping,
        config.price_grouping,
        prices.high - prices.low,
    );
    if !config.any_layer_enabled() {
        return WindowResolution::Disabled(effective_grouping);
    }
    let Some((time_start, time_end)) = timeline.timestamp_range() else {
        return WindowResolution::Empty(effective_grouping);
    };
    // The depth layer is projected only while the map is both recording and on
    // screen — on *either* pane. Retained runs survive hiding it untouched:
    // they simply stop being drawn and keep accumulating, so the aggression
    // layer can render without the map behind it and reopening repaints the
    // whole retained past.
    //
    // "Either pane" is the whole point and was the bug: these cells span the
    // normalized x axis, tape included, and the renderer clips them per pane
    // (`layer_clip`). Gating production on the *candles'* switch therefore
    // deleted the tape's map along with the chart's — the projection decided
    // there was nothing to draw before the renderer ever got to decide where.
    let depth_enabled = config.depth_visible_anywhere();
    let retained_start = history
        .retention_start_ms()
        .map_or(time_start, |start| start.max(time_start));
    let open_run_end_ms = history.latest_book_ms().unwrap_or(time_end);
    WindowResolution::Ready(ResolvedWindow {
        effective_grouping,
        time_start,
        time_end,
        depth_enabled,
        retained_start,
        open_run_end_ms,
    })
}

pub(super) fn sweep_window(
    history: &LiquidityHistory,
    window: &ResolvedWindow,
    prices: PriceWindow,
) -> WindowLiquidity {
    let ResolvedWindow {
        effective_grouping,
        time_end,
        depth_enabled,
        retained_start,
        open_run_end_ms,
        ..
    } = *window;
    let coverage: Vec<_> = if depth_enabled {
        history.coverage_segments().cloned().collect()
    } else {
        Vec::new()
    };
    let grouped = if depth_enabled {
        sweep_grouped_runs(
            history.runs_intersecting(retained_start, time_end),
            coverage.iter(),
            effective_grouping,
            GroupingWindow {
                start_ms: retained_start,
                end_ms: time_end,
                open_run_end_ms,
                price_low: prices.low,
                price_high: prices.high,
            },
        )
    } else {
        GroupedLiquidity::default()
    };

    WindowLiquidity { coverage, grouped }
}

/// The price resolution the tape clusters on.
///
/// Native, always: the adaptive display grouping is a *compression* device for
/// the candles, where a wide window has to fit many bars into few pixels, and
/// resolving it against the visible price span is what made the candles' zoom
/// decide which of the tape's prints fused together. The tape is not
/// compressed — it has the room to draw prints one by one, which is the whole
/// reason a scalper reads it — so it clusters at capture resolution and stays
/// the same picture through every zoom of the pane beside it.
pub(super) fn lane_grouping(config: &HeatmapConfig) -> EffectiveGrouping {
    // Only the adaptive mode is overridden. `Adaptive` resolves against the
    // visible price span, which is how the candles' zoom came to decide which
    // of the tape's prints fused together — that is the leak. `Multiple(n)` is
    // the trader saying "give me rows this wide" and has nothing to do with
    // zoom, so it is obeyed on the tape as it is on the candles; overriding it
    // would leave an illiquid instrument readable on one pane and not the
    // other, with no control and no explanation.
    let display = match config.display_grouping {
        DisplayGrouping::Adaptive { .. } => DisplayGrouping::Native,
        chosen => chosen,
    };
    EffectiveGrouping::resolve(display, config.price_grouping, Decimal::ZERO)
}

#[cfg(test)]
mod tests;
