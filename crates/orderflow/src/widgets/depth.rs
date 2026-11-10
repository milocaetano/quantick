//! The Depth pane's model: a DOM ladder around the last price.
//!
//! Each row is one tick of price carrying what traded there (SOLD, BOUGHT)
//! and what rests there now (BID, ASK). Given the book the pane saw before,
//! a resting size that shrank by more than the trades at that price on that
//! side explain is reported as an [`UNATTRIBUTED_L2_REDUCTION`]: the book
//! does not say whether it was a cancel, a modify or an unseen fill, so the
//! model does not either.

use std::collections::BTreeMap;

use quantick_engine::Side;
use quantick_orderbook::{BookLevel, BookSide};
use rust_decimal::Decimal;

use crate::config::DisplayGrouping;
use crate::engine::BookLadder;
use crate::grouping::{EffectiveGrouping, bucket_for_price};
use crate::history::Aggression;
use crate::interaction::consumed_side;

/// What a resting size that dropped without enough trades to explain it is
/// called. Never "cancel": L2 cannot tell a cancel from a hidden fill.
pub const UNATTRIBUTED_L2_REDUCTION: &str = "unattributed L2 reduction";

/// The band around the mid the footer sums resting size within: 0.1%.
pub const NEAR_TOUCH_FRACTION: Decimal = Decimal::from_parts(1, 0, 0, false, 3);

/// A resting size at least this many times the median displayed size is a
/// wall.
pub const WALL_MEDIAN_MULTIPLE: u32 = 4;

/// Everything one depth build reads.
#[derive(Debug, Clone, Copy)]
pub struct DepthInput<'a> {
    /// The book the pane saw before, and the exchange time it was current
    /// at. Prints after that time, up to [`ladder_at_ms`](Self::ladder_at_ms),
    /// are the evidence a reduction is checked against. `None` labels
    /// nothing.
    pub earlier: Option<(&'a BookLadder, i64)>,
    /// The current book.
    pub ladder: &'a BookLadder,
    /// The exchange time the current book is current at: a print after it
    /// cannot explain a change the book already shows.
    pub ladder_at_ms: i64,
    /// The tape the SOLD and BOUGHT columns sum, in any order.
    pub prints: &'a [Aggression],
    /// Row height in price.
    pub tick: Decimal,
    /// Rows above and below the last price's row.
    pub rows_per_side: usize,
}

/// One price row, top of the ladder first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepthRow {
    /// The row's lower price edge.
    pub price: Decimal,
    /// Taker sells executed in the row.
    pub sold: Decimal,
    /// Resting bids in the row now.
    pub bid: Decimal,
    /// Resting asks in the row now.
    pub ask: Decimal,
    /// Taker buys executed in the row.
    pub bought: Decimal,
    /// The row holding the last price.
    pub last: bool,
    /// A resting size [`WALL_MEDIAN_MULTIPLE`] times the displayed median.
    pub wall: bool,
    /// Bid size that left the row with no trade to account for it.
    pub bid_unattributed: Decimal,
    /// Ask size that left the row with no trade to account for it.
    pub ask_unattributed: Decimal,
}

impl DepthRow {
    /// [`UNATTRIBUTED_L2_REDUCTION`] when either side lost size no trade
    /// explains.
    #[must_use]
    pub fn reduction_label(&self) -> Option<&'static str> {
        (self.bid_unattributed > Decimal::ZERO || self.ask_unattributed > Decimal::ZERO)
            .then_some(UNATTRIBUTED_L2_REDUCTION)
    }
}

/// The ladder and its footer figures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepthModel {
    /// Rows from the highest price down.
    pub rows: Vec<DepthRow>,
    /// The latest print's price, or the mid when the tape is empty.
    pub last_price: Decimal,
    /// Best ask minus best bid; `None` on a one-sided book.
    pub spread: Option<Decimal>,
    /// Bid and ask size resting within [`NEAR_TOUCH_FRACTION`] of the mid;
    /// `None` on a one-sided book.
    pub near_touch: Option<(Decimal, Decimal)>,
}

