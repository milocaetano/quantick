//! The budget fold: how a pane brings its marks inside the frame's bubble
//! budget without deleting a single contract.
//!
//! The parent module places the marks; this module decides which of them
//! merge when there are more than the pane may draw — the split of the budget
//! between the two panes, the order each pane folds in, and the merge that
//! conserves every exact quantity. Nothing here reads the tape or the book.

use std::collections::BTreeMap;

use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive as _;

use super::{AggressionPrimitive, normalized_area_size};
use crate::config::{
    DEFAULT_LIVE_LANE_SHARE, LiveLaneStyle, MAX_LIVE_LANE_SHARE, MIN_LIVE_LANE_SHARE,
};
use crate::history::AggressorSide;
use crate::timeline::BarTimeline;

/// A total order over sides, so a fold can sort and group by one.
/// `AggressorSide` is a fact about a print, not a rank, so it carries no `Ord`
/// of its own.
fn side_key(side: AggressorSide) -> u8 {
    match side {
        AggressorSide::Buy => 0,
        AggressorSide::Sell => 1,
    }
}

/// Split the frame's bubble budget between the two panes.
///
/// The two panes draw different things out of the same budget: the candles
/// draw a compressed history whose marks each carry a bar, the tape draws the
/// newest prints one by one. Ranked against each other by quantity — which is
/// what a single shared budget did — the tape lost every time, and zooming the
/// candles out emptied it. So the budget is split before anything is ranked,
/// and each pane spends its own.
///
/// The split is the tape's own width share ([`LiveLaneStyle::width_share`]),
/// not a number of its own: marks need room to be read, so the pane with a
/// third of the canvas gets a third of the marks, and a trader who drags the
/// divider moves both together.
///
/// With the tape switched off there is no second pane, and the candles get the
/// whole budget — reserving a share for a band nobody is drawing would fold the
/// candles harder to protect nothing.
///
/// Both shares are at least two whenever the tape is on, which is what lets a
/// pane always fit: two marks is one per side, and a fold never crosses sides.
pub(super) fn pane_budgets(limit: usize, lane: &LiveLaneStyle) -> (usize, usize) {
    if !lane.enabled {
        return (limit, 0);
    }
    let share = if lane.width_share.is_finite() {
        lane.width_share
            .clamp(MIN_LIVE_LANE_SHARE, MAX_LIVE_LANE_SHARE)
    } else {
        DEFAULT_LIVE_LANE_SHARE
    };
    let lane_budget = ((limit as f32) * share).round().max(0.0) as usize;
    let lane_budget = lane_budget.clamp(2, limit.saturating_sub(2).max(2));
    (limit.saturating_sub(lane_budget).max(2), lane_budget)
}

/// Fold `other` into `mark`, conserving every exact quantity it carried.
///
/// `mark` is always the group's heaviest member — [`fold_chunk`] puts it there
/// — so the merged mark keeps the position of the print that actually carries
/// the volume. A quantity-weighted midpoint would land between the members: a
/// bubble at a price and an instant where nothing traded at all, and a
/// fabricated fact is worse than the omission this fold replaced. The regional
/// fold settled the same question the same way — `finish_regional` refuses the
/// fold-wide average and anchors at the point of control.
///
/// The declared price band, by contrast, covers *every* member: a consumer that
/// draws the range (the live strip's histogram) has to be told the whole zone
/// the folded quantity came from, not just the winner's row.
fn merge_marks(
    mark: &mut AggressionPrimitive,
    other: &mut AggressionPrimitive,
    reference: Decimal,
) {
    debug_assert_eq!(mark.live, other.live, "a fold may not cross the panes");
    debug_assert_eq!(mark.side, other.side, "a fold may not cross sides");
    let total = mark.quantity + other.quantity;
    let low = mark.price_bucket.min(other.price_bucket);
    let high = (mark.price_bucket + mark.price_span).max(other.price_bucket + other.price_span);
    let total_f64 = total.to_f64().unwrap_or(0.0);
    // A fold of folds counts the marks underneath it, not the folds: a virgin
    // mark stands for itself, so it enters the sum as one.
    mark.folded_marks = mark
        .folded_marks
        .max(1)
        .saturating_add(other.folded_marks.max(1));
    mark.buy_share = if total_f64 > 0.0 {
        ((f64::from(mark.buy_share) * mark.quantity.to_f64().unwrap_or(0.0)
            + f64::from(other.buy_share) * other.quantity.to_f64().unwrap_or(0.0))
            / total_f64) as f32
    } else {
        mark.buy_share
    };
    mark.quantity = total;
    mark.trade_count = mark.trade_count.saturating_add(other.trade_count);
    mark.first_timestamp_ms = mark.first_timestamp_ms.min(other.first_timestamp_ms);
    mark.last_timestamp_ms = mark.last_timestamp_ms.max(other.last_timestamp_ms);
    mark.matched_quantity += other.matched_quantity;
    mark.matched_fraction = if total > Decimal::ZERO {
        (mark.matched_quantity / total)
            .to_f64()
            .unwrap_or(0.0)
            .clamp(0.0, 1.0) as f32
    } else {
        0.0
    };
    // Moved, never cloned, and never sorted here: a fold absorbs many marks in
    // a row, and sorting the growing list once per absorption is quadratic — it
    // was worth 25x the frame cost on a dense tape. `settle_ids` tidies each
    // finished fold exactly once instead.
    mark.agg_ids.append(&mut other.agg_ids);
    mark.agg_id = mark.agg_id.min(other.agg_id);
    mark.liquidity_event_ids
        .append(&mut other.liquidity_event_ids);
    if mark.generation != other.generation {
        mark.generation = None;
    }
    mark.price_bucket = low;
    mark.price_span = (high - low).max(mark.price_span);
    // The fold carries more quantity, so it has to read bigger. Sized against
    // the reference the pane was already drawn on — merging is a drawing
    // decision and must not rescale the marks it left alone.
    mark.size = normalized_area_size(mark.quantity, reference);
}

