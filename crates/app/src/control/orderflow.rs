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

use quantick_control::{
    id::{ModuleId, SnapshotScopeId},
    limits::CONTROL_SNAPSHOT_MAX_BOOK_LEVELS_PER_SIDE,
    registry::ModuleDescriptor,
    wire::{CanonicalDecimal, WireU64},
};
use quantick_orderbook::BookLevel;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    app::QuantickApp,
    orderflow::{DisplayGrouping, LaneWindow},
    orderflow_view::OrderflowView,
    pane::{ChartPane, PaneSide},
    tab::Tab,
};

use super::{
    interaction::AvailabilitySnapshot,
    registry::{CaptureContext, ProjectionRegistry, ProjectionRegistryError},
    types::{PaneSideDto, canonical_decimal, canonical_f32, wire_usize},
};

pub(crate) const TAPE_SCOPE_ID: &str = "orderflow.tape";
pub(crate) const FOOTPRINT_SCOPE_ID: &str = "orderflow.footprint";
pub(crate) const BUBBLES_SCOPE_ID: &str = "orderflow.bubbles";
pub(crate) const HEATMAP_SCOPE_ID: &str = "orderflow.heatmap";
pub(crate) const L2_SCOPE_ID: &str = "orderflow.l2";
const MODULE_ID: &str = "orderflow";
const SCHEMA_VERSION: u32 = 1;
/// Opacity, gamma and the size scales are UI floats in a small range; six
/// places is the resolution `health.rs` publishes its own metrics at.
const SETTING_DECIMAL_PLACES: u32 = 6;
/// Where the aggressor side on this tape comes from. The feed scope owns the
/// per-market declaration; this names the rule so a reader of the tape scope
/// alone is not left guessing whether a side was reported or derived.
const AGGRESSION_PROVENANCE: &str = "venue_reported_side_or_tick_rule";
/// What a book level is: a price the venue published a resting quantity at.
/// Not an inference, and not a trade.
const BOOK_PROVENANCE: &str = "venue_published_depth";

