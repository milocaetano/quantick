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

mod constants;
mod flow_execution;
pub use flow_execution::FlowExecutionSnapshot;

use quantick_control_host::wire::{
    AvailabilitySnapshot, PaneSideDto, canonical_decimal, canonical_f32, wire_usize,
};
use quantick_stores::footprint_config::{FootprintConfig, FootprintStyle};

use quantick_control::{
    limits::CONTROL_SNAPSHOT_MAX_BOOK_LEVELS_PER_SIDE,
    wire::{CanonicalDecimal, WireU64},
};

use quantick_orderbook::BookLevel;

use schemars::JsonSchema;

use serde::{Deserialize, Serialize};

use quantick_orderflow::{DisplayGrouping, HeatmapConfig, LaneWindow};

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

/// Revision rows are never serialized. The setup debug strings compare every
/// field exactly, including floats that prevent deriving `Eq` on the configs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OrderflowRevisionKey {
    pub tab_id: u64,
    pub panes: Vec<PaneOrderflowRevisionKey>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaneOrderflowRevisionKey {
    pub pane_id: u64,
    pub footprint_visible: bool,
    pub footprint_overridden: bool,
    pub footprint_setup: String,
    pub engine: Option<EngineRevisionKey>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EngineRevisionKey {
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

impl EngineRevisionKey {
    pub fn from_config(
        config: &HeatmapConfig,
        status: &quantick_orderflow::engine::CaptureStatus,
        grouping: rust_decimal::Decimal,
    ) -> Self {
        Self {
            enabled: config.enabled,
            depth_visible: config.depth_visible(),
            lane_depth_visible: config.lane_depth_visible(),
            bubbles: config.show_aggressions,
            lane_bubbles: config.lane_aggressions_visible(),
            lane_enabled: config.lane_enabled(),
            status: status.code(),
            grouping,
            config: format!("{config:?}"),
        }
    }
}

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

impl TapeStateSnapshot {
    pub fn from_published(
        config: &HeatmapConfig,
        health: &quantick_orderflow::engine::OrderflowHealth,
        age_ms: Option<i64>,
        live_end_unix_ms: Option<i64>,
    ) -> Self {
        Self {
            enabled: health.enabled,
            aggression_provenance: AGGRESSION_PROVENANCE.to_owned(),
            last_event_unix_ms: health.last_event_ms,
            age_ms,
            live_end_unix_ms,
            live_lane_enabled: config.lane_enabled(),
            live_lane_window: lane_window(config.lane_window()),
        }
    }
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

impl From<&FootprintConfig> for FootprintSetupSnapshot {
    fn from(config: &FootprintConfig) -> Self {
        Self {
            style: footprint_style_name(config.style).to_owned(),
            imbalance_ratio: canonical_decimal(config.imbalance_ratio),
            imbalance_minimum_quantity: config.imbalance_min_qty.map(canonical_decimal),
            stacked_count: wire_usize(config.stacked_count),
            show_point_of_control: config.show_poc,
            show_numbers: config.show_numbers,
            show_delta_totals: config.show_delta_totals,
        }
    }
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

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct OpeningScaleSnapshot {
    pub tape: Option<bool>,
    pub candle: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PaneBubblesSnapshot {
    /// Independent reversible preferences, readable even when marks are hidden.
    #[serde(default)]
    pub opening_scale: OpeningScaleSnapshot,
    /// Retained executions aggregated into visible FLOW regions, separate from native Tape.
    #[serde(default)]
    pub flow_execution: Option<FlowExecutionSnapshot>,
    pub pane_id: WireU64,
    pub side: PaneSideDto,
    pub engine: AvailabilitySnapshot,
    pub bubbles: Option<BubblesStateSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BubblesStateSnapshot {
    /// Aggression bubbles are drawn over the chart. Beside the native tape,
    /// FLOW draws regional execution totals on the tick candles;
    /// `layers.visibility` names any block.
    pub enabled: bool,
    /// And over the live lane, which is a separate switch.
    pub lane_enabled: bool,
    /// Where the aggression these bubbles draw comes from.
    pub aggression_provenance: String,
    /// The exact quantity the trader's own display floor keeps off the canvas.
    /// Floored, not dropped: it is still in the totals.
    pub floored_quantity: CanonicalDecimal,
    /// Bubbles use volume dots with a buy/sell pie. The ordinary chart and
    /// lane use dots keyed by bars or market-time windows and price levels.
    /// On the native tape, dots use quantity-weighted trade time and price;
    /// the forming dot rides the right edge. Nearby recent prints may merge;
    /// once they leave the live aggregation region, their membership stays
    /// fixed while time and price axes transform their coordinates. The
    /// default area scale follows the largest visible dot, with a common
    /// radius cap to keep neighbours readable. Explicit display changes
    /// start a new grouping. The bubble setting `overlap_merge` is
    /// switched by `layers.visibility.set` as layer `bubble_overlap_merge`.
    #[serde(default)]
    pub overlap_merge: bool,
    /// The tape is the native tape: each print at its own execution time
    /// and price, on the market clock over the whole window, with one price
    /// axis fitted by its prints (manual Y allowed). Beside the tick candles
    /// it keeps its share of the pane behind a draggable divider, and moving
    /// or zooming the candles never changes it. The tape the pane builds, not
    /// the switch: true whenever `tape_only` is, and beside the candles only
    /// with the tape on and `overlap_merge` (volume dots) on. The request is
    /// the lane setting `native_tape`, one of the asset's bubble settings
    /// (see `asset`), read back by `layers.visibility` as layer `native_tape`.
    #[serde(default)]
    pub native_tape: bool,
    /// The pane shows the tape alone, Bookmap style: the native tape takes
    /// the whole canvas, no candle or candle mark is drawn and time runs
    /// across the full width on the tape's own window. The lane setting
    /// `tape_only`, switched by `layers.visibility.set` as layer `tape_only`.
    #[serde(default)]
    pub tape_only: bool,
    /// Exclude the first recorded 100 ms burst from automatic tape-only
    /// sizing; its factual volume is retained and its drawn radius capped.
    #[serde(default)]
    pub ignore_opening_burst_in_scale: bool,
    /// First recorded native window of each retained UTC day. For daytime
    /// WIN sessions, this identifies the first available burst, not a proven
    /// exchange auction. Empty until executions have been observed.
    #[serde(default)]
    pub recorded_opening_windows_ms: Vec<i64>,
    /// The rungs and size scales the last volume-dots frame was built on;
    /// absent when the pane last drew no dots.
    #[serde(default)]
    pub volume_dots: Option<VolumeDotsSnapshot>,
    /// The asset these bubble settings belong to. Every change made while
    /// it is on screen — the panel, `layers.visibility.set` on `bubbles`,
    /// `bubble_overlap_merge`, `native_tape`, `tape_only` and the flow
    /// pane's `candle_aggression`, and `orderflow.tape.opening_scale.set` —
    /// belongs to this asset alone. While its `save_changes` is on it is
    /// saved, reaches every tab showing the asset and is restored whenever
    /// a tab shows it again; off, it stays on the pane it was made on for
    /// the session. Wheeling or dragging the tape's window or width moves
    /// the view only. Absent until the tab binds an asset.
    #[serde(default)]
    pub asset: Option<BubbleAssetSnapshot>,
}

/// The asset a pane's bubble settings belong to.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BubbleAssetSnapshot {
    /// The asset's key: the feed config's `symbol_bubble_presets` key that
    /// names the symbol (`WIN*` covers every mini index contract), or the
    /// symbol itself.
    pub key: String,
    /// Where the settings came from: `stored` (changed for this asset; see
    /// `saved`), `declared` (the preset the feed config declares) or
    /// `default` (the presets file's active look).
    pub source: String,
    /// The preset the look on screen started from; empty once the panel's
    /// defaults replaced it.
    pub preset: String,
    /// The asset's "Save changes for this asset" (default on), the same in
    /// every tab on it; `orderflow.bubbles.save_changes.set` switches it.
    /// Off, a change stays on the pane it was made on for this session — not
    /// stored, not shown in other tabs on the asset, which show the stored
    /// settings — and `saved` is `false` while one is on screen. Switched on
    /// again, the switching pane's settings are stored for the asset, unless
    /// its stored settings changed since that pane last showed them: then
    /// the pane wears those instead (the action's result `screen` says
    /// which).
    #[serde(default = "save_changes_default")]
    pub save_changes: bool,
    /// Whether the store file holds the settings this pane shows. `false`
    /// until the next frame writes a change, while the file cannot be read
    /// or written — they are kept in memory, retried, for this run only —
    /// and while saving is off and a change is on screen.
    pub saved: bool,
    /// Why they are not saved; absent when they are.
    #[serde(default)]
    pub save_error: Option<String>,
}

const fn save_changes_default() -> bool {
    true
}

impl BubbleAssetSnapshot {
    /// The asset `binding` keeps a pane's settings for, wearing the look
    /// that started from `preset`.
    #[must_use]
    pub fn of(binding: &quantick_stores::bubble_asset_store::AssetBinding, preset: &str) -> Self {
        let unsaved = binding.unsaved();
        Self {
            key: binding.key().to_owned(),
            source: binding.source().as_str().to_owned(),
            preset: preset.to_owned(),
            save_changes: binding.saves_changes(),
            saved: unsaved.is_none(),
            save_error: unsaved,
        }
    }
}

impl BubblesStateSnapshot {
    pub fn from_config(
        config: &HeatmapConfig,
        floored_quantity: rust_decimal::Decimal,
        dot_scale: Option<&quantick_orderflow::DotScale>,
        opening_bursts: &[i64],
        asset: Option<BubbleAssetSnapshot>,
    ) -> Self {
        Self {
            enabled: config.show_aggressions,
            lane_enabled: config.lane_aggressions_visible(),
            aggression_provenance: AGGRESSION_PROVENANCE.to_owned(),
            floored_quantity: canonical_decimal(floored_quantity),
            overlap_merge: config.volume_dots.enabled,
            native_tape: config.native_tape(),
            tape_only: config.live_lane.tape_only,
            ignore_opening_burst_in_scale: config.volume_dots.ignore_opening_burst_in_scale,
            recorded_opening_windows_ms: opening_bursts.to_vec(),
            volume_dots: dot_scale.map(Into::into),
            asset,
        }
    }
}

/// What a volume-dots frame was keyed and sized on.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct VolumeDotsSnapshot {
    /// The tape's window, in milliseconds.
    pub tape_window_ms: WireU64,
    /// Native ticks per tape level, chosen from the price span of the tape's
    /// own prints.
    pub tape_level_ticks: WireU64,
    /// Native ticks per candle level, chosen from the candle price axis. A
    /// candle dot is its whole bar at one level.
    pub candle_level_ticks: WireU64,
    /// Contracts a dot holds at the largest radius when the size is typed:
    /// area is proportional to quantity against it. The bubble setting
    /// `volume_dot_full_quantity`.
    pub volume_dot_full_quantity: CanonicalDecimal,
    /// Each pane sizes its dots relative to its own biggest dot on screen
    /// instead of `volume_dot_full_quantity`. The bubble setting
    /// `volume_dot_auto_full`.
    pub auto_full: bool,
}

impl From<&quantick_orderflow::DotScale> for VolumeDotsSnapshot {
    fn from(scale: &quantick_orderflow::DotScale) -> Self {
        let wire = |value: i64| WireU64::new(u64::try_from(value.max(0)).unwrap_or(0));
        Self {
            tape_window_ms: wire(scale.tape_window_ms),
            tape_level_ticks: wire(scale.tape_level_ticks),
            candle_level_ticks: wire(scale.candle_level_ticks),
            volume_dot_full_quantity: canonical_decimal(scale.volume_dot_full_quantity),
            auto_full: scale.auto_full,
        }
    }
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

impl HeatmapStateSnapshot {
    pub fn from_config(config: &HeatmapConfig, grouping: rust_decimal::Decimal) -> Self {
        Self {
            visible: config.depth_visible(),
            lane_visible: config.lane_depth_visible(),
            retention_ms: config.retention_ms,
            capture_price_grouping: canonical_decimal(grouping),
            display_grouping: display_grouping_name(config.display_grouping),
            opacity: canonical_f32(config.opacity, SETTING_DECIMAL_PLACES),
            gamma: canonical_f32(config.gamma, SETTING_DECIMAL_PLACES),
            show_aggressions: config.show_aggressions,
        }
    }
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

impl BookSnapshot {
    /// Capture the bounded published ladder without consulting an engine.
    pub fn from_ladder(
        status: &quantick_orderflow::engine::CaptureStatus,
        ladder: Option<&quantick_orderflow::engine::BookLadder>,
        grouping: rust_decimal::Decimal,
    ) -> Self {
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
}
