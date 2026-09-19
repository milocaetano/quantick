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

use quantick_control_host::wire::{
    AvailabilitySnapshot, PaneSideDto, canonical_decimal, canonical_f32,
};
use quantick_stores::footprint_config::FootprintStyle;

use quantick_control::{
    limits::CONTROL_SNAPSHOT_MAX_BOOK_LEVELS_PER_SIDE,
    wire::{CanonicalDecimal, WireU64},
};

use quantick_orderbook::BookLevel;

use schemars::JsonSchema;

use serde::{Deserialize, Serialize};

use quantick_orderflow::{DisplayGrouping, LaneWindow};

pub const TAPE_SCOPE_ID: &str = "orderflow.tape";

pub const FOOTPRINT_SCOPE_ID: &str = "orderflow.footprint";

pub const BUBBLES_SCOPE_ID: &str = "orderflow.bubbles";

pub const HEATMAP_SCOPE_ID: &str = "orderflow.heatmap";

pub const L2_SCOPE_ID: &str = "orderflow.l2";

pub const MODULE_ID: &str = "orderflow";

pub const SCHEMA_VERSION: u32 = 1;

/// Opacity, gamma and the size scales are UI floats in a small range; six
/// places is the resolution `health.rs` publishes its own metrics at.
pub const SETTING_DECIMAL_PLACES: u32 = 6;

/// Where the aggressor side on this tape comes from. The feed scope owns the
/// per-market declaration; this names the rule so a reader of the tape scope
/// alone is not left guessing whether a side was reported or derived.
pub const AGGRESSION_PROVENANCE: &str = "venue_reported_side_or_tick_rule";

/// What a book level is: a price the venue published a resting quantity at.
/// Not an inference, and not a trade.
pub const BOOK_PROVENANCE: &str = "venue_published_depth";

/// A pane with no order-flow engine attached reports this rather than an empty
/// book, an empty tape or a zeroed setup.
pub const NO_ENGINE: &str = "order_flow_engine_not_attached_to_this_pane";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TapeSnapshot {
    pub tabs: Vec<TabTapeSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TabTapeSnapshot {
    pub tab_id: WireU64,
    pub panes: Vec<PaneTapeSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PaneTapeSnapshot {
    pub pane_id: WireU64,
    pub side: PaneSideDto,
    pub engine: AvailabilitySnapshot,
    pub tape: Option<TapeStateSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TapeStateSnapshot {
    pub enabled: bool,
    /// Where the aggressor side comes from on this tape.
    pub aggression_provenance: String,
    /// Venue time of the newest print the order-flow engine has seen.
    #[schemars(extend("x-unit" = "unix_milliseconds"))]
    pub last_event_unix_ms: Option<i64>,
    /// How far behind the newest print the tape was running at the instant
    /// this capture was taken. Absent while replaying — a recording is played
    /// on its own clock and is never late — and before anything has arrived.
    #[schemars(extend("x-unit" = "milliseconds"))]
    pub age_ms: Option<i64>,
    /// The right edge of the live lane, in venue time. Absent when no flow
    /// layer is drawn: a chart with no lane has no edge, and the newest print
    /// the engine happens to have seen is not one.
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
pub struct LaneWindowSnapshot {
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
pub struct FootprintSnapshot {
    pub tabs: Vec<TabFootprintSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TabFootprintSnapshot {
    pub tab_id: WireU64,
    pub panes: Vec<PaneFootprintSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PaneFootprintSnapshot {
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
pub struct FootprintSetupSnapshot {
    /// `split`, `ladder`, `bid_ask`, `cluster` or `auto` — how each ladder is
    /// read. `auto` is not a look of its own: it picks the richest reading the
    /// zoom can pay for, again on every frame.
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
pub struct BubblesSnapshot {
    pub tabs: Vec<TabBubblesSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TabBubblesSnapshot {
    pub tab_id: WireU64,
    pub panes: Vec<PaneBubblesSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PaneBubblesSnapshot {
    pub pane_id: WireU64,
    pub side: PaneSideDto,
    pub engine: AvailabilitySnapshot,
    pub bubbles: Option<BubblesStateSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BubblesStateSnapshot {
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
pub struct HeatmapSnapshot {
    pub tabs: Vec<TabHeatmapSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TabHeatmapSnapshot {
    pub tab_id: WireU64,
    pub panes: Vec<PaneHeatmapSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PaneHeatmapSnapshot {
    pub pane_id: WireU64,
    pub side: PaneSideDto,
    pub engine: AvailabilitySnapshot,
    pub heatmap: Option<HeatmapStateSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HeatmapStateSnapshot {
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
    /// How the captured buckets are merged for display: `native`,
    /// `multiple_<n>` for a fixed multiple of the capture bucket, or
    /// `adaptive_<rows>` — the default — which picks a multiple near that row
    /// count for the visible price window.
    pub display_grouping: String,
    #[schemars(extend("x-unit" = "ratio"))]
    pub opacity: Option<CanonicalDecimal>,
    #[schemars(extend("x-unit" = "ratio"))]
    pub gamma: Option<CanonicalDecimal>,
    pub show_aggressions: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct L2Snapshot {
    pub tabs: Vec<TabL2Snapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TabL2Snapshot {
    pub tab_id: WireU64,
    pub panes: Vec<PaneL2Snapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PaneL2Snapshot {
    pub pane_id: WireU64,
    pub side: PaneSideDto,
    pub engine: AvailabilitySnapshot,
    pub book: Option<BookSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BookSnapshot {
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
pub struct BookLevelSnapshot {
    pub price: CanonicalDecimal,
    pub quantity: CanonicalDecimal,
}

pub fn level(value: BookLevel) -> BookLevelSnapshot {
    BookLevelSnapshot {
        price: canonical_decimal(value.price()),
        quantity: canonical_decimal(value.quantity()),
    }
}

pub fn display_grouping_name(grouping: DisplayGrouping) -> String {
    match grouping {
        DisplayGrouping::Native => "native".to_owned(),
        DisplayGrouping::Multiple(multiple) => format!("multiple_{multiple}"),
        DisplayGrouping::Adaptive { target_rows } => format!("adaptive_{target_rows}"),
    }
}

/// The wire name of a footprint style. Owned here rather than derived from
/// `Debug`: the vocabulary belongs to the control plane's contract, and a
/// `{:?}` rendering silently republishes every future rename and every new
/// variant as a name no client was told about.
pub const fn footprint_style_name(style: FootprintStyle) -> &'static str {
    match style {
        FootprintStyle::Split => "split",
        FootprintStyle::Ladder => "ladder",
        FootprintStyle::BidAsk => "bid_ask",
        FootprintStyle::Cluster => "cluster",
        FootprintStyle::Auto => "auto",
    }
}

pub fn lane_window(window: LaneWindow) -> LaneWindowSnapshot {
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
