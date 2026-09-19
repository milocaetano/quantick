//! Order-flow projections — the book, the tape, and the three layers drawn
//! over them.
//!
//! Five scopes under one owner module. Every one of them reads the frame the
//! application last published and nothing else: a capture never asks the book
//! worker for fresh state, because one requested scope must not advance
//! another scope underneath the same capture, and never rebuilds a projection,
//! because a capture runs on the UI thread under `CONTROL_UI_BUDGET_US`.
//!
//! What these scopes deliberately do *not* republish is the order-flow health
//! counters. `health.summary` already carries them, in full, and one number
//! with two homes drifts. These scopes carry the market content — prices,
//! quantities, ladders and the setup they are drawn with — which is what an
//! agent asked to "read the order flow" actually needs.
//!
//! Provenance is stated where it is not obvious. Book levels are what the
//! venue published, so they are band limits and not inference. Trade
//! aggression is the venue's own side where a venue reports one and the tick
//! rule where it does not; the feed scope owns that declaration for the market
//! as a whole and it is named here rather than restated per level.

pub(crate) use quantick_control_schema::orderflow::*;

use crate::app::TabsPort;
use quantick_control::{
    id::{ModuleId, SnapshotScopeId},
    limits::CONTROL_SNAPSHOT_MAX_BOOK_LEVELS_PER_SIDE,
    registry::ModuleDescriptor,
    wire::WireU64,
};

use crate::{orderflow_view::OrderflowView, pane::ChartPane, tab::Tab};

use super::{
    registry::{CaptureContext, ProjectionRegistry, ProjectionRegistryError},
    types::{
        AvailabilitySnapshot, available, canonical_decimal, canonical_f32, unavailable, wire_usize,
    },
};

pub(crate) fn register(registry: &mut ProjectionRegistry) -> Result<(), ProjectionRegistryError> {
    let module_id = ModuleId::new(MODULE_ID).expect("static module ID is valid");
    registry.register_module(
        ModuleDescriptor {
            id: module_id.clone(),
            title: "Order flow".to_owned(),
            description: "The book, the tape, and the layers drawn over them.".to_owned(),
        },
        revision,
    )?;
    registry.register_scope(
        SnapshotScopeId::new(TAPE_SCOPE_ID).expect("static scope ID is valid"),
        module_id.clone(),
        SCHEMA_VERSION,
        "Tape",
        "Reports how current each pane's tape is, where its live lane ends, and where its aggressor side comes from.",
        &["observe", "observe.orderflow"],
        project_tape,
    )?;
    registry.register_scope(
        SnapshotScopeId::new(FOOTPRINT_SCOPE_ID).expect("static scope ID is valid"),
        module_id.clone(),
        SCHEMA_VERSION,
        "Footprint",
        "Reports whether the candle footprint layer is drawn and the setup it is drawn with.",
        &["observe", "observe.orderflow", "observe.chart"],
        project_footprint,
    )?;
    registry.register_scope(
        SnapshotScopeId::new(BUBBLES_SCOPE_ID).expect("static scope ID is valid"),
        module_id.clone(),
        SCHEMA_VERSION,
        "Aggression bubbles",
        "Reports whether aggression bubbles are drawn over the chart and the lane, and what the display floor keeps off the canvas.",
        &["observe", "observe.orderflow"],
        project_bubbles,
    )?;
    registry.register_scope(
        SnapshotScopeId::new(HEATMAP_SCOPE_ID).expect("static scope ID is valid"),
        module_id.clone(),
        SCHEMA_VERSION,
        "Depth heatmap",
        "Reports whether depth heat is drawn, at which capture and display grouping, and with which retention.",
        &["observe", "observe.orderflow"],
        project_heatmap,
    )?;
    registry.register_scope(
        SnapshotScopeId::new(L2_SCOPE_ID).expect("static scope ID is valid"),
        module_id,
        SCHEMA_VERSION,
        "Order book",
        "Reports the published book around the spread: best bid and ask, the clipped ladders, and the capture state.",
        &["observe", "observe.orderflow", "observe.market"],
        project_l2,
    )
}

