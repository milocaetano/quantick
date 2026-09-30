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
    registry::ModuleDescriptor,
    wire::WireU64,
};

use crate::{orderflow_view::OrderflowView, pane::ChartPane, tab::Tab};

use super::{
    registry::{CaptureContext, ProjectionRegistry, ProjectionRegistryError},
    types::{AvailabilitySnapshot, available, unavailable},
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
        "Reports whether aggression bubbles are drawn over the chart and the lane, whether they are drawn as Bookmap-style volume dots, and what the display floor keeps off the canvas.",
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
                        EngineRevisionKey::from_config(view.cached_config(), status, grouping)
                    }),
                })
                .collect(),
        })
        .collect()
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
    TapeStateSnapshot::from_published(
        view.cached_config(),
        view.cached_health(),
        // Measured against the instant the capture was taken, which is the
        // only clock an age can be read off. Handing the newest event in as
        // "now" would compare it with itself and answer zero forever.
        tab.tape_age_at(context.captured_at_unix_ms),
        view.cached_live_end_ms(),
    )
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
                        setup: pane.footprint_config(window).into(),
                    })
                    .collect(),
            })
            .collect(),
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
                        candle_aggression: pane.footprint.candle_aggression().map(Into::into),
                        pane_id: WireU64::new(pane.id),
                        side: side.into(),
                        engine: engine_availability(pane),
                        bubbles: pane.orderflow.as_ref().map(OrderflowView::bubbles_snapshot),
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
    let (_status, _ladder, grouping) = view.cached_book();
    HeatmapStateSnapshot::from_config(view.cached_config(), grouping)
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
    BookSnapshot::from_ladder(status, ladder, grouping)
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