/// Ids a single fold keeps, before it stops recording which prints it stands
/// for and lets [`AggressionPrimitive::trade_count`] speak for them.
///
/// A fold is unbounded in principle — a quiet budget on a busy session can put
/// a whole minute of prints under one mark — and the id lists are cloned into
/// every published frame. The exact count is never lost, only the roll of
/// individual ids past this point, and the truncation is declared by
/// `trade_count` exceeding `agg_ids.len()`.
const MAX_FOLD_IDS: usize = 256;

/// Put one finished fold's id lists back in order, once, and bound them.
fn settle_ids(mark: &mut AggressionPrimitive) {
    mark.agg_ids.sort_unstable();
    mark.agg_ids.dedup();
    mark.agg_ids.truncate(MAX_FOLD_IDS);
    mark.liquidity_event_ids.sort_unstable();
    mark.liquidity_event_ids.dedup();
    mark.liquidity_event_ids.truncate(MAX_FOLD_IDS);
}

/// Merge one group of compatible marks into a single mark anchored on the
/// heaviest of them — the group's point of control.
fn fold_chunk(mut chunk: Vec<AggressionPrimitive>, reference: Decimal) -> AggressionPrimitive {
    let heaviest = chunk
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| {
            a.quantity
                .cmp(&b.quantity)
                .then_with(|| b.agg_id.cmp(&a.agg_id))
        })
        .map_or(0, |(index, _)| index);
    chunk.swap(0, heaviest);
    let mut rest = chunk.split_off(1);
    let mut merged = chunk.pop().expect("a chunk is never empty");
    for other in &mut rest {
        merge_marks(&mut merged, other, reference);
    }
    if merged.folded_marks > 0 {
        settle_ids(&mut merged);
    }
    merged
}

/// Which mark folds first, per pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FoldOrder {
    /// The candles fold their *smallest* marks together. A trader reads the
    /// big prints off the compressed history; the small ones are the ones a
    /// wider zoom was always going to blur, and folding them keeps the marks
    /// that carry the story untouched.
    SmallestFirst,
    /// The tape folds its *oldest* marks together. Its whole reason to exist
    /// is the newest prints, one by one, at the right edge — so pressure on
    /// the budget is paid at the left edge, where a print was leaving anyway.
    OldestFirst,
}

