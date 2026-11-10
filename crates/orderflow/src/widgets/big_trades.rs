//! The Big Trades pane's model: prints at or above a notional threshold,
//! newest first, with buy and sell totals over the last five minutes of a
//! clock the caller passes in.
//!
//! With merging on, fills one taker side sent in the same exchange
//! millisecond fold into one row before the threshold applies — a sweep
//! across several prices is one decision, and only merged can it clear the
//! filter. A merged row keeps its fill count and says it was merged.
//!
//! The fold is one pass keyed on (millisecond, side). It does not go through
//! `interaction`'s cluster folds: those key on a price bucket and dedupe
//! aggregate-trade ids, and an id is not unique across a retained tape (an
//! MT5 bridge restarts its sequence per session), so a fold by id would drop
//! real fills. Every print counts once, by position.

use std::collections::BTreeMap;

use quantick_engine::Side;
use rust_decimal::Decimal;

use crate::history::Aggression;

/// The span the footer totals cover, ending at the passed clock.
pub const BIG_TRADES_WINDOW_MS: i64 = 300_000;

/// The threshold presets the pane offers, in quote units.
pub const MIN_NOTIONAL_PRESETS: [u32; 3] = [50_000, 250_000, 1_000_000];

/// How the tape is filtered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigTradesParams {
    /// Smallest notional (price times quantity, in quote units) a row may
    /// carry.
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
    /// Summed price times quantity, exact per print.
    pub notional: Decimal,
    /// Prints behind the row.
    pub fill_count: usize,
    /// The row folds more than one print.
    pub merged: bool,
}

/// The filtered list and the footer totals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigTrades {
    /// Newest first, at most [`BigTradesParams::max_rows`].
    pub rows: Vec<BigTrade>,
    /// Buy notional of every passing row newer than `now - window`.
    pub window_buy_notional: Decimal,
    /// Sell notional of every passing row newer than `now - window`.
    pub window_sell_notional: Decimal,
}

/// Filter `prints` (any order) at `params`, totalling the window ending at
/// `now_ms`. A row stamped after `now_ms` (a caller clock behind the venue)
/// still counts as recent, so the totals never omit a row the list shows.
#[must_use]
pub fn big_trades(prints: &[Aggression], params: &BigTradesParams, now_ms: i64) -> BigTrades {
    // Key: time, side, and — unmerged — the print's position, so every print
    // stays its own row.
    let mut folds: BTreeMap<(i64, bool, usize), Fold> = BTreeMap::new();
    for (index, print) in prints.iter().enumerate() {
        let position = if params.merge_fills { 0 } else { index };
        let key = (print.timestamp_ms, print.side == Side::Sell, position);
        folds
            .entry(key)
            .and_modify(|fold| fold.push(print))
            .or_insert_with(|| Fold::new(print));
    }
    let mut rows: Vec<BigTrade> = folds
        .into_values()
        .map(Fold::finish)
        .filter(|row| row.notional >= params.min_notional)
        .collect();
    rows.sort_by(|a, b| {
        newest_first_key(b)
            .cmp(&newest_first_key(a))
            .then_with(|| b.price.cmp(&a.price))
            .then_with(|| b.quantity.cmp(&a.quantity))
    });

    let window_start = now_ms.saturating_sub(BIG_TRADES_WINDOW_MS);
    let mut window_buy_notional = Decimal::ZERO;
    let mut window_sell_notional = Decimal::ZERO;
    for row in rows.iter().filter(|row| row.timestamp_ms > window_start) {
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

/// Time, id and side: the order rows are listed in, newest first once
/// reversed. Price and quantity break what is left of a tie.
fn newest_first_key(row: &BigTrade) -> (i64, u64, bool) {
    (row.timestamp_ms, row.agg_id, row.side == Side::Sell)
}

/// One row being summed.
struct Fold {
    agg_id: u64,
    timestamp_ms: i64,
    side: Side,
    quantity: Decimal,
    notional: Decimal,
    fill_count: usize,
}

impl Fold {
    fn new(print: &Aggression) -> Self {
        Self {
            agg_id: print.agg_id,
            timestamp_ms: print.timestamp_ms,
            side: print.side,
            quantity: print.quantity,
            notional: print.price * print.quantity,
            fill_count: 1,
        }
    }

    fn push(&mut self, print: &Aggression) {
        self.agg_id = self.agg_id.min(print.agg_id);
        self.quantity += print.quantity;
        self.notional += print.price * print.quantity;
        self.fill_count += 1;
    }

    fn finish(self) -> BigTrade {
        let price = if self.quantity > Decimal::ZERO {
            self.notional / self.quantity
        } else {
            Decimal::ZERO
        };
        BigTrade {
            agg_id: self.agg_id,
            timestamp_ms: self.timestamp_ms,
            side: self.side,
            price,
            quantity: self.quantity,
            notional: self.notional,
            fill_count: self.fill_count,
            merged: self.fill_count > 1,
        }
    }
}