/// Build the ladder. `None` when there is neither a print nor a two-sided
/// book to centre it on, or the tick is not positive.
#[must_use]
pub fn build_depth(input: &DepthInput<'_>) -> Option<DepthModel> {
    if input.tick <= Decimal::ZERO {
        return None;
    }
    let grouping = EffectiveGrouping::resolve(DisplayGrouping::Native, input.tick, Decimal::ZERO);
    let ladder = input.ladder;
    let mid = match (ladder.best_bid, ladder.best_ask) {
        (Some(bid), Some(ask)) => Some((bid.price() + ask.price()) / Decimal::TWO),
        _ => None,
    };
    let last_price = latest_print(input.prints)
        .map(|print| print.price)
        .or(mid)?;
    let centre = bucket_for_price(last_price, grouping);

    let bids = sum_levels(&ladder.bids, grouping);
    let asks = sum_levels(&ladder.asks, grouping);
    let mut sold = BTreeMap::new();
    let mut bought = BTreeMap::new();
    for print in input.prints {
        let column = match print.side {
            Side::Sell => &mut sold,
            Side::Buy => &mut bought,
        };
        *column
            .entry(bucket_for_price(print.price, grouping))
            .or_insert(Decimal::ZERO) += print.quantity;
    }
    let reductions = input
        .earlier
        .map(|(earlier, at_ms)| Reductions::new(earlier, at_ms, input, grouping));

    let span = Decimal::from(input.rows_per_side);
    let top = centre + span * input.tick;
    let mut rows: Vec<DepthRow> = (0..=input.rows_per_side * 2)
        .map(|index| {
            let price = top - Decimal::from(index) * input.tick;
            let cell = |map: &BTreeMap<Decimal, Decimal>| {
                map.get(&price).copied().unwrap_or(Decimal::ZERO)
            };
            let (bid_unattributed, ask_unattributed) = reductions
                .as_ref()
                .map_or((Decimal::ZERO, Decimal::ZERO), |reductions| {
                    reductions.at(price, &bids, &asks)
                });
            DepthRow {
                price,
                sold: cell(&sold),
                bid: cell(&bids),
                ask: cell(&asks),
                bought: cell(&bought),
                last: price == centre,
                wall: false,
                bid_unattributed,
                ask_unattributed,
            }
        })
        .collect();
    flag_walls(&mut rows);

    let spread = match (ladder.best_bid, ladder.best_ask) {
        (Some(bid), Some(ask)) => Some(ask.price() - bid.price()),
        _ => None,
    };
    let near_touch = mid.map(|mid| {
        let band = mid * NEAR_TOUCH_FRACTION;
        let within = |levels: &[BookLevel], keep: &dyn Fn(Decimal) -> bool| {
            levels
                .iter()
                .filter(|level| keep(level.price()))
                .map(|level| level.quantity())
                .sum::<Decimal>()
        };
        (
            within(&ladder.bids, &|price| price >= mid - band),
            within(&ladder.asks, &|price| price <= mid + band),
        )
    });
    Some(DepthModel {
        rows,
        last_price,
        spread,
        near_touch,
    })
}

/// The tape's latest print: highest exchange time, then highest id, then
/// highest price and quantity — a total order, so ids repeated across
/// sessions cannot make the answer depend on input order.
fn latest_print(prints: &[Aggression]) -> Option<&Aggression> {
    prints.iter().max_by(|a, b| {
        (a.timestamp_ms, a.agg_id, a.price, a.quantity).cmp(&(
            b.timestamp_ms,
            b.agg_id,
            b.price,
            b.quantity,
        ))
    })
}

fn sum_levels(levels: &[BookLevel], grouping: EffectiveGrouping) -> BTreeMap<Decimal, Decimal> {
    let mut rows = BTreeMap::new();
    for level in levels {
        *rows
            .entry(bucket_for_price(level.price(), grouping))
            .or_insert(Decimal::ZERO) += level.quantity();
    }
    rows
}