/// The module's revision key: the setup and the state of the book, not the
/// prices moving through it.
///
/// Book levels change on every depth update and the tape's age changes on
/// every frame. A key holding either would differ at every capture and so
/// would mark nothing — the same reasoning `health.rs` applies to its frame
/// averages. What this tracks is a change a person made or a connection
/// underwent: a layer switched, a grouping changed, a setup edited, the
/// capture state moving between disabled, syncing and live.
fn revision<P: TabsPort + ?Sized>(app: &P) -> Vec<OrderflowRevisionKey> {
    app.tab_reads()
        .tabs()
        .iter_with_ids()
        .map(|(tab_id, tab)| OrderflowRevisionKey {
            tab_id,
            panes: tab
                .panes()
                .map(|(pane, _side)| PaneOrderflowRevisionKey {
                    pane_id: pane.id,
                    footprint_visible: pane.footprint.visible,
                    footprint_overridden: pane.footprint.config.is_some(),
                    footprint_setup: format!(
                        "{:?}",
                        pane.footprint_config(app.tab_reads().footprint_config())
                    ),
                    engine: pane.orderflow.as_ref().map(|view| {
                        let (status, _ladder, grouping) = view.cached_book();
                        EngineRevisionKey {
                            enabled: view.enabled(),
                            depth_visible: view.depth_visible(),
                            lane_depth_visible: view.lane_depth_visible(),
                            bubbles: view.bubbles_enabled(),
                            lane_bubbles: view.lane_bubbles_enabled(),
                            lane_enabled: view.lane_enabled(),
                            status: status.code(),
                            grouping,
                            config: format!("{:?}", view.cached_config()),
                        }
                    }),
                })
                .collect(),
        })
        .collect()
}