/// Bring a pane inside its budget without deleting a single contract.
///
/// This is the mission's invariant in code. The old behaviour ranked marks by
/// quantity and truncated, so a print that traded could leave the canvas with
/// nothing said — and which prints left changed with the zoom, because the
/// zoom changes how many marks there are to rank. Now the excess is *folded*:
/// compatible neighbours merge into one bigger mark carrying the exact summed
/// quantity and the union of their evidence, and the sum of the ink is the sum
/// of the tape whatever the budget.
///
/// The result can sit *above* the budget, deliberately. A fold may not cross a
/// side, a pane, or a bar, and with more of those groups than the budget has
/// marks the only way further down is to misattribute volume. The budget is a
/// performance target; correctness outranks it.
pub(super) fn fold_to_budget(
    marks: &mut Vec<AggressionPrimitive>,
    limit: usize,
    order: FoldOrder,
    reference: Decimal,
    bars: Option<&BarTimeline>,
) {
    let limit = limit.max(2);
    if marks.len() <= limit {
        return;
    }
    // Ranked by what the pane is willing to lose resolution on first, across
    // every group: the candles rank by size, the tape by age.
    match order {
        FoldOrder::SmallestFirst => marks.sort_by(|a, b| {
            a.quantity
                .cmp(&b.quantity)
                .then_with(|| a.first_timestamp_ms.cmp(&b.first_timestamp_ms))
                .then_with(|| side_key(a.side).cmp(&side_key(b.side)))
                .then_with(|| a.price_bucket.cmp(&b.price_bucket))
                .then_with(|| a.agg_id.cmp(&b.agg_id))
        }),
        FoldOrder::OldestFirst => marks.sort_by(|a, b| {
            a.first_timestamp_ms
                .cmp(&b.first_timestamp_ms)
                .then_with(|| a.quantity.cmp(&b.quantity))
                .then_with(|| side_key(a.side).cmp(&side_key(b.side)))
                .then_with(|| a.price_bucket.cmp(&b.price_bucket))
                .then_with(|| a.agg_id.cmp(&b.agg_id))
        }),
    }

    // Only the front of that ranking is touched, and the tail comes through
    // untouched — which is the whole point of ranking. On the candles the big
    // prints a trader is reading stay exactly as they were; on the tape the
    // newest prints at the right edge stay one mark per execution, and the
    // pressure is paid at the left edge where a print was leaving anyway.
    let excess = marks.len() - limit;
    let (pool_len, target) = if excess * 2 <= marks.len() {
        // Twice the excess folded two-at-a-time is the smallest pool that fits,
        // so the loss is spread thin and most of the pane is untouched.
        (excess * 2, excess)
    } else {
        // Far past budget: everything folds, in even groups.
        (marks.len(), limit)
    };

    let mut ranked = std::mem::take(marks);
    let tail = ranked.split_off(pool_len);
    // What a fold may never mix. Pane, because a tape mark and a candle mark
    // are clipped, sized and switched on and off separately — merging them
    // draws one pane's volume inside the other. Side, because a buy and a sell
    // are not one pressure. Bar, because a mark drawn inside a bar's slot is a
    // claim that *that* bar took the volume it carries, and merging a
    // neighbour's prints into it is a fabricated fact about who traded when —
    // the same reason `regionalize_clusters` keeps `bar_index` in its key. The
    // tape has no bars to confuse: it is one continuous band, so its marks key
    // on `None` and the whole pane is one group per side.
    let mut groups: BTreeMap<(bool, u8, Option<usize>), Vec<AggressionPrimitive>> = BTreeMap::new();
    for mark in ranked {
        let bar = if mark.live {
            None
        } else {
            bars.and_then(|timeline| {
                timeline
                    .locate(mark.first_timestamp_ms)
                    .map(|position| position.bar_index)
            })
        };
        groups
            .entry((mark.live, side_key(mark.side), bar))
            .or_default()
            .push(mark);
    }

    // Enough marks per fold that the pane fits, given how many groups there
    // are: two part-full groups are two marks, not one, and a budget that
    // ignored that would be quietly overspent. When the groups alone already
    // outnumber the target no group size can reach it, so the search stops
    // rather than walking `group` up to `pool_len` one step at a time.
    let mut group = pool_len.div_ceil(target.max(1)).max(2);
    if groups.len() <= target.max(1) {
        let folds_at = |size: usize| -> usize {
            groups
                .values()
                .map(|members| members.len().div_ceil(size))
                .sum()
        };
        while group < pool_len && folds_at(group) > target.max(1) {
            group += 1;
        }
    }

    let mut folded: Vec<AggressionPrimitive> = Vec::with_capacity(limit + 2);
    for mut members in groups.into_values() {
        while members.len() > group {
            let rest = members.split_off(group);
            folded.push(fold_chunk(members, reference));
            members = rest;
        }
        folded.push(fold_chunk(members, reference));
    }
    folded.extend(tail);
    *marks = folded;
}
