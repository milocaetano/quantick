//! Chart summary and append-only paginated bar-window projections.

pub(crate) use quantick_control_schema::chart::*;

use crate::app::TabsPort;
use quantick_control::{
    cursor::PageCursor,
    error::ControlError,
    id::{InstanceId, ModuleId, SnapshotScopeId},
    registry::ModuleDescriptor,
    wire::WireU64,
};

use quantick_engine::Bar;

use crate::{
    config::AppConfig,
    pane::{ChartPane, PaneSide},
    tab::Tab,
};

use super::{
    registry::{CaptureContext, ProjectionRegistry, ProjectionRegistryError},
    types::{DecimalRange, canonical_decimal, canonical_f32, wire_usize},
};

pub(crate) fn register(registry: &mut ProjectionRegistry) -> Result<(), ProjectionRegistryError> {
    let module_id = ModuleId::new(MODULE_ID).expect("static module ID is valid");
    registry.register_module(
        ModuleDescriptor {
            id: module_id.clone(),
            title: "Chart".to_owned(),
            description: "Pane framing, bar construction, and visible market coverage.".to_owned(),
        },
        revision,
    )?;
    registry.register_scope(
        SnapshotScopeId::new(SCOPE_ID).expect("static scope ID is valid"),
        module_id,
        SCHEMA_VERSION,
        "Chart summary",
        "Reports every pane's bar rule, revisions, viewport, price range, and coverage.",
        &["observe", "observe.market", "observe.chart"],
        project,
    )
}

fn revision<P: TabsPort + ?Sized>(app: &P) -> ChartSnapshot {
    snapshot(app)
}

fn project<P: TabsPort + ?Sized>(app: &P, _context: CaptureContext) -> ChartSnapshot {
    snapshot(app)
}

fn snapshot<P: TabsPort + ?Sized>(app: &P) -> ChartSnapshot {
    let active = app.tab_reads().active_tab_index();
    let config = app.tab_reads().config();
    let mut panes = Vec::new();
    for (tab_index, tab) in app.tab_reads().tabs().iter().enumerate() {
        let focused = tab.focused_side();
        let shown = usize::from(!tab.context_collapsed) * tab.context_panes_shown();
        for (pane, side) in tab.panes() {
            let visible = match side {
                PaneSide::Flow => tab.layout.shows_flow(),
                PaneSide::Time(slot) => tab.layout.shows_time() && slot < shown,
            };
            panes.push(pane_snapshot(
                app.tab_reads().tabs().id_at(tab_index),
                tab,
                pane,
                side,
                tab_index == active && visible,
                tab_index == active && focused == side,
                config,
            ));
        }
    }
    ChartSnapshot { panes }
}

