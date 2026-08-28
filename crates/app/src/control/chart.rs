//! Chart summary and append-only paginated bar-window projections.

use quantick_control::{
    cursor::{PageContext, PageCursor, PaginationConsistency},
    error::ControlError,
    id::{InstanceId, ModuleId, SnapshotScopeId},
    limits::CONTROL_CHART_WINDOW_MAX_PAGE_ITEMS,
    registry::ModuleDescriptor,
    wire::{CanonicalDecimal, WireU64},
};

use quantick_engine::Bar;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    app::QuantickApp,
    config::AppConfig,
    pane::{ChartPane, PaneSide},
    tab::Tab,
};

use super::{
    registry::{CaptureContext, ProjectionRegistry, ProjectionRegistryError},
    types::{BarSpecDto, DecimalRange, PaneSideDto, canonical_decimal, canonical_f32, wire_usize},
};

pub(crate) const SCOPE_ID: &str = "chart.summary";
pub(crate) const WINDOW_SCOPE_ID: &str = "chart.window";
const MODULE_ID: &str = "chart";
const SCHEMA_VERSION: u32 = 1;
const VIEWPORT_DECIMAL_PLACES: u32 = 6;
const PRICE_DECIMAL_PLACES: u32 = 10;
const PIXEL_DECIMAL_PLACES: u32 = 3;
const OMITTED_WINDOW_MODULE_IDS: [&str; 3] = ["drawings", "indicators", "orderflow"];

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct ChartSnapshot {
    pub panes: Vec<ChartPaneSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct ChartPaneSnapshot {
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    pub side: PaneSideDto,
    /// The pane's address within its tab — `0` the flow pane, `1..` the
    /// context stack top to bottom — the number `layout.focus` takes.
    pub pane_index: WireU64,
    pub feed_id: String,
    pub symbol: String,
    pub visible: bool,
    pub focused: bool,
    pub bar_spec: BarSpecDto,
    pub timeline_revision: WireU64,
    pub pagination_revision: WireU64,
    pub closed_bar_count: WireU64,
    pub venue_history_bar_count: WireU64,
    pub backfill_boundary_slot: Option<WireU64>,
    pub has_in_progress_bar: bool,
    pub viewport: ViewportSnapshot,
    pub coverage: ChartCoverage,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct ViewportSnapshot {
    pub geometry_available: bool,
    /// The slots the viewport asks the renderer to paint: generous by one bar
    /// at each edge, so a bar partly off screen is drawn rather than clipped.
    /// This is the pane's own notion of "visible", not the exact pixel set.
    pub visible_start_slot: WireU64,
    pub visible_end_slot_exclusive: WireU64,
    #[schemars(extend("x-unit" = "pixels_per_bar"))]
    pub pixels_per_bar: CanonicalDecimal,
    pub right_edge_bar: CanonicalDecimal,
    pub follows_live: bool,
    pub price_auto_fit: bool,
    pub price_axis_inverted: bool,
    pub price_range: Option<DecimalRange>,
    #[schemars(extend("x-unit" = "pixels"))]
    pub chart_width_px: Option<CanonicalDecimal>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct ChartCoverage {
    #[schemars(extend("x-unit" = "unix_milliseconds"))]
    pub oldest_open_time_unix_ms: Option<i64>,
    #[schemars(extend("x-unit" = "unix_milliseconds"))]
    pub newest_close_time_unix_ms: Option<i64>,
    pub older_history_paging_supported: bool,
    pub venue_prefix_present: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BarStateDto {
    Closed,
    InProgress,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct BarSnapshot {
    pub slot: WireU64,
    pub state: BarStateDto,
    #[schemars(extend("x-unit" = "unix_milliseconds"))]
    pub open_time_unix_ms: i64,
    #[schemars(extend("x-unit" = "unix_milliseconds"))]
    pub close_time_unix_ms: i64,
    pub open: CanonicalDecimal,
    pub high: CanonicalDecimal,
    pub low: CanonicalDecimal,
    pub close: CanonicalDecimal,
    pub volume: CanonicalDecimal,
    pub buy_volume: CanonicalDecimal,
    pub sell_volume: CanonicalDecimal,
    pub delta: CanonicalDecimal,
    pub trade_count: WireU64,
    pub provenance: BarProvenance,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct BarProvenance {
    pub source: String,
    pub completeness: String,
    pub price: String,
    pub volume: String,
    pub aggressor_side: String,
    pub trade_count: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct ChartWindowQuery {
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    pub range: ChartWindowRange,
    #[schemars(range(min = 1, max = CONTROL_CHART_WINDOW_MAX_PAGE_ITEMS))]
    pub page_size: usize,
}

impl ChartWindowQuery {
    #[cfg(test)]
    pub fn visible(tab_id: u64, pane_id: u64) -> Self {
        Self {
            tab_id: WireU64::new(tab_id),
            pane_id: WireU64::new(pane_id),
            range: ChartWindowRange::Visible,
            page_size: CONTROL_CHART_WINDOW_MAX_PAGE_ITEMS,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum ChartWindowRange {
    Visible,
    Slots {
        start_slot: WireU64,
        end_slot_exclusive: WireU64,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct ChartWindowPage {
    #[schemars(extend("x-unit" = "unix_milliseconds"))]
    pub captured_at_unix_ms: i64,
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    pub side: PaneSideDto,
    pub feed_id: String,
    pub symbol: String,
    pub consistency_revision: WireU64,
    pub high_water_slot_exclusive: WireU64,
    pub viewport: ViewportSnapshot,
    pub bars: ChartBarPage,
    pub in_progress_bar: Option<BarSnapshot>,
    pub omitted_modules: Vec<ModuleId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct ChartBarPage {
    #[schemars(length(max = CONTROL_CHART_WINDOW_MAX_PAGE_ITEMS))]
    pub items: Vec<BarSnapshot>,
    #[schemars(range(max = CONTROL_CHART_WINDOW_MAX_PAGE_ITEMS))]
    pub item_count: usize,
    pub has_more: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<PageCursor>,
}

impl ChartBarPage {
    fn new(items: Vec<BarSnapshot>, next_cursor: Option<PageCursor>) -> Result<Self, ControlError> {
        if items.len() > CONTROL_CHART_WINDOW_MAX_PAGE_ITEMS {
            return Err(ControlError::invalid_request(
                "chart page exceeds its application-thread item limit",
            ));
        }
        let item_count = items.len();
        Ok(Self {
            items,
            item_count,
            has_more: next_cursor.is_some(),
            next_cursor,
        })
    }
}

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

fn revision(app: &QuantickApp) -> ChartSnapshot {
    snapshot(app)
}

fn project(app: &QuantickApp, _context: CaptureContext) -> ChartSnapshot {
    snapshot(app)
}

fn snapshot(app: &QuantickApp) -> ChartSnapshot {
    let active = app.control_active_tab_index();
    let config = app.control_config();
    let mut panes = Vec::new();
    for (tab_index, tab) in app.control_tabs().iter().enumerate() {
        let focused = tab.focused_side();
        let shown = tab.context_panes_shown();
        for (pane, side) in tab.panes() {
            let visible = match side {
                PaneSide::Flow => tab.layout.shows_flow(),
                PaneSide::Time(slot) => tab.layout.shows_time() && slot < shown,
            };
            panes.push(pane_snapshot(
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
    tab: &Tab,
    pane: &ChartPane,
    side: PaneSide,
    visible: bool,
    focused: bool,
    config: &AppConfig,
) -> ChartPaneSnapshot {
    let seam = pane.seam_slot();
    ChartPaneSnapshot {
        tab_id: WireU64::new(tab.id),
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
    let price_range = pane
        .last_auto_range
        .map(|auto| pane.price_view.resolve(auto));
    let chart_width_px = pane.last_chart_area.map(|chart| {
        let right = pane.last_lane_divider_x.unwrap_or_else(|| chart.right());
        (right - chart.left()).max(0.0)
    });
    ViewportSnapshot {
        geometry_available: pane.last_chart_area.is_some(),
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
    }
}

fn visible_slots(pane: &ChartPane) -> (usize, usize) {
    let total = pane.slots();
    let Some(chart) = pane.last_chart_area else {
        return (0, 0);
    };
    let right = pane.last_lane_divider_x.unwrap_or_else(|| chart.right());
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

struct ProvenanceContext {
    engine_price: String,
    engine_volume: String,
    engine_side: String,
}

fn provenance_context(tab: &Tab, config: &AppConfig) -> ProvenanceContext {
    // One vocabulary with the feed scope: the same tab must never be
    // described two ways.
    let provenance = super::feed::market_data_provenance(tab, config);
    ProvenanceContext {
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
    bar_snapshot_with(tab, pane, slot, bar, state, &provenance)
}

fn bar_snapshot_with(
    _tab: &Tab,
    pane: &ChartPane,
    slot: usize,
    bar: &Bar,
    state: BarStateDto,
    context: &ProvenanceContext,
) -> BarSnapshot {
    let seam = pane.seam_slot();
    let venue_prefix = slot < seam;
    let source = if venue_prefix {
        "venue_ohlcv"
    } else {
        let engine_slot = slot - seam;
        match pane.state.backfill_boundary() {
            Some(boundary) if engine_slot < boundary => "trade_backfill",
            Some(boundary) if engine_slot == boundary => "live_or_backfill_live_boundary",
            _ => "live_trades",
        }
    };
    BarSnapshot {
        slot: wire_usize(slot),
        state,
        open_time_unix_ms: bar.open_time,
        close_time_unix_ms: bar.close_time,
        open: canonical_decimal(bar.open),
        high: canonical_decimal(bar.high),
        low: canonical_decimal(bar.low),
        close: canonical_decimal(bar.close),
        volume: canonical_decimal(bar.volume()),
        buy_volume: canonical_decimal(bar.buy_volume),
        sell_volume: canonical_decimal(bar.sell_volume),
        delta: canonical_decimal(bar.delta()),
        trade_count: WireU64::new(bar.trade_count),
        provenance: BarProvenance {
            source: source.to_owned(),
            completeness: match state {
                BarStateDto::Closed => "complete",
                BarStateDto::InProgress => "in_progress",
            }
            .to_owned(),
            price: if venue_prefix {
                "venue_candle".to_owned()
            } else {
                context.engine_price.clone()
            },
            volume: if venue_prefix {
                "venue_reported".to_owned()
            } else {
                context.engine_volume.clone()
            },
            aggressor_side: if venue_prefix {
                "unavailable_or_venue_dependent".to_owned()
            } else {
                context.engine_side.clone()
            },
            trade_count: if venue_prefix && bar.trade_count == 0 {
                "unavailable".to_owned()
            } else if venue_prefix {
                "venue_reported".to_owned()
            } else {
                "derived_from_trades".to_owned()
            },
        },
    }
}

/// Read one append-only page of closed chart bars. A live append is allowed;
/// a prefix install, backfill, reset, or bar-spec rebuild advances the pane's
/// pagination revision and returns `control.page_stale`.
#[cfg(test)]
pub(crate) fn chart_window(
    app: &QuantickApp,
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
pub(crate) fn chart_window_prevalidated(
    app: &QuantickApp,
    instance_id: &InstanceId,
    query: &ChartWindowQuery,
    canonical_query: &serde_json::Value,
    cursor: Option<&PageCursor>,
) -> Result<ChartWindowPage, ControlError> {
    if query.page_size == 0 || query.page_size > CONTROL_CHART_WINDOW_MAX_PAGE_ITEMS {
        return Err(ControlError::invalid_request(format!(
            "chart page size must be in 1..={CONTROL_CHART_WINDOW_MAX_PAGE_ITEMS}"
        )));
    }
    let tab = app
        .control_tabs()
        .iter()
        .find(|tab| tab.id == query.tab_id.get())
        .ok_or_else(|| ControlError::invalid_request("chart window names an unknown tab"))?;
    let Some((pane, side)) = tab.panes().find(|(pane, _)| pane.id == query.pane_id.get()) else {
        return Err(ControlError::invalid_request(
            "chart window names an unknown pane on the requested tab",
        ));
    };
    let closed = pane.closed_slots();
    let scope_id = SnapshotScopeId::new(WINDOW_SCOPE_ID).expect("static scope ID is valid");
    let consistency_revision = WireU64::new(pane.pagination_revision());

    let (position, high_water, context) = if let Some(cursor) = cursor {
        let high_water = cursor.high_water_position.ok_or_else(|| {
            ControlError::invalid_request("append-only chart cursor has no high-water position")
        })?;
        let context = PageContext {
            instance_id,
            scope_id: &scope_id,
            query: canonical_query,
            consistency_mode: PaginationConsistency::AppendOnly,
            consistency_revision,
            high_water_position: Some(high_water),
            resource_id: None,
            resource_available: true,
        };
        cursor.validate_next(&context)?;
        let high_water_usize = usize::try_from(high_water.get()).unwrap_or(usize::MAX);
        if high_water_usize > closed {
            return Err(ControlError::page_stale(
                "the chart no longer contains the cursor's high-water prefix",
            ));
        }
        (cursor.next_position, high_water, context)
    } else {
        let (start, requested_end) = match &query.range {
            ChartWindowRange::Visible => {
                if pane.last_chart_area.is_none() {
                    // A well-formed query the pane cannot answer yet: say so
                    // with a retryable code and a next step, not as a malformed
                    // request the client would have to guess about.
                    let mut error = ControlError::new(
                        quantick_control::id::ErrorCode::new(
                            quantick_control::error::codes::CAPABILITY_UNAVAILABLE,
                        )
                        .expect("static error code is valid"),
                        "visible chart range is unavailable before the pane has painted",
                        true,
                    );
                    error.context.next_steps = vec![
                        "Retry after the pane's first frame, or ask for an explicit slot range."
                            .to_owned(),
                    ];
                    return Err(error);
                }
                let (start, end) = visible_slots(pane);
                (start.min(closed), end.min(closed))
            }
            ChartWindowRange::Slots {
                start_slot,
                end_slot_exclusive,
            } => (
                usize::try_from(start_slot.get()).unwrap_or(usize::MAX),
                usize::try_from(end_slot_exclusive.get()).unwrap_or(usize::MAX),
            ),
        };
        if start > requested_end || start > closed {
            return Err(ControlError::invalid_request(
                "chart slot range is reversed or starts beyond loaded closed bars",
            ));
        }
        let high_water = wire_usize(requested_end.min(closed));
        let context = PageContext {
            instance_id,
            scope_id: &scope_id,
            query: canonical_query,
            consistency_mode: PaginationConsistency::AppendOnly,
            consistency_revision,
            high_water_position: Some(high_water),
            resource_id: None,
            resource_available: true,
        };
        (wire_usize(start), high_water, context)
    };

    let start = usize::try_from(position.get()).unwrap_or(usize::MAX);
    let stop = usize::try_from(high_water.get()).unwrap_or(usize::MAX);
    if start > stop {
        return Err(ControlError::invalid_request(
            "chart cursor position exceeds its high-water mark",
        ));
    }
    let end = start.saturating_add(query.page_size).min(stop);
    let provenance = provenance_context(tab, app.control_config());
    let items = (start..end)
        .filter_map(|slot| {
            pane.closed_bar(slot).map(|bar| {
                bar_snapshot_with(tab, pane, slot, bar, BarStateDto::Closed, &provenance)
            })
        })
        .collect::<Vec<_>>();
    if items.len() != end.saturating_sub(start) {
        return Err(ControlError::page_stale(
            "the chart's closed-bar prefix changed during pagination",
        ));
    }
    let next_cursor = if end < stop {
        let next_position = wire_usize(end);
        Some(if let Some(cursor) = cursor {
            let mut next = cursor.clone();
            next.next_position = next_position;
            next
        } else {
            PageCursor::first(&context, next_position)?
        })
    } else {
        None
    };
    let bars = ChartBarPage::new(items, next_cursor)?;
    let partial_slot = pane.closed_slots();
    let in_progress_bar = pane.state.partial().map(|bar| {
        bar_snapshot_with(
            tab,
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
        high_water_slot_exclusive: high_water,
        viewport: viewport_snapshot(pane),
        bars,
        in_progress_bar,
        omitted_modules: OMITTED_WINDOW_MODULE_IDS
            .into_iter()
            .map(|id| ModuleId::new(id).expect("static omitted module ID is valid"))
            .collect(),
    })
}