/// The revision key's rows. Their only contract is [`Eq`]: they are never
/// serialized and never leave the registry. The two setups are compared
/// through their `Debug` rendering because both hold `f32` fields and so are
/// `PartialEq` but not `Eq`; the rendering is exact for every field.
#[derive(Clone, Debug, Eq, PartialEq)]
struct OrderflowRevisionKey {
    tab_id: u64,
    panes: Vec<PaneOrderflowRevisionKey>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PaneOrderflowRevisionKey {
    pane_id: u64,
    footprint_visible: bool,
    footprint_overridden: bool,
    footprint_setup: String,
    engine: Option<EngineRevisionKey>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct EngineRevisionKey {
    enabled: bool,
    depth_visible: bool,
    lane_depth_visible: bool,
    bubbles: bool,
    lane_bubbles: bool,
    lane_enabled: bool,
    status: &'static str,
    grouping: rust_decimal::Decimal,
    config: String,
}

fn project_tape<P: TabsPort + ?Sized>(app: &P, context: CaptureContext) -> TapeSnapshot {
    TapeSnapshot {
        tabs: app
            .tab_reads()
            .tabs()
            .iter_with_ids()
            .map(|(tab_id, tab)| TabTapeSnapshot {
                tab_id: WireU64::new(tab_id),
                panes: tab
                    .panes()
                    .map(|(pane, side)| PaneTapeSnapshot {
                        pane_id: WireU64::new(pane.id),
                        side: side.into(),
                        engine: engine_availability(pane),
                        tape: pane
                            .orderflow
                            .as_ref()
                            .map(|view| tape_state(tab, view, context)),
                    })
                    .collect(),
            })
            .collect(),
    }
}

fn tape_state(tab: &Tab, view: &OrderflowView, context: CaptureContext) -> TapeStateSnapshot {
    let health = view.cached_health();
    TapeStateSnapshot {
        enabled: health.enabled,
        aggression_provenance: AGGRESSION_PROVENANCE.to_owned(),
        last_event_unix_ms: health.last_event_ms,
        // Measured against the instant the capture was taken, which is the
        // only clock an age can be read off. Handing the newest event in as
        // "now" would compare it with itself and answer zero forever.
        age_ms: tab.tape_age_at(context.captured_at_unix_ms),
        live_end_unix_ms: view.cached_live_end_ms(),
        live_lane_enabled: view.lane_enabled(),
        live_lane_window: lane_window(view.live_lane_window()),
    }
}

fn project_footprint<P: TabsPort + ?Sized>(app: &P, _context: CaptureContext) -> FootprintSnapshot {
    let window = app.tab_reads().footprint_config();
    FootprintSnapshot {
        tabs: app
            .tab_reads()
            .tabs()
            .iter_with_ids()
            .map(|(tab_id, tab)| TabFootprintSnapshot {
                tab_id: WireU64::new(tab_id),
                panes: tab
                    .panes()
                    .map(|(pane, side)| PaneFootprintSnapshot {
                        pane_id: WireU64::new(pane.id),
                        side: side.into(),
                        visible: pane.footprint.visible,
                        overridden: pane.footprint.config.is_some(),
                        setup: footprint_setup(pane, window),
                    })
                    .collect(),
            })
            .collect(),
    }
}

fn footprint_setup(
    pane: &ChartPane,
    window: &crate::footprint_config::FootprintConfig,
) -> FootprintSetupSnapshot {
    let config = pane.footprint_config(window);
    FootprintSetupSnapshot {
        style: footprint_style_name(config.style).to_owned(),
        imbalance_ratio: canonical_decimal(config.imbalance_ratio),
        imbalance_minimum_quantity: config.imbalance_min_qty.map(canonical_decimal),
        stacked_count: wire_usize(config.stacked_count),
        show_point_of_control: config.show_poc,
        show_numbers: config.show_numbers,
        show_delta_totals: config.show_delta_totals,
    }
}

fn project_bubbles<P: TabsPort + ?Sized>(app: &P, _context: CaptureContext) -> BubblesSnapshot {
    BubblesSnapshot {
        tabs: app
            .tab_reads()
            .tabs()
            .iter_with_ids()
            .map(|(tab_id, tab)| TabBubblesSnapshot {
                tab_id: WireU64::new(tab_id),
                panes: tab
                    .panes()
                    .map(|(pane, side)| PaneBubblesSnapshot {
                        pane_id: WireU64::new(pane.id),
                        side: side.into(),
                        engine: engine_availability(pane),
                        bubbles: pane.orderflow.as_ref().map(|view| BubblesStateSnapshot {
                            enabled: view.bubbles_enabled(),
                            lane_enabled: view.lane_bubbles_enabled(),
                            aggression_provenance: AGGRESSION_PROVENANCE.to_owned(),
                            floored_quantity: canonical_decimal(
                                view.cached_health().floored_quantity,
                            ),
                        }),
                    })
                    .collect(),
            })
            .collect(),
    }
}

fn project_heatmap<P: TabsPort + ?Sized>(app: &P, _context: CaptureContext) -> HeatmapSnapshot {
    HeatmapSnapshot {
        tabs: app
            .tab_reads()
            .tabs()
            .iter_with_ids()
            .map(|(tab_id, tab)| TabHeatmapSnapshot {
                tab_id: WireU64::new(tab_id),
                panes: tab
                    .panes()
                    .map(|(pane, side)| PaneHeatmapSnapshot {
                        pane_id: WireU64::new(pane.id),
                        side: side.into(),
                        engine: engine_availability(pane),
                        heatmap: pane.orderflow.as_ref().map(heatmap_state),
                    })
                    .collect(),
            })
            .collect(),
    }
}

fn heatmap_state(view: &OrderflowView) -> HeatmapStateSnapshot {
    let config = view.cached_config();
    let (_status, _ladder, grouping) = view.cached_book();
    HeatmapStateSnapshot {
        visible: view.depth_visible(),
        lane_visible: view.lane_depth_visible(),
        retention_ms: config.retention_ms,
        capture_price_grouping: canonical_decimal(grouping),
        display_grouping: display_grouping_name(config.display_grouping),
        opacity: canonical_f32(config.opacity, SETTING_DECIMAL_PLACES),
        gamma: canonical_f32(config.gamma, SETTING_DECIMAL_PLACES),
        show_aggressions: config.show_aggressions,
    }
}

fn project_l2<P: TabsPort + ?Sized>(app: &P, _context: CaptureContext) -> L2Snapshot {
    L2Snapshot {
        tabs: app
            .tab_reads()
            .tabs()
            .iter_with_ids()
            .map(|(tab_id, tab)| TabL2Snapshot {
                tab_id: WireU64::new(tab_id),
                panes: tab
                    .panes()
                    .map(|(pane, side)| PaneL2Snapshot {
                        pane_id: WireU64::new(pane.id),
                        side: side.into(),
                        engine: engine_availability(pane),
                        book: pane.orderflow.as_ref().map(book_snapshot),
                    })
                    .collect(),
            })
            .collect(),
    }
}

fn book_snapshot(view: &OrderflowView) -> BookSnapshot {
    let (status, ladder, grouping) = view.cached_book();
    let best_bid = ladder.and_then(|ladder| ladder.best_bid).map(level);
    let best_ask = ladder.and_then(|ladder| ladder.best_ask).map(level);
    let bids = ladder.map(|ladder| ladder.bids.as_slice()).unwrap_or(&[]);
    let asks = ladder.map(|ladder| ladder.asks.as_slice()).unwrap_or(&[]);
    BookSnapshot {
        status: status.code().to_owned(),
        provenance: BOOK_PROVENANCE.to_owned(),
        price_grouping: canonical_decimal(grouping),
        spread: ladder
            .and_then(|ladder| ladder.best_bid.zip(ladder.best_ask))
            .map(|(bid, ask)| canonical_decimal(ask.price() - bid.price())),
        best_bid,
        best_ask,
        bids: bids
            .iter()
            .take(CONTROL_SNAPSHOT_MAX_BOOK_LEVELS_PER_SIDE)
            .map(|value| level(*value))
            .collect(),
        asks: asks
            .iter()
            .take(CONTROL_SNAPSHOT_MAX_BOOK_LEVELS_PER_SIDE)
            .map(|value| level(*value))
            .collect(),
        bids_truncated: bids.len() > CONTROL_SNAPSHOT_MAX_BOOK_LEVELS_PER_SIDE,
        asks_truncated: asks.len() > CONTROL_SNAPSHOT_MAX_BOOK_LEVELS_PER_SIDE,
    }
}

/// Whether this pane has an order-flow engine at all. A pane without one
/// reports the absence with a reason rather than an empty book, which would
/// read as "the venue is quoting nothing".
fn engine_availability(pane: &ChartPane) -> AvailabilitySnapshot {
    if pane.orderflow.is_some() {
        available()
    } else {
        unavailable(NO_ENGINE)
    }
}
