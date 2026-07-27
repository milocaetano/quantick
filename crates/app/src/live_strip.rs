//! The live strip: a narrow column between the chart body and the price
//! gutter showing the book's resting depth *right now* (Live Edge proposal,
//! phase 4). Rows are bucketed at the same effective grouping as the heatmap
//! and coloured through the heatmap's own ramp, so a wall on the strip lines
//! up 1:1 with that wall's history to its left; the real spread reads as an
//! empty gap between the marked best bid and best ask.
//!
//! Phase 5 lays the forming bar's aggression histogram over that background:
//! buys grow rightward from the strip's centre, sells leftward, each bar's
//! width on the same square-root area rule the bubbles use, normalized by the
//! forming bar's own biggest bucket. The rows come from the projection's
//! aggression clusters — the one engine code path — filtered to the forming
//! bar, so the histogram resets on bar close simply because the new bar has
//! no clusters yet. Without book data the strip degrades to this histogram
//! alone, which is why it works on any source that streams trades (replay
//! included).
//!
//! This module owns the pure, testable math (bucketing, references, the
//! histogram); painting lives in `OrderflowView::draw_live_strip`, which
//! reads the published [`BookLadder`] and frame.

use quantick_engine::Side;
use quantick_orderbook::{BookLevel, BookSide};
use rust_decimal::Decimal;

use crate::orderflow::projection::AggressionPrimitive;
use crate::orderflow_engine::BookLadder;

/// Width of the strip, in pixels. The proposal band is 72–96 px: wide enough
/// for the depth silhouette to read, narrow enough to never crowd the chart.
pub(crate) const LIVE_STRIP_WIDTH_PX: f32 = 84.0;

/// Stroke of the best bid/ask touch markers, in pixels.
pub(crate) const TOUCH_MARKER_STROKE_PX: f32 = 1.5;

/// Alpha of the strip's left border line, against the chart body.
pub(crate) const STRIP_BORDER_ALPHA: f32 = 0.3;

/// Left inset of the depth rows, in pixels, so the border stays visible.
pub(crate) const STRIP_ROW_INSET_PX: f32 = 1.0;

/// One depth row: an aggregated price bucket of the current book.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DepthRow {
    pub side: BookSide,
    /// Inclusive lower price edge of the bucket, in the heatmap's bucket
    /// space: `floor(price / width) * width`.
    pub price_bucket: Decimal,
    /// Summed resting quantity of the ladder levels inside the bucket.
    pub quantity: Decimal,
}

/// Aggregate the ladder into heatmap-aligned buckets: asks first (ascending),
/// then bids (descending) — each side walking away from the spread, so the
/// order is deterministic and mirrors the ladder's own.
pub(crate) fn depth_rows(ladder: &BookLadder, bucket_width: Decimal) -> Vec<DepthRow> {
    if bucket_width <= Decimal::ZERO {
        return Vec::new();
    }
    let mut rows = Vec::new();
    bucket_side(&ladder.asks, BookSide::Ask, bucket_width, &mut rows);
    bucket_side(&ladder.bids, BookSide::Bid, bucket_width, &mut rows);
    rows
}

fn bucket_side(levels: &[BookLevel], side: BookSide, width: Decimal, rows: &mut Vec<DepthRow>) {
    // Ladder levels arrive best-first and strictly ordered, so equal buckets
    // are always adjacent: one running row per bucket, no map needed.
    let mut current: Option<DepthRow> = None;
    for level in levels {
        let bucket = (level.price() / width).trunc() * width;
        match &mut current {
            Some(row) if row.price_bucket == bucket => row.quantity += level.quantity(),
            _ => {
                if let Some(done) = current.take() {
                    rows.push(done);
                }
                current = Some(DepthRow {
                    side,
                    price_bucket: bucket,
                    quantity: level.quantity(),
                });
            }
        }
    }
    if let Some(done) = current {
        rows.push(done);
    }
}

/// Full-intensity reference when no heatmap frame supplies one: the largest
/// visible bucket. Honest for a lone column — the biggest wall in view is
/// the hottest — though it re-normalizes as walls come and go, unlike the
/// frame's stickier visible-P99 reference.
pub(crate) fn fallback_reference(rows: &[DepthRow]) -> Decimal {
    rows.iter()
        .map(|row| row.quantity)
        .max()
        .unwrap_or(Decimal::ZERO)
}

/// Opacity of the histogram bars over the depth background.
pub(crate) const HISTOGRAM_ALPHA: f32 = 0.8;

/// Widest histogram bar, as a fraction of the strip's half width, leaving a
/// sliver of background visible even at full scale.
pub(crate) const HISTOGRAM_MAX_HALF_FRAC: f32 = 0.94;

/// One histogram row: the forming bar's aggression at one price bucket,
/// both sides together because the drawing mirrors them around one centre.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HistogramRow {
    /// Inclusive lower price edge, in the projection's own bucket space.
    pub price_bucket: Decimal,
    /// Summed buy-aggression quantity in the bucket.
    pub buy: Decimal,
    /// Summed sell-aggression quantity in the bucket.
    pub sell: Decimal,
}