fn pane_snapshot(
    tab_id: u64,
    tab: &Tab,
    pane: &ChartPane,
    side: PaneSide,
    visible: bool,
    focused: bool,
    config: &AppConfig,
) -> ChartPaneSnapshot {
    let seam = pane.seam_slot();
    let rule = pane.state.rule_diagnostics();
    ChartPaneSnapshot {
        tab_id: WireU64::new(tab_id),
        pane_id: WireU64::new(pane.id),
        side: side.into(),
        pane_index: wire_usize(side.index()),
        feed_id: tab.feed_id.clone(),
        symbol: tab.symbol.clone(),
        visible,
        focused,
        bar_spec: pane.state.spec().into(),
        timeline_revision: WireU64::new(pane.state.timeline_revision()),
        pagination_revision: WireU64::new(pane.pagination_revision()),
        closed_bar_count: wire_usize(pane.closed_slots()),
        uncounted_prints: Some(WireU64::new(rule.uncounted_trades)),
        inferred_price_step: pane.state.inferred_price_step().map(canonical_decimal),
        held_prints: Some(WireU64::new(rule.held_prints)),
        off_grid_prints: Some(WireU64::new(rule.off_grid_prints)),
        venue_history_bar_count: wire_usize(seam),
        backfill_boundary_slot: pane
            .state
            .backfill_boundary()
            .map(|boundary| wire_usize(seam + boundary)),
        has_in_progress_bar: pane.state.partial().is_some(),
        viewport: viewport_snapshot(pane),
        coverage: coverage(tab, pane, config),
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
                low: super::types::canonical_f64(low, PRICE_DECIMAL_PLACES)?,
                high: super::types::canonical_f64(high, PRICE_DECIMAL_PLACES)?,
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

fn coverage(tab: &Tab, pane: &ChartPane, config: &AppConfig) -> ChartCoverage {
    let oldest = pane.slot_open_time(0);
    let newest = pane
        .state
        .partial()
        .or_else(|| pane.state.bars().last())
        .or_else(|| pane.history_prefix.last())
        .map(|bar| bar.close_time);
    ChartCoverage {
        oldest_open_time_unix_ms: oldest,
        newest_close_time_unix_ms: newest,
        older_history_paging_supported: tab.capabilities(config).history_paging,
        venue_prefix_present: !pane.history_prefix.is_empty(),
    }
}

fn provenance_context(tab: &Tab, config: &AppConfig) -> BarProvenanceContext {
    // One vocabulary with the feed scope: the same tab must never be
    // described two ways.
    let provenance = super::feed::market_data_provenance(tab, config);
    BarProvenanceContext {
        engine_price: provenance.price,
        engine_volume: provenance.volume,
        engine_side: provenance.aggressor_side,
    }
}

pub(crate) fn bar_snapshot(
    tab: &Tab,
    pane: &ChartPane,
    slot: usize,
    bar: &Bar,
    state: BarStateDto,
    config: &AppConfig,
) -> BarSnapshot {
    let provenance = provenance_context(tab, config);
    bar_snapshot_with(pane, slot, bar, state, &provenance)
}

fn bar_snapshot_with(
    pane: &ChartPane,
    slot: usize,
    bar: &Bar,
    state: BarStateDto,
    context: &BarProvenanceContext,
) -> BarSnapshot {
    BarSnapshot::from_bar(
        slot,
        bar,
        state,
        pane.seam_slot(),
        pane.state.backfill_boundary(),
        context,
    )
}

/// Read one append-only page of closed chart bars. A live append is allowed;
/// a prefix install, backfill, reset, or bar-spec rebuild advances the pane's
/// pagination revision and returns `control.page_stale`.
#[cfg(test)]
pub(crate) fn chart_window<P: TabsPort + ?Sized>(
    app: &P,
    instance_id: &InstanceId,
    query: &ChartWindowQuery,
    cursor: Option<&PageCursor>,
) -> Result<ChartWindowPage, ControlError> {
    let canonical_query = serde_json::to_value(query)
        .map_err(|error| ControlError::invalid_request(format!("invalid chart query: {error}")))?;
    chart_window_prevalidated(app, instance_id, query, &canonical_query, cursor)
}

/// Gateway path for a query parsed, schema-checked, and canonicalized away
/// from the application thread.
pub(crate) fn chart_window_prevalidated<P: TabsPort + ?Sized>(
    app: &P,
    instance_id: &InstanceId,
    query: &ChartWindowQuery,
    canonical_query: &serde_json::Value,
    cursor: Option<&PageCursor>,
) -> Result<ChartWindowPage, ControlError> {
    query.validate_page_size()?;
    let tab = app
        .tab_reads()
        .tabs()
        .by_id(query.tab_id.get())
        .ok_or_else(|| ControlError::invalid_request("chart window names an unknown tab"))?;
    let Some((pane, side)) = tab.panes().find(|(pane, _)| pane.id == query.pane_id.get()) else {
        return Err(ControlError::invalid_request(
            "chart window names an unknown pane on the requested tab",
        ));
    };
    let closed = pane.closed_slots();
    let consistency_revision = WireU64::new(pane.pagination_revision());
    let selection = ChartWindowSelection::resolve(
        query,
        instance_id,
        canonical_query,
        cursor,
        consistency_revision,
        closed,
        pane.frame.chart_area.map(|_| visible_slots(pane)),
    )?;
    let provenance = provenance_context(tab, app.tab_reads().config());
    let items = selection
        .slots
        .clone()
        .filter_map(|slot| {
            pane.closed_bar(slot)
                .map(|bar| bar_snapshot_with(pane, slot, bar, BarStateDto::Closed, &provenance))
        })
        .collect::<Vec<_>>();
    let bars = selection.complete(items)?;
    let partial_slot = pane.closed_slots();
    let in_progress_bar = pane.state.partial().map(|bar| {
        bar_snapshot_with(
            pane,
            partial_slot,
            bar,
            BarStateDto::InProgress,
            &provenance,
        )
    });

    Ok(ChartWindowPage {
        captured_at_unix_ms: crate::metrics::wall_clock_ms(),
        tab_id: query.tab_id,
        pane_id: query.pane_id,
        side: side.into(),
        feed_id: tab.feed_id.clone(),
        symbol: tab.symbol.clone(),
        consistency_revision,
        high_water_slot_exclusive: selection.high_water,
        viewport: viewport_snapshot(pane),
        bars,
        in_progress_bar,
        omitted_modules: OMITTED_WINDOW_MODULE_IDS
            .into_iter()
            .map(|id| ModuleId::new(id).expect("static omitted module ID is valid"))
            .collect(),
    })
}
