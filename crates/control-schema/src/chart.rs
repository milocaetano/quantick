//! Chart summary and append-only paginated bar-window projections.

use quantick_control_host::wire::{
    BarSpecDto, DecimalRange, PaneSideDto, canonical_decimal, wire_usize,
};

use quantick_control::{
    cursor::{PageContext, PageCursor, PaginationConsistency},
    error::ControlError,
    id::{InstanceId, ModuleId, SnapshotScopeId},
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
    /// The price step the pane's rule read off the pane's own prints and
    /// cuts on — a Renko pane's, once its prints have shown it. Inferred,
    /// never declared by a venue. Absent for a rule that reads no step, and
    /// for one still reading it: `held_prints` says how many it holds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inferred_price_step: Option<CanonicalDecimal>,
    /// Prints the pane's rule holds uncut until they show it its price step
    /// — a Renko pane's before its first brick. They form the in-progress
    /// bar. Zero for every rule that reads no step. Optional on the wire, as
    /// `uncounted_prints` is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub held_prints: Option<WireU64>,
    /// Prints off the price step the pane's rule froze: cut on that step and
    /// counted, never snapped onto it. Zero for every rule that freezes none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub off_grid_prints: Option<WireU64>,
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
    /// The native tape's own time frame; absent without a native tape.
    pub tape: Option<crate::tape_view::TapeViewSnapshot>,
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

/// Source facts for bars built from trades, supplied by the feed owner.
pub struct BarProvenanceContext {
    pub engine_price: String,
    pub engine_volume: String,
    pub engine_side: String,
    /// Whether the venue's candles carry its own aggressor split, rather
    /// than half the volume on each side.
    pub venue_aggressor_split: bool,
}

impl BarSnapshot {
    /// Map an exact bar and its position among venue, backfill and live bars
    /// onto the wire. The caller supplies the history boundaries it owns.
    pub fn from_bar(
        slot: usize,
        bar: &quantick_engine::Bar,
        state: BarStateDto,
        seam: usize,
        backfill_boundary: Option<usize>,
        context: &BarProvenanceContext,
    ) -> Self {
        let venue_prefix = slot < seam;
        let source = if venue_prefix {
            "venue_ohlcv"
        } else {
            let engine_slot = slot - seam;
            match backfill_boundary {
                Some(boundary) if engine_slot < boundary => "trade_backfill",
                Some(boundary) if engine_slot == boundary => "live_or_backfill_live_boundary",
                _ => "live_trades",
            }
        };
        Self {
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
}

impl BarSnapshot {
    /// The seam bar holds `lead` — the venue's candles of its bucket before
    /// the first print — as well as the prints: each provenance it summarises
    /// is named, venue first, as they sit in time. Where the venue reports no
    /// trade count, or no aggressor split, the bar's count holds the prints'
    /// alone and its buy, sell and delta hold the venue's even split: both
    /// are said, never folded into the prints' own label.
    pub fn mark_venue_lead(&mut self, lead: &quantick_engine::Bar, context: &BarProvenanceContext) {
        let provenance = &mut self.provenance;
        provenance.source = format!("venue_ohlcv+{}", provenance.source);
        provenance.price = format!("venue_candle+{}", provenance.price);
        provenance.volume = format!("venue_reported+{}", provenance.volume);
        let venue_side = if context.venue_aggressor_split {
            "venue_reported"
        } else {
            "venue_even_split_not_measured"
        };
        provenance.aggressor_side = format!("{venue_side}+{}", provenance.aggressor_side);
        let venue_count = if lead.trade_count == 0 {
            "venue_unavailable"
        } else {
            "venue_reported"
        };
        provenance.trade_count = format!("{venue_count}+derived_from_trades");
    }

    /// The bar knowingly omits a stretch of its bucket — the seconds before
    /// the first print no candle speaks for, or the start of history cutting
    /// into it — closed or forming.
    pub fn mark_partial(&mut self) {
        self.provenance.completeness = "partial".to_owned();
    }
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
    pub fn validate_page_size(&self) -> Result<(), ControlError> {
        if self.page_size == 0 || self.page_size > CONTROL_CHART_WINDOW_MAX_PAGE_ITEMS {
            return Err(ControlError::invalid_request(format!(
                "chart page size must be in 1..={CONTROL_CHART_WINDOW_MAX_PAGE_ITEMS}"
            )));
        }
        Ok(())
    }

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

/// The fixed prefix selected by an append-only chart request. The caller
/// supplies observed bounds and reads bars; this owner validates the cursor
/// and finishes the page without knowing a pane or an application clock.
#[derive(Debug)]
pub struct ChartWindowSelection<'a> {
    pub slots: std::ops::Range<usize>,
    pub high_water: WireU64,
    instance_id: &'a InstanceId,
    canonical_query: &'a serde_json::Value,
    cursor: Option<&'a PageCursor>,
    consistency_revision: WireU64,
    scope_id: SnapshotScopeId,
}

impl<'a> ChartWindowSelection<'a> {
    pub fn resolve(
        query: &ChartWindowQuery,
        instance_id: &'a InstanceId,
        canonical_query: &'a serde_json::Value,
        cursor: Option<&'a PageCursor>,
        consistency_revision: WireU64,
        closed: usize,
        visible_slots: Option<(usize, usize)>,
    ) -> Result<Self, ControlError> {
        query.validate_page_size()?;
        let scope_id = SnapshotScopeId::new(WINDOW_SCOPE_ID).expect("static scope ID is valid");
        let (position, high_water) = if let Some(cursor) = cursor {
            let high_water = cursor.high_water_position.ok_or_else(|| {
                ControlError::invalid_request("append-only chart cursor has no high-water position")
            })?;
            cursor.validate_next(&PageContext {
                instance_id,
                scope_id: &scope_id,
                query: canonical_query,
                consistency_mode: PaginationConsistency::AppendOnly,
                consistency_revision,
                high_water_position: Some(high_water),
                resource_id: None,
                resource_available: true,
            })?;
            let high_water_usize = usize::try_from(high_water.get()).unwrap_or(usize::MAX);
            if high_water_usize > closed {
                return Err(ControlError::page_stale(
                    "the chart no longer contains the cursor's high-water prefix",
                ));
            }
            (cursor.next_position, high_water)
        } else {
            let (start, requested_end) = match &query.range {
                ChartWindowRange::Visible => {
                    let Some((start, end)) = visible_slots else {
                        // A valid request with no painted geometry is
                        // retryable, rather than a malformed slot range.
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
                    };
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
            (wire_usize(start), wire_usize(requested_end.min(closed)))
        };
        let start = usize::try_from(position.get()).unwrap_or(usize::MAX);
        let stop = usize::try_from(high_water.get()).unwrap_or(usize::MAX);
        if start > stop {
            return Err(ControlError::invalid_request(
                "chart cursor position exceeds its high-water mark",
            ));
        }
        Ok(Self {
            slots: start..start.saturating_add(query.page_size).min(stop),
            high_water,
            instance_id,
            canonical_query,
            cursor,
            consistency_revision,
            scope_id,
        })
    }

    pub fn complete(&self, items: Vec<BarSnapshot>) -> Result<ChartBarPage, ControlError> {
        if items.len() != self.slots.end.saturating_sub(self.slots.start) {
            return Err(ControlError::page_stale(
                "the chart's closed-bar prefix changed during pagination",
            ));
        }
        let stop = usize::try_from(self.high_water.get()).unwrap_or(usize::MAX);
        let next_cursor = if self.slots.end < stop {
            let next_position = wire_usize(self.slots.end);
            Some(if let Some(cursor) = self.cursor {
                let mut next = cursor.clone();
                next.next_position = next_position;
                next
            } else {
                PageCursor::first(
                    &PageContext {
                        instance_id: self.instance_id,
                        scope_id: &self.scope_id,
                        query: self.canonical_query,
                        consistency_mode: PaginationConsistency::AppendOnly,
                        consistency_revision: self.consistency_revision,
                        high_water_position: Some(self.high_water),
                        resource_id: None,
                        resource_available: true,
                    },
                    next_position,
                )?
            })
        } else {
            None
        };
        ChartBarPage::new(items, next_cursor)
    }
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

#[cfg(test)]
mod venue_lead_label_tests {
    use super::*;

    fn context(venue_aggressor_split: bool) -> BarProvenanceContext {
        BarProvenanceContext {
            engine_price: "venue_or_broker_trade".to_owned(),
            engine_volume: "venue_reported".to_owned(),
            engine_side: "inferred_or_derived".to_owned(),
            venue_aggressor_split,
        }
    }

    fn bar(trade_count: u64) -> quantick_engine::Bar {
        quantick_engine::Bar {
            open_time: 0,
            close_time: 59_999,
            open: rust_decimal::Decimal::ONE,
            high: rust_decimal::Decimal::ONE,
            low: rust_decimal::Decimal::ONE,
            close: rust_decimal::Decimal::ONE,
            buy_volume: rust_decimal::Decimal::ONE,
            sell_volume: rust_decimal::Decimal::ONE,
            trade_count,
        }
    }

    fn seam_bar(state: BarStateDto, context: &BarProvenanceContext) -> BarSnapshot {
        BarSnapshot::from_bar(3, &bar(9), state, 3, None, context)
    }

    /// MetaTrader's lead reports no trade count and no aggressor split: the
    /// seam bar says its count is the prints' alone and its side the venue's
    /// even split, and a bar missing the seconds before its first print is
    /// partial whether it is closed or forming.
    #[test]
    fn a_lead_without_a_count_or_a_split_says_so() {
        let mt5 = context(false);
        let mut snapshot = seam_bar(BarStateDto::Closed, &mt5);
        snapshot.mark_venue_lead(&bar(0), &mt5);
        snapshot.mark_partial();
        let provenance = &snapshot.provenance;
        assert_eq!(
            provenance.trade_count,
            "venue_unavailable+derived_from_trades"
        );
        assert_eq!(
            provenance.aggressor_side,
            "venue_even_split_not_measured+inferred_or_derived"
        );
        assert_eq!(provenance.completeness, "partial");
        let mut forming = seam_bar(BarStateDto::InProgress, &mt5);
        forming.mark_partial();
        assert_eq!(forming.provenance.completeness, "partial");
        assert_eq!(forming.state, BarStateDto::InProgress);
    }

    /// Binance's klines carry the venue's own count and taker-buy split.
    #[test]
    fn a_lead_with_the_venue_s_count_and_split_is_reported() {
        let binance = context(true);
        let mut snapshot = seam_bar(BarStateDto::Closed, &binance);
        snapshot.mark_venue_lead(&bar(42), &binance);
        let provenance = &snapshot.provenance;
        assert_eq!(provenance.trade_count, "venue_reported+derived_from_trades");
        assert_eq!(
            provenance.aggressor_side,
            "venue_reported+inferred_or_derived"
        );
        assert_eq!(provenance.source, "venue_ohlcv+live_trades");
        assert_eq!(provenance.completeness, "complete");
    }
}
