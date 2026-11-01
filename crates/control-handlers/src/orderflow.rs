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
//!
//! The window hands each pane over through [`OrderflowPane`]: its footprint
//! setup, its opening-scale rule, and — where it has one — its engine's
//! published frame as [`OrderflowEngine`]. Everything the five scopes decide
//! from those is here.

pub use quantick_control_schema::orderflow::*;

use quantick_control::{
    id::{ModuleId, SnapshotScopeId},
    registry::ModuleDescriptor,
    wire::WireU64,
};
use quantick_control_host::{
    projection::{CaptureContext, ProjectionRegistryError},
    wire::{AvailabilitySnapshot, PaneSideDto, available, unavailable},
};
use quantick_orderflow::{
    HeatmapConfig,
    engine::{BookLadder, CaptureStatus, OrderflowHealth},
};
use quantick_stores::footprint_config::FootprintConfig;

use crate::dock::ProjectionDock;

/// One pane's order-flow engine as its last published frame reads.
pub trait OrderflowEngine {
    /// The heatmap, bubble and tape setup the engine is drawn with.
    fn config(&self) -> &HeatmapConfig;
    /// The capture state, the clipped ladder and the capture grouping.
    fn book(&self) -> (&CaptureStatus, Option<&BookLadder>, rust_decimal::Decimal);
    /// The published health counters.
    fn health(&self) -> &OrderflowHealth;
    /// Where the live lane ends, when the tape knows.
    fn live_end_ms(&self) -> Option<i64>;
    /// The bubble layer as the bubble scope reports it.
    fn bubbles(&self) -> BubblesStateSnapshot;
    /// The flow execution's published frame, when one is drawn.
    fn flow_execution(&self) -> Option<FlowExecutionSnapshot>;
}

/// One chart pane as the order-flow scopes read it.
pub trait OrderflowPane {
    /// The pane's stable id.
    fn pane_id(&self) -> u64;
    /// Whether the footprint layer is drawn.
    fn footprint_visible(&self) -> bool;
    /// Whether the pane overrides the window's footprint setup.
    fn footprint_overridden(&self) -> bool;
    /// The footprint setup in force on the pane: its own, else the window's.
    fn footprint_config(&self) -> &FootprintConfig;
    /// The opening-scale rule, as the bubble scope reports it.
    fn opening_scale(&self) -> OpeningScaleSnapshot;
    /// How old the tab's tape is at `now_ms`, `None` on a replay.
    fn tape_age_at(&self, now_ms: i64) -> Option<i64>;
    /// The pane's engine, `None` when the pane has none.
    fn engine(&self) -> Option<&dyn OrderflowEngine>;
}

/// One pane with the side it sits on.
pub struct OrderflowPaneEntry<'a> {
    pub side: PaneSideDto,
    pub pane: &'a dyn OrderflowPane,
}

