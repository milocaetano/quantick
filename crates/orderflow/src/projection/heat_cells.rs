//! How a resting run becomes a band, and where the book is missing: the one
//! rule the live frame and a tape held in the past both cut their bands and
//! gaps by, so the two cannot drift apart.

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
    positive.sort_unstable();
    let rank = (99 * positive.len()).div_ceil(100);
    positive[rank.saturating_sub(1)]
}

/// Colour `drafts` against `reference`, keeping the strongest walls when
/// there are more than the cap allows. Returns the bands and how many the
/// cap dropped. Hidden heat drops nothing: the trader chose to hide it.
pub(super) fn finish_cells(
    mut drafts: Vec<DraftCell>,
    reference: Decimal,
    config: &HeatmapConfig,
) -> (Vec<HeatmapCell>, usize) {
    if !config.show_liquidity {
        drafts.clear();
    }
    let dropped = drafts.len().saturating_sub(config.max_visible_cells);
    if dropped > 0 {
        drafts.sort_by(|a, b| {
            b.quantity
                .cmp(&a.quantity)
                .then_with(|| a.generation.cmp(&b.generation))
                .then_with(|| a.price_bucket.cmp(&b.price_bucket))
                .then_with(|| a.x0.total_cmp(&b.x0))
        });
        drafts.truncate(config.max_visible_cells);
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
                alpha: intensity * config.opacity,
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
