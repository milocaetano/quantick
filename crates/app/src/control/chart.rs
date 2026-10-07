//! The window's adapter onto `quantick_control_handlers::chart`: only what the
//! window can say — where its last frame laid a pane out, which tab and side
//! it sits on, and what its feed provides.

pub(crate) use quantick_control_handlers::chart::chart_window_prevalidated;
use quantick_control_handlers::chart::{
    ChartPaneEntry, ChartPaneSource, ChartPort, MissingPane, PaneFraming,
};
pub(crate) use quantick_control_schema::chart::*;

use quantick_engine::Bar;

use crate::{
    app::control_host::ControlPort,
    config::AppConfig,
    pane::{ChartPane, PaneSide},
    state::ChartState,
    tab::Tab,
};

use super::types::{DecimalRange, canonical_f32, canonical_f64, wire_usize};

impl ChartPort for dyn ControlPort {
    fn visit_chart_panes(&self, visit: &mut dyn FnMut(ChartPaneEntry<'_>, PaneFraming)) {
        let reads = self.tab_reads();
        let active = reads.active_tab_index();
        let config = reads.config();
        for (tab_index, tab) in reads.tabs().iter().enumerate() {
            let tab_id = reads.tabs().id_at(tab_index);
            let focused = tab.focused_side();
            let shown = usize::from(!tab.context_collapsed) * tab.context_panes_shown();
            for (pane, side) in tab.panes() {
                let visible = match side {
                    PaneSide::Flow => tab.layout.shows_flow(),
                    PaneSide::Time(slot) => tab.layout.shows_time() && slot < shown,
                };
                let framing = PaneFraming {
                    visible: tab_index == active && visible,
                    focused: tab_index == active && focused == side,
                };
                let read = PaneRead { tab, pane, config };
                visit(entry(tab_id, tab, side, &read), framing);
            }
        }
    }

    fn visit_chart_pane(
        &self,
        tab_id: u64,
        pane_id: u64,
        visit: &mut dyn FnMut(ChartPaneEntry<'_>),
    ) -> Result<(), MissingPane> {
        let reads = self.tab_reads();
        let tab = reads.tabs().by_id(tab_id).ok_or(MissingPane::Tab)?;
        let (pane, side) = tab
            .panes()
            .find(|(pane, _)| pane.id == pane_id)
            .ok_or(MissingPane::Pane)?;
        let read = PaneRead {
            tab,
            pane,
            config: reads.config(),
        };
        visit(entry(tab_id, tab, side, &read));
        Ok(())
    }
}

fn entry<'a>(
    tab_id: u64,
    tab: &'a Tab,
    side: PaneSide,
    read: &'a PaneRead<'a>,
) -> ChartPaneEntry<'a> {
    ChartPaneEntry {
        tab_id,
        side: side.into(),
        pane_index: side.index(),
        feed_id: &tab.feed_id,
        symbol: &tab.symbol,
        pane: read,
    }
}

/// One pane with the tab and config its reads consult.
struct PaneRead<'a> {
    tab: &'a Tab,
    pane: &'a ChartPane,
    config: &'a AppConfig,
}

impl ChartPaneSource for PaneRead<'_> {
    fn pane_id(&self) -> u64 {
        self.pane.id
    }
    fn state(&self) -> &ChartState {
        &self.pane.state
    }
    fn pagination_revision(&self) -> u64 {
        self.pane.pagination_revision()
    }
    fn closed_slots(&self) -> usize {
        self.pane.closed_slots()
    }
    fn seam_slot(&self) -> usize {
        self.pane.seam_slot()
    }
    fn closed_bar(&self, slot: usize) -> Option<&Bar> {
        self.pane.closed_bar(slot)
    }
    fn venue_prefix(&self) -> &[Bar] {
        &self.pane.history_prefix
    }
    fn slot_open_time(&self, slot: usize) -> Option<i64> {
        self.pane.slot_open_time(slot)
    }
    fn visible_slots(&self) -> Option<(usize, usize)> {
        self.pane.frame.chart_area.map(|_| visible_slots(self.pane))
    }
    fn viewport(&self) -> ViewportSnapshot {
        viewport_snapshot(self.pane)
    }
    fn history_paging(&self) -> bool {
        self.tab.capabilities(self.config).history_paging
    }
    fn venue_record_starts_inside(&self, interval_ms: i64) -> bool {
        self.tab.venue_record_starts_inside(interval_ms)
    }
    fn provenance(&self) -> BarProvenanceContext {
        provenance_context(self.tab, self.config)
    }
}