/// The window's tabs and panes as the order-flow scopes walk them.
pub trait OrderflowPort {
    /// Visit every tab in screen order with its panes in the tab's order.
    fn visit_orderflow_tabs(&self, visit: &mut dyn FnMut(u64, &[OrderflowPaneEntry<'_>]));
}

/// Dock the order-flow module and its five scopes.
pub fn register<D, H>(registry: &mut D) -> Result<(), ProjectionRegistryError>
where
    D: ProjectionDock<H>,
    H: OrderflowPort + ?Sized + 'static,
{
    let module_id = ModuleId::new(MODULE_ID).expect("static module ID is valid");
    registry.register_module(
        ModuleDescriptor {
            id: module_id.clone(),
            title: "Order flow".to_owned(),
            description: "The book, the tape, and the layers drawn over them.".to_owned(),
        },
        revision::<H>,
    )?;
    registry.register_scope(
        SnapshotScopeId::new(TAPE_SCOPE_ID).expect("static scope ID is valid"),
        module_id.clone(),
        SCHEMA_VERSION,
        "Tape",
        "Reports how current each pane's tape is, where its live lane ends, and where its aggressor side comes from.",
        &["observe", "observe.orderflow"],
        project_tape::<H>,
    )?;
    registry.register_scope(
        SnapshotScopeId::new(FOOTPRINT_SCOPE_ID).expect("static scope ID is valid"),
        module_id.clone(),
        SCHEMA_VERSION,
        "Footprint",
        "Reports whether the candle footprint layer is drawn and the setup it is drawn with.",
        &["observe", "observe.orderflow", "observe.chart"],
        project_footprint::<H>,
    )?;
    registry.register_scope(
        SnapshotScopeId::new(BUBBLES_SCOPE_ID).expect("static scope ID is valid"),
        module_id.clone(),
        SCHEMA_VERSION,
        "Aggression bubbles",
        "Reports whether aggression bubbles are drawn over the chart and the lane, whether they are drawn as Bookmap-style volume dots, and what the display floor keeps off the canvas.",
        &["observe", "observe.orderflow"],
        project_bubbles::<H>,
    )?;
    registry.register_scope(
        SnapshotScopeId::new(HEATMAP_SCOPE_ID).expect("static scope ID is valid"),
        module_id.clone(),
        SCHEMA_VERSION,
        "Depth heatmap",
        "Reports whether depth heat is drawn, at which capture and display grouping, and with which retention.",
        &["observe", "observe.orderflow"],
        project_heatmap::<H>,
    )?;
    registry.register_scope(
        SnapshotScopeId::new(L2_SCOPE_ID).expect("static scope ID is valid"),
        module_id,
        SCHEMA_VERSION,
        "Order book",
        "Reports the published book around the spread: best bid and ask, the clipped ladders, and the capture state.",
        &["observe", "observe.orderflow", "observe.market"],
        project_l2::<H>,
    )
}

/// Every tab's rows, built from one walk over the port.
fn per_tab<P, T>(app: &P, mut row: impl FnMut(u64, &[OrderflowPaneEntry<'_>]) -> T) -> Vec<T>
where
    P: OrderflowPort + ?Sized,
{
    let mut rows = Vec::new();
    app.visit_orderflow_tabs(&mut |tab_id, panes| rows.push(row(tab_id, panes)));
    rows
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
fn revision<P: OrderflowPort + ?Sized>(app: &P) -> Vec<OrderflowRevisionKey> {
    per_tab(app, |tab_id, panes| OrderflowRevisionKey {
        tab_id,
        panes: panes
            .iter()
            .map(|entry| {
                let pane = entry.pane;
                PaneOrderflowRevisionKey {
                    pane_id: pane.pane_id(),
                    footprint_visible: pane.footprint_visible(),
                    footprint_overridden: pane.footprint_overridden(),
                    footprint_setup: format!("{:?}", pane.footprint_config()),
                    engine: pane.engine().map(|view| {
                        let (status, _ladder, grouping) = view.book();
                        EngineRevisionKey::from_config(view.config(), status, grouping)
                    }),
                }
            })
            .collect(),
    })
}

fn project_tape<P: OrderflowPort + ?Sized>(app: &P, context: CaptureContext) -> TapeSnapshot {
    TapeSnapshot {
        tabs: per_tab(app, |tab_id, panes| TabTapeSnapshot {
            tab_id: WireU64::new(tab_id),
            panes: panes
                .iter()
                .map(|entry| PaneTapeSnapshot {
                    pane_id: WireU64::new(entry.pane.pane_id()),
                    side: entry.side,
                    engine: engine_availability(entry.pane),
                    tape: entry
                        .pane
                        .engine()
                        .map(|view| tape_state(entry.pane, view, context)),
                })
                .collect(),
        }),
    }
}

fn tape_state(
    pane: &dyn OrderflowPane,
    view: &dyn OrderflowEngine,
    context: CaptureContext,
) -> TapeStateSnapshot {
    TapeStateSnapshot::from_published(
        view.config(),
        view.health(),
        // Measured against the instant the capture was taken, which is the
        // only clock an age can be read off. Handing the newest event in as
        // "now" would compare it with itself and answer zero forever.
        pane.tape_age_at(context.captured_at_unix_ms),
        view.live_end_ms(),
    )
}

fn project_footprint<P: OrderflowPort + ?Sized>(
    app: &P,
    _context: CaptureContext,
) -> FootprintSnapshot {
    FootprintSnapshot {
        tabs: per_tab(app, |tab_id, panes| TabFootprintSnapshot {
            tab_id: WireU64::new(tab_id),
            panes: panes
                .iter()
                .map(|entry| PaneFootprintSnapshot {
                    pane_id: WireU64::new(entry.pane.pane_id()),
                    side: entry.side,
                    visible: entry.pane.footprint_visible(),
                    overridden: entry.pane.footprint_overridden(),
                    setup: entry.pane.footprint_config().into(),
                })
                .collect(),
        }),
    }
}

fn project_bubbles<P: OrderflowPort + ?Sized>(
    app: &P,
    _context: CaptureContext,
) -> BubblesSnapshot {
    BubblesSnapshot {
        tabs: per_tab(app, |tab_id, panes| TabBubblesSnapshot {
            tab_id: WireU64::new(tab_id),
            panes: panes
                .iter()
                .map(|entry| PaneBubblesSnapshot {
                    opening_scale: entry.pane.opening_scale(),
                    flow_execution: entry
                        .pane
                        .engine()
                        .and_then(OrderflowEngine::flow_execution),
                    pane_id: WireU64::new(entry.pane.pane_id()),
                    side: entry.side,
                    engine: engine_availability(entry.pane),
                    bubbles: entry.pane.engine().map(OrderflowEngine::bubbles),
                })
                .collect(),
        }),
    }
}

fn project_heatmap<P: OrderflowPort + ?Sized>(
    app: &P,
    _context: CaptureContext,
) -> HeatmapSnapshot {
    HeatmapSnapshot {
        tabs: per_tab(app, |tab_id, panes| TabHeatmapSnapshot {
            tab_id: WireU64::new(tab_id),
            panes: panes
                .iter()
                .map(|entry| PaneHeatmapSnapshot {
                    pane_id: WireU64::new(entry.pane.pane_id()),
                    side: entry.side,
                    engine: engine_availability(entry.pane),
                    heatmap: entry.pane.engine().map(heatmap_state),
                })
                .collect(),
        }),
    }
}

fn heatmap_state(view: &dyn OrderflowEngine) -> HeatmapStateSnapshot {
    let (_status, _ladder, grouping) = view.book();
    HeatmapStateSnapshot::from_config(view.config(), grouping)
}

fn project_l2<P: OrderflowPort + ?Sized>(app: &P, _context: CaptureContext) -> L2Snapshot {
    L2Snapshot {
        tabs: per_tab(app, |tab_id, panes| TabL2Snapshot {
            tab_id: WireU64::new(tab_id),
            panes: panes
                .iter()
                .map(|entry| PaneL2Snapshot {
                    pane_id: WireU64::new(entry.pane.pane_id()),
                    side: entry.side,
                    engine: engine_availability(entry.pane),
                    book: entry.pane.engine().map(book_snapshot),
                })
                .collect(),
        }),
    }
}

fn book_snapshot(view: &dyn OrderflowEngine) -> BookSnapshot {
    let (status, ladder, grouping) = view.book();
    BookSnapshot::from_ladder(status, ladder, grouping)
}

/// Whether this pane has an order-flow engine at all. A pane without one
/// reports the absence with a reason rather than an empty book, which would
/// read as "the venue is quoting nothing".
fn engine_availability(pane: &dyn OrderflowPane) -> AvailabilitySnapshot {
    if pane.engine().is_some() {
        available()
    } else {
        unavailable(NO_ENGINE)
    }
}
