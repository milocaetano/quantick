//! Chart summary and append-only paginated bar-window projections.

use quantick_control_host::wire::{BarSpecDto, DecimalRange, PaneSideDto};

use quantick_control::{
    cursor::PageCursor,
    error::ControlError,
    id::ModuleId,
    limits::CONTROL_CHART_WINDOW_MAX_PAGE_ITEMS,
    wire::{CanonicalDecimal, WireU64},
};

use schemars::JsonSchema;

use serde::{Deserialize, Serialize};

pub const SCOPE_ID: &str = "chart.summary";

pub const WINDOW_SCOPE_ID: &str = "chart.window";

pub const MODULE_ID: &str = "chart";

pub const SCHEMA_VERSION: u32 = 1;

pub const VIEWPORT_DECIMAL_PLACES: u32 = 6;

pub const PRICE_DECIMAL_PLACES: u32 = 10;

pub const PIXEL_DECIMAL_PLACES: u32 = 3;

pub const OMITTED_WINDOW_MODULE_IDS: [&str; 3] = ["drawings", "indicators", "orderflow"];

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ChartSnapshot {
    pub panes: Vec<ChartPaneSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ChartPaneSnapshot {
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
    /// Prints the pane's rule could place in no bar — a deal bar's prints
    /// before its first counter reading. Zero for every other rule. The
    /// number the chart-corner chip shows, as data.
    /// Optional on the wire so v1 readers remain compatible with snapshots
    /// produced before deal bars existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uncounted_prints: Option<WireU64>,
    pub venue_history_bar_count: WireU64,
    pub backfill_boundary_slot: Option<WireU64>,
    pub has_in_progress_bar: bool,
    pub viewport: ViewportSnapshot,
    pub coverage: ChartCoverage,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ViewportSnapshot {
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
pub struct ChartCoverage {
    #[schemars(extend("x-unit" = "unix_milliseconds"))]
    pub oldest_open_time_unix_ms: Option<i64>,
    #[schemars(extend("x-unit" = "unix_milliseconds"))]
    pub newest_close_time_unix_ms: Option<i64>,
    pub older_history_paging_supported: bool,
    pub venue_prefix_present: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BarStateDto {
    Closed,
    InProgress,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BarSnapshot {
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
pub struct BarProvenance {
    pub source: String,
    pub completeness: String,
    pub price: String,
    pub volume: String,
    pub aggressor_side: String,
    pub trade_count: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ChartWindowQuery {
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    pub range: ChartWindowRange,
    #[schemars(range(min = 1, max = CONTROL_CHART_WINDOW_MAX_PAGE_ITEMS))]
    pub page_size: usize,
}

impl ChartWindowQuery {
    /// The visible window of one pane, a full page: what a test asks for
    /// first.
    #[must_use]
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
pub enum ChartWindowRange {
    Visible,
    Slots {
        start_slot: WireU64,
        end_slot_exclusive: WireU64,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ChartWindowPage {
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
pub struct ChartBarPage {
    #[schemars(length(max = CONTROL_CHART_WINDOW_MAX_PAGE_ITEMS))]
    pub items: Vec<BarSnapshot>,
    #[schemars(range(max = CONTROL_CHART_WINDOW_MAX_PAGE_ITEMS))]
    pub item_count: usize,
    pub has_more: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<PageCursor>,
}

impl ChartBarPage {
    pub fn new(
        items: Vec<BarSnapshot>,
        next_cursor: Option<PageCursor>,
    ) -> Result<Self, ControlError> {
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
