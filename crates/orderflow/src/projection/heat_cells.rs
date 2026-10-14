//! How a resting run becomes a band, and where the book is missing: the one
//! rule the live frame and a tape held in the past both cut their bands and
//! gaps by, so the two cannot drift apart.

use std::collections::BTreeMap;

use rust_decimal::Decimal;

use super::model::{BEFORE_CAPTURE, GapPrimitive, HeatmapCell, PriceWindow};
use crate::config::{HeatmapConfig, IntensityMode};
use crate::grouping::EffectiveGrouping;
use crate::history::{LiquidityHistory, RestingSide};

/// A band before it is coloured: the scale is read from all of them first.
#[derive(Debug)]
pub(super) struct DraftCell {
    pub(super) generation: u64,
    pub(super) side: RestingSide,
    pub(super) price_bucket: Decimal,
    pub(super) quantity: Decimal,
    pub(super) x0: f64,
    pub(super) x1: f64,
    pub(super) y0: f64,
    pub(super) y1: f64,
    /// Past the last confirmation, carried to the live edge.
    pub(super) carried: bool,
    pub(super) time_averaged: bool,
    pub(super) current_book: bool,
    pub(super) current_book_source: Option<super::model::BookSource>,
}

/// The `(y0, y1)` rows of the bucket at `price_bucket`, clipped to `prices`;
/// `None` when nothing of it is inside them.
pub(super) fn bucket_rows(
    price_bucket: Decimal,
    grouping: EffectiveGrouping,
    prices: PriceWindow,
) -> Option<(f64, f64)> {
    let low = price_bucket.max(prices.low);
    let high = (price_bucket + grouping.bucket_width).min(prices.high);
    let (y0, y1) = (prices.y(high)?, prices.y(low)?);
    (y1 > y0).then_some((y0, y1))
}

/// A price row is identical for every interval at that level in this projection.
/// Keep its exact Decimal conversion once, including an off-screen result.
pub(super) struct PriceRows {
    grouping: EffectiveGrouping,
    prices: PriceWindow,
    rows: BTreeMap<Decimal, Option<(f64, f64)>>,
}

impl PriceRows {
    pub(super) fn new(grouping: EffectiveGrouping, prices: PriceWindow) -> Self {
        Self {
            grouping,
            prices,
            rows: BTreeMap::new(),
        }
    }

    pub(super) fn get(&mut self, bucket: Decimal) -> Option<(f64, f64)> {
        *self
            .rows
            .entry(bucket)
            .or_insert_with(|| bucket_rows(bucket, self.grouping, self.prices))
    }
}

/// The quantity a band reads full against: the drawn runs' 99th percentile,
/// or the trader's pinned maximum.
pub(super) fn liquidity_reference(
    config: &HeatmapConfig,
    quantities: impl Iterator<Item = Decimal>,
) -> Decimal {
    match config.intensity_mode {
        IntensityMode::VisibleP99 => percentile_99(quantities),
        IntensityMode::Fixed(maximum) => maximum,
    }
}

pub(super) fn percentile_99(values: impl Iterator<Item = Decimal>) -> Decimal {
    let mut positive: Vec<Decimal> = values
        .filter(|quantity| *quantity > Decimal::ZERO)
        .collect();
    if positive.is_empty() {
        return Decimal::ZERO;
    }
    let rank = (99 * positive.len()).div_ceil(100);
    *positive.select_nth_unstable(rank.saturating_sub(1)).1
}

/// Colour `drafts` against `reference`, keeping the strongest walls when
/// there are more than the cap allows. Returns the bands and how many the
/// cap dropped. Hidden heat drops nothing: the trader chose to hide it.
pub(super) fn finish_cells(
    drafts: Vec<DraftCell>,
    reference: Decimal,
    config: &HeatmapConfig,
) -> (Vec<HeatmapCell>, usize) {
    finish_with_priority(drafts, reference, config, false)
}

