//! Order-flow projections — the book, the tape, and the three layers drawn
//! over them.
//!
//! Six scopes under one owner module. Every one of them reads the frame the
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
//! published frame as [`OrderflowEngine`]. Everything the scopes decide from
//! those is here. The sixth, the settings panel's state, reads the tab and not
//! a pane, painted or not.

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
use quantick_control_schema::orderflow_settings::{
    AssetFolds, OrderFlowPanel, SETTINGS_SCOPE_ID, SettingsSnapshot, TabSettingsSnapshot,
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
    /// The capture state, the clipped ladder, the capture grouping and the gaps
    /// of the frame last painted.
    fn book(
        &self,
    ) -> (
        &CaptureStatus,
        Option<&BookLadder>,
        rust_decimal::Decimal,
        &[quantick_orderflow::projection::GapPrimitive],
    );
    /// The published health counters.
    fn health(&self) -> &OrderflowHealth;
    /// Where the live lane ends, when the tape knows.
    fn live_end_ms(&self) -> Option<i64>;
    /// Where a hold has stopped the heat while the book goes on arriving.
    fn book_held_at_ms(&self) -> Option<i64>;
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

/// What sees each tab's panel: its id, its asset, its panel, the window's folds.
pub type PanelVisit<'a> = dyn FnMut(u64, &str, &OrderFlowPanel, &AssetFolds) + 'a;

/// One pane with the side it sits on.
pub struct OrderflowPaneEntry<'a> {
    pub side: PaneSideDto,
    pub pane: &'a dyn OrderflowPane,
}

/// The window's tabs and panes as the order-flow scopes walk them.
pub trait OrderflowPort {
    /// Visit every tab in screen order with its panes in the tab's order.
    fn visit_orderflow_tabs(&self, visit: &mut dyn FnMut(u64, &[OrderflowPaneEntry<'_>]));
    /// Visit every tab in screen order with the asset it shows, its order-flow
    /// panel and the window's section folds, whatever was painted.
    fn visit_orderflow_panels(&self, visit: &mut PanelVisit<'_>);
}

/// Dock the order-flow module and its six scopes.
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
        module_id.clone(),
        SCHEMA_VERSION,
        "Order book",
        "Reports the published book around the spread: best bid and ask, the clipped ladders, and the capture state.",
        &["observe", "observe.orderflow", "observe.market"],
        project_l2::<H>,
    )?;
    registry.register_scope(
        SnapshotScopeId::new(SETTINGS_SCOPE_ID).expect("static scope ID is valid"),
        module_id,
        SCHEMA_VERSION,
        "Order-flow settings panel",
        "Reports whether each tab's order-flow settings panel is open and which of its sections the tab's asset keeps open, whether or not the panel was painted.",
        &["observe", "observe.orderflow"],
        project_settings::<H>,
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
fn revision<P: OrderflowPort + ?Sized>(
    app: &P,
) -> (Vec<OrderflowRevisionKey>, Vec<TabSettingsSnapshot>) {
    let engines = per_tab(app, |tab_id, panes| OrderflowRevisionKey {
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
                        let (status, _ladder, grouping, gaps) = view.book();
                        EngineRevisionKey::from_config(view.config(), status, grouping)
                            .with_book(gaps, view.book_held_at_ms())
                    }),
                }
            })
            .collect(),
    });
    (engines, settings_rows(app))
}

fn settings_rows<P: OrderflowPort + ?Sized>(app: &P) -> Vec<TabSettingsSnapshot> {
    let mut rows = Vec::new();
    app.visit_orderflow_panels(&mut |tab_id, asset, panel, folds| {
        rows.push(TabSettingsSnapshot::read(
            WireU64::new(tab_id),
            asset,
            panel,
            folds,
        ));
    });
    rows
}