pub(crate) fn viewport_snapshot(pane: &ChartPane) -> ViewportSnapshot {
    let total = pane.slots();
    let (start, end) = visible_slots(pane);
    let price_range = pane.price_view.manual_range().or(pane.frame.auto_range);
    let chart_width_px = pane.frame.chart_area.map(|chart| {
        let right = pane.frame.lane_divider_x.unwrap_or_else(|| chart.right());
        (right - chart.left()).max(0.0)
    });
    ViewportSnapshot {
        geometry_available: pane.frame.chart_area.is_some(),
        visible_start_slot: wire_usize(start),
        visible_end_slot_exclusive: wire_usize(end),
        pixels_per_bar: canonical_f32(pane.viewport.px_per_bar(), VIEWPORT_DECIMAL_PLACES)
            .expect("viewport pixels per bar is finite"),
        right_edge_bar: canonical_f32(pane.viewport.right_edge_bar(total), VIEWPORT_DECIMAL_PLACES)
            .expect("viewport right edge is finite"),
        follows_live: pane.viewport.follows_live(),
        price_auto_fit: pane.price_view.is_auto(),
        price_axis_inverted: pane.price_view.is_inverted(),
        price_range: price_range.and_then(|(low, high)| {
            Some(DecimalRange {
                low: canonical_f64(low, PRICE_DECIMAL_PLACES)?,
                high: canonical_f64(high, PRICE_DECIMAL_PLACES)?,
            })
        }),
        chart_width_px: chart_width_px.and_then(|width| canonical_f32(width, PIXEL_DECIMAL_PLACES)),
        tape: pane
            .orderflow
            .as_ref()
            .and_then(|tape| tape.tape_view_snapshot()),
    }
}

fn visible_slots(pane: &ChartPane) -> (usize, usize) {
    let total = pane.slots();
    let Some(chart) = pane.frame.chart_area else {
        return (0, 0);
    };
    let right = pane.frame.lane_divider_x.unwrap_or_else(|| chart.right());
    pane.viewport
        .visible_range((right - chart.left()).max(0.0), total)
}

fn provenance_context(tab: &Tab, config: &AppConfig) -> BarProvenanceContext {
    // One vocabulary with the feed scope: the same tab must never be
    // described two ways.
    let provenance = super::feed::market_data_provenance(tab, config);
    BarProvenanceContext {
        engine_price: provenance.price,
        engine_volume: provenance.volume,
        engine_side: provenance.aggressor_side,
        venue_aggressor_split: tab.capabilities(config).ohlcv_aggressor_split,
    }
}

/// One bar of `pane` on the wire, as the window read and the pointer read
/// both put it.
pub(crate) fn bar_snapshot(
    tab: &Tab,
    pane: &ChartPane,
    slot: usize,
    bar: &Bar,
    state: BarStateDto,
    config: &AppConfig,
) -> BarSnapshot {
    let provenance = provenance_context(tab, config);
    let read = PaneRead { tab, pane, config };
    quantick_control_handlers::chart::bar_snapshot(&read, slot, bar, state, &provenance)
}

/// Read one append-only page of closed chart bars, stamped now.
#[cfg(test)]
pub(crate) fn chart_window(
    app: &crate::app::ControlWindow,
    instance_id: &quantick_control::id::InstanceId,
    query: &ChartWindowQuery,
    cursor: Option<&quantick_control::cursor::PageCursor>,
) -> Result<ChartWindowPage, quantick_control::error::ControlError> {
    let canonical_query = serde_json::to_value(query).map_err(|error| {
        quantick_control::error::ControlError::invalid_request(format!(
            "invalid chart query: {error}"
        ))
    })?;
    let now = crate::metrics::wall_clock_ms();
    chart_window_prevalidated(app, instance_id, query, &canonical_query, cursor, now)
}