pub(super) fn finish_with_priority(
    mut drafts: Vec<DraftCell>,
    reference: Decimal,
    config: &HeatmapConfig,
    current_first: bool,
) -> (Vec<HeatmapCell>, usize) {
    if !config.show_liquidity {
        drafts.clear();
    }
    let dropped = drafts.len().saturating_sub(config.max_visible_cells);
    if dropped > 0 {
        // Select the retained prefix before sorting it. The original position
        // breaks equal ranks exactly as the stable full sort did, so ties at
        // the cap preserve side, geometry and carried/observed provenance.
        let mut ranked: Vec<_> = drafts.into_iter().enumerate().collect();
        let rank = |a: &(usize, DraftCell), b: &(usize, DraftCell)| {
            (current_first && !a.1.current_book)
                .cmp(&(current_first && !b.1.current_book))
                .then_with(|| a.1.time_averaged.cmp(&b.1.time_averaged))
                .then_with(|| {
                    if current_first {
                        b.1.x1.total_cmp(&a.1.x1)
                    } else {
                        std::cmp::Ordering::Equal
                    }
                })
                .then_with(|| b.1.quantity.cmp(&a.1.quantity))
                .then_with(|| a.1.generation.cmp(&b.1.generation))
                .then_with(|| a.1.price_bucket.cmp(&b.1.price_bucket))
                .then_with(|| a.1.x0.total_cmp(&b.1.x0))
                .then_with(|| a.0.cmp(&b.0))
        };
        ranked.select_nth_unstable_by(config.max_visible_cells, rank);
        ranked.truncate(config.max_visible_cells);
        ranked.sort_unstable_by(rank);
        drafts = ranked.into_iter().map(|(_, draft)| draft).collect();
    }
    let cells = drafts
        .into_iter()
        .map(|draft| {
            let intensity =
                super::normalized_log_intensity(draft.quantity, reference, config.gamma);
            HeatmapCell {
                generation: draft.generation,
                side: draft.side,
                price_bucket: draft.price_bucket,
                quantity: draft.quantity,
                x0: draft.x0,
                x1: draft.x1,
                y0: draft.y0,
                y1: draft.y1,
                intensity,
                alpha: intensity
                    * config.opacity
                    * if draft.carried {
                        super::CARRIED_BOOK_ALPHA
                    } else {
                        1.0
                    },
                carried: draft.carried,
                time_averaged: draft.time_averaged,
                current_book: draft.current_book,
                current_book_source: draft.current_book_source,
            }
        })
        .collect();
    (cells, dropped)
}

/// Where the book is missing over `[start_ms, end_ms)`, located by `locate`:
/// the coverage gaps inside it, and the stretch before the first book this
/// session captured — all of it when none was. Sorted left to right.
pub(super) fn book_gaps(
    history: &LiquidityHistory,
    start_ms: i64,
    end_ms: i64,
    locate: impl Fn(i64) -> Option<f64>,
) -> Vec<GapPrimitive> {
    let mut gaps: Vec<GapPrimitive> = history
        .coverage_gaps()
        .filter_map(|gap| {
            let gap_end = gap.end_ms.unwrap_or(end_ms);
            if gap_end <= start_ms || gap.start_ms >= end_ms {
                return None;
            }
            let x0 = locate(gap.start_ms.max(start_ms))?;
            let x1 = locate(gap_end.min(end_ms))?;
            (x1 > x0).then(|| GapPrimitive {
                from_generation: gap.from_generation,
                to_generation: gap.to_generation,
                x0,
                x1,
                reason: gap.reason.clone(),
            })
        })
        .collect();
    // Prints can precede the first captured snapshot: that absence is a mark,
    // never a transparent stretch that reads as an empty book.
    let leading = match history.coverage_segments().next() {
        Some(first) if first.start_ms > start_ms => {
            match (locate(start_ms), locate(first.start_ms.min(end_ms))) {
                (Some(x0), Some(x1)) if x1 > x0 => Some((Some(first.generation), x0, x1)),
                _ => None,
            }
        }
        None => Some((None, 0.0, 1.0)),
        Some(_) => None,
    };
    gaps.extend(leading.map(|(to_generation, x0, x1)| GapPrimitive {
        from_generation: None,
        to_generation,
        x0,
        x1,
        reason: BEFORE_CAPTURE.to_owned(),
    }));
    gaps.sort_by(|a, b| a.x0.total_cmp(&b.x0).then_with(|| a.x1.total_cmp(&b.x1)));
    gaps
}

#[cfg(test)]
mod tests;
