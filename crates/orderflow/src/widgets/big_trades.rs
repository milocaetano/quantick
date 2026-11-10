//! The Big Trades pane's model: prints at or above a notional threshold,
//! newest first, with buy and sell totals over the last five minutes of a
//! clock the caller passes in.
//!
//! With merging on, fills one taker side sent in the same exchange
//! millisecond fold into one row before the threshold applies — a sweep
//! across several prices is one decision, and only merged can it clear the
//! filter. A merged row keeps its fill count and says it was merged.

use std::collections::BTreeMap;

use quantick_engine::Side;
use rust_decimal::Decimal;

use crate::config::DisplayGrouping;
use crate::grouping::EffectiveGrouping;
use crate::history::Aggression;
use crate::interaction::{AggressionCluster, cluster_aggressions, fold_by_key};

/// The span the footer totals cover, ending at the passed clock.
pub const BIG_TRADES_WINDOW_MS: i64 = 300_000;

/// The threshold presets the pane offers, in quote currency.
pub const MIN_NOTIONAL_PRESETS: [u32; 3] = [50_000, 250_000, 1_000_000];

/// How the tape is filtered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigTradesParams {
    /// Smallest quote notional (price times quantity) a row may carry.
    pub min_notional: Decimal,
    /// Fold same-millisecond, same-side fills into one row first.
    pub merge_fills: bool,
    /// Most rows kept, newest first. The totals ignore this cap.
    pub max_rows: usize,
}

/// One row: a print, or a merge of same-millisecond fills.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigTrade {
    /// The lowest aggregate-trade id behind the row.
    pub agg_id: u64,
    /// Exchange time of the row's fills.
    pub timestamp_ms: i64,
    /// Taker side.
    pub side: Side,
    /// Quantity-weighted price.
    pub price: Decimal,
    /// Summed quantity.
    pub quantity: Decimal,
    /// Summed price times quantity.
    pub notional: Decimal,
    /// Aggregate trades behind the row.
    pub fill_count: usize,
    /// The row folds more than one fill.
    pub merged: bool,
}

/// The filtered list and the footer totals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigTrades {
    /// Newest first, at most [`BigTradesParams::max_rows`].
    pub rows: Vec<BigTrade>,
    /// Buy notional of every passing row in `(now - window, now]`.
    pub window_buy_notional: Decimal,
    /// Sell notional of every passing row in `(now - window, now]`.
    pub window_sell_notional: Decimal,
}

/// Filter `prints` (any order) at `params`, totalling the window ending at
/// `now_ms`.
#[must_use]
pub fn big_trades(prints: &[Aggression], params: &BigTradesParams, now_ms: i64) -> BigTrades {
    // Raw mode (no time window) gives one cluster per print; the grouping
    // only names a bucket nothing here reads.
    let grouping = EffectiveGrouping::resolve(DisplayGrouping::Native, Decimal::ONE, Decimal::ZERO);
    let mut clusters = cluster_aggressions(prints, &[], grouping, 0);
    if params.merge_fills {
        clusters = fold_by_key(clusters, |cluster| {
            Some((cluster.first_timestamp_ms, side_key(cluster.side)))
        });
    }
    // Notional is summed per print, exactly, rather than re-derived from
    // the merged average price.
    let notional_of: BTreeMap<u64, Decimal> = prints
        .iter()
        .map(|print| (print.agg_id, print.price * print.quantity))
        .collect();
    let mut rows: Vec<BigTrade> = clusters
        .into_iter()
        .map(|cluster| row(cluster, &notional_of))
        .filter(|row| row.notional >= params.min_notional)
        .collect();
    rows.sort_by(|a, b| (b.timestamp_ms, b.agg_id).cmp(&(a.timestamp_ms, a.agg_id)));

    let window_start = now_ms.saturating_sub(BIG_TRADES_WINDOW_MS);
    let mut window_buy_notional = Decimal::ZERO;
    let mut window_sell_notional = Decimal::ZERO;
    for row in rows
        .iter()
        .filter(|row| row.timestamp_ms > window_start && row.timestamp_ms <= now_ms)
    {
        match row.side {
            Side::Buy => window_buy_notional += row.notional,
            Side::Sell => window_sell_notional += row.notional,
        }
    }
    rows.truncate(params.max_rows);
    BigTrades {
        rows,
        window_buy_notional,
        window_sell_notional,
    }
}

fn row(cluster: AggressionCluster, notional_of: &BTreeMap<u64, Decimal>) -> BigTrade {
    let notional = cluster
        .agg_ids
        .iter()
        .filter_map(|agg_id| notional_of.get(agg_id))
        .sum();
    BigTrade {
        agg_id: cluster.agg_id,
        timestamp_ms: cluster.first_timestamp_ms,
        side: cluster.side,
        notional,
        price: cluster.price,
        quantity: cluster.quantity,
        fill_count: cluster.trade_count,
        merged: cluster.trade_count > 1,
    }
}

fn side_key(side: Side) -> u8 {
    match side {
        Side::Buy => 0,
        Side::Sell => 1,
    }
}
