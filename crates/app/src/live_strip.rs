//! The live strip: a narrow column between the chart body and the price
//! gutter showing the book's resting depth *right now* (Live Edge proposal,
//! phase 4). Rows are bucketed at the same effective grouping as the heatmap
//! and coloured through the heatmap's own ramp, so a wall on the strip lines
//! up 1:1 with that wall's history to its left; the real spread reads as an
//! empty gap between the marked best bid and best ask.
//!
//! This module owns the pure, testable math (bucketing, the fallback
//! reference); painting lives in `OrderflowView::draw_live_strip`, which
//! reads the published [`BookLadder`].

use quantick_orderbook::{BookLevel, BookSide};
use rust_decimal::Decimal;

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
}