/// What the earlier book held per row and what traded into each side since.
struct Reductions {
    bids: BTreeMap<Decimal, Decimal>,
    asks: BTreeMap<Decimal, Decimal>,
    traded_bids: BTreeMap<Decimal, Decimal>,
    traded_asks: BTreeMap<Decimal, Decimal>,
    /// The current book's deepest bid and ask. The ladder is published
    /// clipped (level count and view window) without saying where, so past
    /// its deepest level a missing size is not evidence of anything and is
    /// left unlabelled — no claim rather than a guessed one. A pull of the
    /// outermost levels is therefore not labelled.
    bid_floor: Option<Decimal>,
    ask_ceiling: Option<Decimal>,
}

impl Reductions {
    fn new(
        earlier: &BookLadder,
        at_ms: i64,
        input: &DepthInput<'_>,
        grouping: EffectiveGrouping,
    ) -> Self {
        let mut traded_bids = BTreeMap::new();
        let mut traded_asks = BTreeMap::new();
        let evidence = input
            .prints
            .iter()
            .filter(|print| print.timestamp_ms > at_ms && print.timestamp_ms <= input.ladder_at_ms);
        for print in evidence {
            let side = match consumed_side(print.side) {
                BookSide::Bid => &mut traded_bids,
                BookSide::Ask => &mut traded_asks,
            };
            *side
                .entry(bucket_for_price(print.price, grouping))
                .or_insert(Decimal::ZERO) += print.quantity;
        }
        Self {
            bids: sum_levels(&earlier.bids, grouping),
            asks: sum_levels(&earlier.asks, grouping),
            traded_bids,
            traded_asks,
            bid_floor: input
                .ladder
                .bids
                .last()
                .map(|level| bucket_for_price(level.price(), grouping)),
            ask_ceiling: input
                .ladder
                .asks
                .last()
                .map(|level| bucket_for_price(level.price(), grouping)),
        }
    }

    /// Bid and ask size the row lost beyond what traded into it.
    fn at(
        &self,
        price: Decimal,
        bids_now: &BTreeMap<Decimal, Decimal>,
        asks_now: &BTreeMap<Decimal, Decimal>,
    ) -> (Decimal, Decimal) {
        let unexplained = |before: &BTreeMap<Decimal, Decimal>,
                           now: &BTreeMap<Decimal, Decimal>,
                           traded: &BTreeMap<Decimal, Decimal>| {
            let get = |map: &BTreeMap<Decimal, Decimal>| {
                map.get(&price).copied().unwrap_or(Decimal::ZERO)
            };
            (get(before) - get(now) - get(traded)).max(Decimal::ZERO)
        };
        let bid = if self.bid_floor.is_some_and(|floor| price >= floor) {
            unexplained(&self.bids, bids_now, &self.traded_bids)
        } else {
            Decimal::ZERO
        };
        let ask = if self.ask_ceiling.is_some_and(|ceiling| price <= ceiling) {
            unexplained(&self.asks, asks_now, &self.traded_asks)
        } else {
            Decimal::ZERO
        };
        (bid, ask)
    }
}

/// Flag every row whose bid or ask is [`WALL_MEDIAN_MULTIPLE`] times the
/// median of the non-empty resting cells on display (lower middle on an
/// even count, so no average is invented).
fn flag_walls(rows: &mut [DepthRow]) {
    let mut sizes: Vec<Decimal> = rows
        .iter()
        .flat_map(|row| [row.bid, row.ask])
        .filter(|size| *size > Decimal::ZERO)
        .collect();
    if sizes.is_empty() {
        return;
    }
    sizes.sort_unstable();
    let threshold = sizes[(sizes.len() - 1) / 2] * Decimal::from(WALL_MEDIAN_MULTIPLE);
    for row in rows {
        row.wall = row.bid.max(row.ask) >= threshold;
    }
}