fn project_settings<P: OrderflowPort + ?Sized>(
    app: &P,
    _context: CaptureContext,
) -> SettingsSnapshot {
    SettingsSnapshot {
        tabs: settings_rows(app),
    }
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
    let (_status, _ladder, grouping, gaps) = view.book();
    HeatmapStateSnapshot {
        gaps: quantick_control_schema::orderflow::book_gap_rows(gaps),
        book_held_at_unix_ms: view.book_held_at_ms(),
        ..HeatmapStateSnapshot::from_config(view.config(), grouping)
    }
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
    let (status, ladder, grouping, _gaps) = view.book();
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

#[cfg(test)]
mod tests {
    use super::*;
    use quantick_control_schema::orderflow_settings::PanelSection;
    use quantick_orderflow::projection::GapPrimitive;
    use std::cell::Cell;

    /// One tab with one pane, on the facts these tests move.
    #[derive(Default)]
    struct Fake {
        config: HeatmapConfig,
        status: Option<CaptureStatus>,
        footprint: FootprintConfig,
        gaps: Vec<GapPrimitive>,
        held: Cell<Option<i64>>,
        panel: OrderFlowPanel,
        folds: AssetFolds,
    }

    impl OrderflowEngine for Fake {
        fn config(&self) -> &HeatmapConfig {
            &self.config
        }
        fn book(
            &self,
        ) -> (
            &CaptureStatus,
            Option<&BookLadder>,
            rust_decimal::Decimal,
            &[GapPrimitive],
        ) {
            let status = self.status.as_ref().expect("the fake has a status");
            (status, None, rust_decimal::Decimal::ONE, &self.gaps)
        }
        fn health(&self) -> &OrderflowHealth {
            unreachable!("the revision and these scopes read no health")
        }
        fn live_end_ms(&self) -> Option<i64> {
            None
        }
        fn book_held_at_ms(&self) -> Option<i64> {
            self.held.get()
        }
        fn bubbles(&self) -> BubblesStateSnapshot {
            unreachable!("the revision and these scopes read no bubbles")
        }
        fn flow_execution(&self) -> Option<FlowExecutionSnapshot> {
            None
        }
    }

    impl OrderflowPane for Fake {
        fn pane_id(&self) -> u64 {
            7
        }
        fn footprint_visible(&self) -> bool {
            false
        }
        fn footprint_overridden(&self) -> bool {
            false
        }
        fn footprint_config(&self) -> &FootprintConfig {
            &self.footprint
        }
        fn opening_scale(&self) -> OpeningScaleSnapshot {
            unreachable!("the revision and these scopes read no opening scale")
        }
        fn tape_age_at(&self, _now_ms: i64) -> Option<i64> {
            None
        }
        fn engine(&self) -> Option<&dyn OrderflowEngine> {
            Some(self)
        }
    }

    impl OrderflowPort for Fake {
        fn visit_orderflow_tabs(&self, visit: &mut dyn FnMut(u64, &[OrderflowPaneEntry<'_>])) {
            let side = PaneSideDto::Flow;
            visit(1, &[OrderflowPaneEntry { side, pane: self }]);
        }
        fn visit_orderflow_panels(&self, visit: &mut PanelVisit<'_>) {
            visit(1, "WIN", &self.panel, &self.folds);
        }
    }

    fn fake() -> Fake {
        Fake {
            status: Some(CaptureStatus::Connecting),
            ..Fake::default()
        }
    }

    #[test]
    fn the_revision_moves_with_the_books_gaps_and_a_held_book() {
        let mut port = fake();
        let before = revision(&port);
        assert!(before == revision(&port));
        port.gaps.push(GapPrimitive {
            from_generation: None,
            to_generation: None,
            x0: 0.0,
            x1: 0.1,
            reason: "sequence_gap".to_owned(),
            lead_span: None,
        });
        let gapped = revision(&port);
        assert!(gapped != before, "a gap the heatmap scope reports");
        port.held.set(Some(5_000));
        assert!(revision(&port) != gapped, "a book held");
    }

    #[test]
    fn the_settings_scope_reads_the_tab_and_the_revision_moves_with_it() {
        let mut port = fake();
        let before = revision(&port);
        port.panel.open_on(None);
        port.folds.set("WIN", PanelSection::L2, false);
        assert!(revision(&port) != before);
        let rows = settings_rows(&port);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].asset, "WIN");
        assert!(rows[0].open && !rows[0].l2_open && rows[0].bubbles_open);
    }

    #[test]
    fn the_heatmap_scope_carries_the_book_held() {
        let port = fake();
        assert_eq!(heatmap_state(&port).book_held_at_unix_ms, None);
        port.held.set(Some(5_000));
        assert_eq!(heatmap_state(&port).book_held_at_unix_ms, Some(5_000));
    }
}