/// A pane with no order-flow engine attached reports this rather than an empty
/// book, an empty tape or a zeroed setup.
const NO_ENGINE: &str = "order_flow_engine_not_attached_to_this_pane";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct TapeSnapshot {
    pub tabs: Vec<TabTapeSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct TabTapeSnapshot {
    pub tab_id: WireU64,
    pub panes: Vec<PaneTapeSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct PaneTapeSnapshot {
    pub pane_id: WireU64,
    pub side: PaneSideDto,
    pub engine: AvailabilitySnapshot,
    pub tape: Option<TapeStateSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct TapeStateSnapshot {
    pub enabled: bool,
    /// Where the aggressor side comes from on this tape.
    pub aggression_provenance: String,
    /// Venue time of the newest print the order-flow engine has seen.
    #[schemars(extend("x-unit" = "unix_milliseconds"))]
    pub last_event_unix_ms: Option<i64>,
    /// How far behind the newest print the tape is running. Absent when the
    /// tape has nothing to be behind.
    #[schemars(extend("x-unit" = "milliseconds"))]
    pub age_ms: Option<i64>,
    /// The right edge of the live lane, in venue time.
    #[schemars(extend("x-unit" = "unix_milliseconds"))]
    pub live_end_unix_ms: Option<i64>,
    /// The live lane is drawn, and how its window is chosen.
    pub live_lane_enabled: bool,
    pub live_lane_window: LaneWindowSnapshot,
}

/// How wide the live lane's window is, said the way the setting says it. An
/// `auto` window follows the recent bars' typical duration, so publishing one
/// resolved number would report a measurement as a setting.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct LaneWindowSnapshot {
    /// `auto` or `fixed`.
    pub mode: String,
    /// Present for `auto`: `1.0` fits about one bar's worth of market time.
    #[schemars(extend("x-unit" = "ratio"))]
    pub zoom: Option<CanonicalDecimal>,
    /// Present for `fixed`: the exchange milliseconds shown in the band.
    #[schemars(extend("x-unit" = "milliseconds"))]
    pub fixed_ms: Option<i64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct FootprintSnapshot {
    pub tabs: Vec<TabFootprintSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct TabFootprintSnapshot {
    pub tab_id: WireU64,
    pub panes: Vec<PaneFootprintSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct PaneFootprintSnapshot {
    pub pane_id: WireU64,
    pub side: PaneSideDto,
    /// The candle footprint layer is drawn on this chart.
    pub visible: bool,
    /// True when this chart carries its own footprint setup rather than the
    /// window's shared one.
    pub overridden: bool,
    pub setup: FootprintSetupSnapshot,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct FootprintSetupSnapshot {
    /// `bid_ask`, `delta`, `profile` or `auto` — how each ladder is read.
    pub style: String,
    /// A level is imbalanced when one side exceeds the diagonal other side by
    /// this factor.
    pub imbalance_ratio: CanonicalDecimal,
    /// Quantity a level must reach before the ratio is applied at all, so a
    /// one-lot against nothing is not called an imbalance.
    pub imbalance_minimum_quantity: Option<CanonicalDecimal>,
    /// Consecutive imbalanced levels that make a stacked zone.
    pub stacked_count: WireU64,
    pub show_point_of_control: bool,
    pub show_numbers: bool,
    pub show_delta_totals: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct BubblesSnapshot {
    pub tabs: Vec<TabBubblesSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct TabBubblesSnapshot {
    pub tab_id: WireU64,
    pub panes: Vec<PaneBubblesSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct PaneBubblesSnapshot {
    pub pane_id: WireU64,
    pub side: PaneSideDto,
    pub engine: AvailabilitySnapshot,
    pub bubbles: Option<BubblesStateSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct BubblesStateSnapshot {
    /// Aggression bubbles are drawn over the chart.
    pub enabled: bool,
    /// And over the live lane, which is a separate switch.
    pub lane_enabled: bool,
    /// Where the aggression these bubbles draw comes from.
    pub aggression_provenance: String,
    /// The exact quantity the trader's own display floor keeps off the canvas.
    /// Floored, not dropped: it is still in the totals.
    pub floored_quantity: CanonicalDecimal,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct HeatmapSnapshot {
    pub tabs: Vec<TabHeatmapSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct TabHeatmapSnapshot {
    pub tab_id: WireU64,
    pub panes: Vec<PaneHeatmapSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct PaneHeatmapSnapshot {
    pub pane_id: WireU64,
    pub side: PaneSideDto,
    pub engine: AvailabilitySnapshot,
    pub heatmap: Option<HeatmapStateSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct HeatmapStateSnapshot {
    /// Depth heat is drawn over the chart.
    pub visible: bool,
    /// And over the live lane, which is a separate switch.
    pub lane_visible: bool,
    /// How long a resting level stays on the canvas after it leaves the book.
    #[schemars(extend("x-unit" = "milliseconds"))]
    pub retention_ms: i64,
    /// The bucket the engine is capturing at. The auto-base logic can move it
    /// with no user action, so it is mirrored rather than assumed.
    pub capture_price_grouping: CanonicalDecimal,
    /// How the captured buckets are merged for display: `native` or a
    /// multiple of the capture bucket.
    pub display_grouping: String,
    #[schemars(extend("x-unit" = "ratio"))]
    pub opacity: Option<CanonicalDecimal>,
    #[schemars(extend("x-unit" = "ratio"))]
    pub gamma: Option<CanonicalDecimal>,
    pub show_aggressions: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct L2Snapshot {
    pub tabs: Vec<TabL2Snapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct TabL2Snapshot {
    pub tab_id: WireU64,
    pub panes: Vec<PaneL2Snapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct PaneL2Snapshot {
    pub pane_id: WireU64,
    pub side: PaneSideDto,
    pub engine: AvailabilitySnapshot,
    pub book: Option<BookSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct BookSnapshot {
    /// `disabled`, `connecting`, `buffering`, `syncing`, `live`, `error` — the
    /// capture state, as a code rather than the badge's prose.
    pub status: String,
    /// What these levels are.
    pub provenance: String,
    /// The bucket the levels were captured at.
    pub price_grouping: CanonicalDecimal,
    /// Best bid and ask of the *whole* book, even when they sit outside the
    /// window the ladders below were clipped to.
    pub best_bid: Option<BookLevelSnapshot>,
    pub best_ask: Option<BookLevelSnapshot>,
    /// Distance between the two best prices, when both exist.
    pub spread: Option<CanonicalDecimal>,
    /// Bid levels best-first, descending price.
    #[schemars(length(max = CONTROL_SNAPSHOT_MAX_BOOK_LEVELS_PER_SIDE))]
    pub bids: Vec<BookLevelSnapshot>,
    /// Ask levels best-first, ascending price.
    #[schemars(length(max = CONTROL_SNAPSHOT_MAX_BOOK_LEVELS_PER_SIDE))]
    pub asks: Vec<BookLevelSnapshot>,
    pub bids_truncated: bool,
    pub asks_truncated: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct BookLevelSnapshot {
    pub price: CanonicalDecimal,
    pub quantity: CanonicalDecimal,
}

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
fn revision(app: &QuantickApp) -> Vec<OrderflowRevisionKey> {
    app.control_tabs()
        .iter()
        .map(|tab| OrderflowRevisionKey {
            tab_id: tab.id,
            panes: panes(tab)
                .into_iter()
                .map(|(pane, _side)| PaneOrderflowRevisionKey {
                    pane_id: pane.id,
                    footprint_visible: pane.footprint_visible,
                    footprint_overridden: pane.footprint_override.is_some(),
                    footprint_setup: format!(
                        "{:?}",
                        pane.footprint_config(app.control_footprint_config())
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

fn project_tape(app: &QuantickApp, _context: CaptureContext) -> TapeSnapshot {
    TapeSnapshot {
        tabs: app
            .control_tabs()
            .iter()
            .map(|tab| TabTapeSnapshot {
                tab_id: WireU64::new(tab.id),
                panes: panes(tab)
                    .into_iter()
                    .map(|(pane, side)| PaneTapeSnapshot {
                        pane_id: WireU64::new(pane.id),
                        side: side.into(),
                        engine: engine_availability(pane),
                        tape: pane.orderflow.as_ref().map(|view| tape_state(tab, view)),
                    })
                    .collect(),
            })
            .collect(),
    }
}

fn tape_state(tab: &Tab, view: &OrderflowView) -> TapeStateSnapshot {
    let health = view.cached_health();
    TapeStateSnapshot {
        enabled: health.enabled,
        aggression_provenance: AGGRESSION_PROVENANCE.to_owned(),
        last_event_unix_ms: health.last_event_ms,
        age_ms: health
            .last_event_ms
            .and_then(|newest| tab.tape_age_at(newest).map(|age| age.max(0))),
        live_end_unix_ms: health.last_event_ms,
        live_lane_enabled: view.lane_enabled(),
        live_lane_window: lane_window(view.live_lane_window()),
    }
}

fn project_footprint(app: &QuantickApp, _context: CaptureContext) -> FootprintSnapshot {
    let window = app.control_footprint_config();
    FootprintSnapshot {
        tabs: app
            .control_tabs()
            .iter()
            .map(|tab| TabFootprintSnapshot {
                tab_id: WireU64::new(tab.id),
                panes: panes(tab)
                    .into_iter()
                    .map(|(pane, side)| PaneFootprintSnapshot {
                        pane_id: WireU64::new(pane.id),
                        side: side.into(),
                        visible: pane.footprint_visible,
                        overridden: pane.footprint_override.is_some(),
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
        style: format!("{:?}", config.style).to_lowercase(),
        imbalance_ratio: canonical_decimal(config.imbalance_ratio),
        imbalance_minimum_quantity: config.imbalance_min_qty.map(canonical_decimal),
        stacked_count: wire_usize(config.stacked_count),
        show_point_of_control: config.show_poc,
        show_numbers: config.show_numbers,
        show_delta_totals: config.show_delta_totals,
    }
}

fn project_bubbles(app: &QuantickApp, _context: CaptureContext) -> BubblesSnapshot {
    BubblesSnapshot {
        tabs: app
            .control_tabs()
            .iter()
            .map(|tab| TabBubblesSnapshot {
                tab_id: WireU64::new(tab.id),
                panes: panes(tab)
                    .into_iter()
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

fn project_heatmap(app: &QuantickApp, _context: CaptureContext) -> HeatmapSnapshot {
    HeatmapSnapshot {
        tabs: app
            .control_tabs()
            .iter()
            .map(|tab| TabHeatmapSnapshot {
                tab_id: WireU64::new(tab.id),
                panes: panes(tab)
                    .into_iter()
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

fn project_l2(app: &QuantickApp, _context: CaptureContext) -> L2Snapshot {
    L2Snapshot {
        tabs: app
            .control_tabs()
            .iter()
            .map(|tab| TabL2Snapshot {
                tab_id: WireU64::new(tab.id),
                panes: panes(tab)
                    .into_iter()
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

fn level(value: BookLevel) -> BookLevelSnapshot {
    BookLevelSnapshot {
        price: canonical_decimal(value.price()),
        quantity: canonical_decimal(value.quantity()),
    }
}

/// Whether this pane has an order-flow engine at all. A pane without one
/// reports the absence with a reason rather than an empty book, which would
/// read as "the venue is quoting nothing".
fn engine_availability(pane: &ChartPane) -> AvailabilitySnapshot {
    if pane.orderflow.is_some() {
        AvailabilitySnapshot::available()
    } else {
        AvailabilitySnapshot::unavailable(NO_ENGINE)
    }
}

fn display_grouping_name(grouping: DisplayGrouping) -> String {
    match grouping {
        DisplayGrouping::Native => "native".to_owned(),
        DisplayGrouping::Multiple(multiple) => format!("multiple_{multiple}"),
        DisplayGrouping::Adaptive { target_rows } => format!("adaptive_{target_rows}"),
    }
}

fn lane_window(window: LaneWindow) -> LaneWindowSnapshot {
    match window {
        LaneWindow::Auto { zoom } => LaneWindowSnapshot {
            mode: "auto".to_owned(),
            zoom: canonical_f32(zoom, SETTING_DECIMAL_PLACES),
            fixed_ms: None,
        },
        LaneWindow::Fixed { ms } => LaneWindowSnapshot {
            mode: "fixed".to_owned(),
            zoom: None,
            fixed_ms: Some(ms),
        },
    }
}

/// The panes of one tab, in a stable order so a capture is diffable against
/// the one before it.
fn panes(tab: &Tab) -> Vec<(&ChartPane, PaneSide)> {
    let mut panes = vec![(&tab.flow_pane, PaneSide::Flow)];
    if let Some(time) = &tab.time_pane {
        panes.push((time, PaneSide::Time));
    }
    panes
}