/// Sum the forming bar's aggression clusters per price bucket. `bar_open_ms`
/// is the forming bar's open time: clusters that ended before it belong to
/// closed bars and are dropped, which is the whole "resets on close" rule.
/// Ascending bucket order, deterministic.
pub(crate) fn aggression_rows(
    aggressions: &[AggressionPrimitive],
    bar_open_ms: i64,
) -> Vec<HistogramRow> {
    let mut buckets: std::collections::BTreeMap<Decimal, (Decimal, Decimal)> =
        std::collections::BTreeMap::new();
    for cluster in aggressions {
        if cluster.last_timestamp_ms < bar_open_ms {
            continue;
        }
        let entry = buckets
            .entry(cluster.price_bucket)
            .or_insert((Decimal::ZERO, Decimal::ZERO));
        match cluster.side {
            Side::Buy => entry.0 += cluster.quantity,
            Side::Sell => entry.1 += cluster.quantity,
        }
    }
    buckets
        .into_iter()
        .map(|(price_bucket, (buy, sell))| HistogramRow {
            price_bucket,
            buy,
            sell,
        })
        .collect()
}

/// Full-width reference for the histogram: the forming bar's own biggest
/// single-side bucket, per the "normalized by the bar" rule — the bar's
/// heaviest price level always reaches full width, whatever its size.
pub(crate) fn histogram_reference(rows: &[HistogramRow]) -> Decimal {
    rows.iter()
        .map(|row| row.buy.max(row.sell))
        .max()
        .unwrap_or(Decimal::ZERO)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr as _;

    fn dec(value: &str) -> Decimal {
        Decimal::from_str(value).unwrap()
    }

    fn level(price: &str, quantity: &str) -> BookLevel {
        BookLevel::new(dec(price), dec(quantity)).unwrap()
    }

    fn ladder(bids: Vec<BookLevel>, asks: Vec<BookLevel>) -> BookLadder {
        BookLadder {
            best_bid: bids.first().copied(),
            best_ask: asks.first().copied(),
            bids,
            asks,
        }
    }

    #[test]
    fn rows_bucket_like_the_heatmap_and_merge_neighbours() {
        // Asks best-first ascending, bids best-first descending, bucket 2:
        // levels sharing a bucket sum into one row.
        let ladder = ladder(
            vec![level("999", "1"), level("998", "2"), level("997", "4")],
            vec![level("1001", "1"), level("1002", "2"), level("1003", "4")],
        );
        let rows = depth_rows(&ladder, dec("2"));
        assert_eq!(
            rows,
            vec![
                DepthRow {
                    side: BookSide::Ask,
                    price_bucket: dec("1000"),
                    quantity: dec("1"),
                },
                DepthRow {
                    side: BookSide::Ask,
                    price_bucket: dec("1002"),
                    quantity: dec("6"),
                },
                DepthRow {
                    side: BookSide::Bid,
                    price_bucket: dec("998"),
                    quantity: dec("3"),
                },
                DepthRow {
                    side: BookSide::Bid,
                    price_bucket: dec("996"),
                    quantity: dec("4"),
                },
            ]
        );
    }

    #[test]
    fn a_degenerate_bucket_width_yields_no_rows() {
        let ladder = ladder(vec![level("999", "1")], vec![level("1001", "1")]);
        assert!(depth_rows(&ladder, Decimal::ZERO).is_empty());
    }

    #[test]
    fn the_fallback_reference_is_the_biggest_visible_bucket() {
        let ladder = ladder(
            vec![level("999", "5"), level("998", "9")],
            vec![level("1001", "3")],
        );
        let rows = depth_rows(&ladder, Decimal::ONE);
        assert_eq!(fallback_reference(&rows), dec("9"));
        assert_eq!(fallback_reference(&[]), Decimal::ZERO);
    }

    /// A minimal cluster: only the fields the histogram reads carry data.
    fn cluster(side: Side, bucket: &str, quantity: &str, last_ms: i64) -> AggressionPrimitive {
        AggressionPrimitive {
            agg_id: 1,
            agg_ids: vec![1],
            generation: None,
            side,
            consumed_side: match side {
                Side::Buy => quantick_orderbook::BookSide::Ask,
                Side::Sell => quantick_orderbook::BookSide::Bid,
            },
            quantity: dec(quantity),
            price_bucket: dec(bucket),
            trade_count: 1,
            first_timestamp_ms: last_ms,
            last_timestamp_ms: last_ms,
            matched_quantity: Decimal::ZERO,
            matched_fraction: 0.0,
            liquidity_event_ids: Vec::new(),
            x: 0.5,
            y: 0.5,
            size: 0.5,
        }
    }

    #[test]
    fn the_histogram_keeps_only_the_forming_bar_and_sums_per_bucket() {
        let clusters = vec![
            // Closed-bar cluster: ended before the forming bar opened.
            cluster(Side::Buy, "100", "50", 900),
            // Forming bar: two buys in one bucket, one sell in another.
            cluster(Side::Buy, "100", "2", 1_100),
            cluster(Side::Buy, "100", "3", 1_200),
            cluster(Side::Sell, "99", "4", 1_150),
        ];
        let rows = aggression_rows(&clusters, 1_000);
        assert_eq!(
            rows,
            vec![
                HistogramRow {
                    price_bucket: dec("99"),
                    buy: Decimal::ZERO,
                    sell: dec("4"),
                },
                HistogramRow {
                    price_bucket: dec("100"),
                    buy: dec("5"),
                    sell: Decimal::ZERO,
                },
            ]
        );
        // The bar's heaviest single-side bucket sets full width.
        assert_eq!(histogram_reference(&rows), dec("5"));
        assert_eq!(histogram_reference(&[]), Decimal::ZERO);
    }
}
