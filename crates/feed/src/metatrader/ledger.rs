//! What the MetaTrader feed remembers across bridge sessions, as pure state.
//!
//! The listener hands the feed a stream of [`Mt5Event`]s; the feed forwards
//! trades to the chart under three rules the module docs state (reconnect
//! overlap dropped, opening block prepended only into an empty chart, every
//! *load older* answered exactly once). Those rules are decisions over a
//! handful of cursors, and they live here, where each transition is a method
//! that takes the event's data and returns what to forward — no channel, no
//! clock, no log. The driver in `metatrader.rs` does the sending and logging.
//!
//! [`Mt5Event`]: quantick_feed_mt5::Mt5Event

use quantick_engine::Trade;

use super::OhlcvBlock;
use crate::FeedContinuity;

/// The earlier of two optional timestamps, treating `None` as "no opinion".
///
/// Not `Option::min`: that orders `None` *below* every `Some`, so folding a
/// fresh batch into an empty cursor with it yields `None` — a chart that just
/// drew its opening block would go on reporting it holds nothing, and every
/// "load older" would be refused as "nothing charted yet".
pub(super) fn earlier(current: Option<i64>, candidate: Option<i64>) -> Option<i64> {
    match (current, candidate) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (held, None) => held,
        (None, fresh) => fresh,
    }
}

/// The trade cursors the forwarding rules are decided against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TapeLedger {
    /// Whether any trade reached the UI yet: the first non-empty history block
    /// may be prepended only into an empty chart.
    forwarded_any: bool,
    /// Whether the bridge has re-sent an opening block, which only a reconnect
    /// does. Set where the reconnect is already detected, never on the first
    /// block: setting it there made the session's *own* slices look like a
    /// repeat and dropped every one of them.
    opening_block_resent: bool,
    /// Newest trade timestamp forwarded to the UI. Reconnect history overlaps
    /// what was already streamed live; only strictly-newer trades pass.
    last_forwarded_ms: i64,
    /// Where a lost session left the tape, owed as a continuity gap to the
    /// first trade forwarded after it.
    pending_gap: Option<i64>,
    /// Oldest trade timestamp forwarded to the UI: the floor a page's overlap
    /// is trimmed against.
    ///
    /// Tracked here rather than asked of the chart because this is the only
    /// place that sees every trade *before* the UI decides what to keep: a tab
    /// that trimmed its retained window would otherwise page from the trim
    /// point and re-fetch what it just dropped, forever. `None` until
    /// something has been forwarded — there is no "older than nothing".
    oldest_forwarded_ms: Option<i64>,
    /// Where the *next* page is asked from — deliberately not the same number.
    ///
    /// A page can move the search hours and yield no trades at all: a pre-open
    /// stretch is thousands of quote-only ticks that map to nothing, and a
    /// window over a closed market holds none to begin with. Paging from the
    /// oldest *trade* would re-request that identical window on every click
    /// and the trader could never get past it. So this follows whichever is
    /// older, the oldest trade in hand or how far the bridge said it searched.
    paging_floor_ms: Option<i64>,
    /// Whether a request is outstanding with no reply yet.
    ///
    /// The chart counts loads: `Tab::request_older_history` begins one for
    /// every command it queues, and only a `HistoryPrepended` ends one. So
    /// every command must be answered exactly once, including the ones no
    /// bridge will ever serve — a session that died holding one, a listener
    /// that never came back. This flag is what lets those be answered here.
    page_outstanding: bool,
}

/// What became of one history block the bridge pushed.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum HistoryBlock {
    /// Nothing in it; nothing to do.
    Empty,
    /// The chart's first trades: prepend them whole.
    Opening(Vec<Trade>),
    /// Reconnect history over a chart that already holds trades: only the
    /// unseen tail goes on, as live, through [`TapeLedger::forward`].
    Recovered {
        fresh: Vec<Trade>,
        overlap_dropped: usize,
    },
}

/// What became of one slice of the opening session.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum OpeningSlice {
    /// A reconnect re-ran the opening block over a session the chart already
    /// holds; the slice is refused whole.
    AlreadyHeld { count: usize },
    /// Prepend the trades strictly older than the chart's oldest.
    Prepend {
        older: Vec<Trade>,
        overlap_dropped: usize,
    },
}

/// A page (or slice) trimmed to what is strictly older than the chart holds.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Trimmed {
    pub(super) older: Vec<Trade>,
    pub(super) overlap_dropped: usize,
}

impl Default for TapeLedger {
    fn default() -> Self {
        Self {
            forwarded_any: false,
            opening_block_resent: false,
            last_forwarded_ms: i64::MIN,
            pending_gap: None,
            oldest_forwarded_ms: None,
            paging_floor_ms: None,
            page_outstanding: false,
        }
    }
}

impl TapeLedger {
    /// Where the next *load older* is asked from; `None` while nothing is
    /// charted.
    pub(super) fn paging_floor_ms(&self) -> Option<i64> {
        self.paging_floor_ms
    }

    /// A trade is about to go out as live. Returns the continuity gap a lost
    /// session left owed, which goes out first.
    pub(super) fn forward(&mut self, trade: &Trade) -> Option<FeedContinuity> {
        self.forwarded_any = true;
        self.last_forwarded_ms = self.last_forwarded_ms.max(trade.timestamp_ms);
        // A live print sets the floor only on a chart that has none: after
        // that the oldest trade is behind, not ahead, and `min` keeps it there.
        self.oldest_forwarded_ms = earlier(self.oldest_forwarded_ms, Some(trade.timestamp_ms));
        self.paging_floor_ms = earlier(self.paging_floor_ms, self.oldest_forwarded_ms);
        crate::continuity::mt5_reconnect_gap(trade, &mut self.pending_gap)
    }

    /// The bridge pushed its recent-history block.
    pub(super) fn history_block(&mut self, batch: Vec<Trade>) -> HistoryBlock {
        if batch.is_empty() {
            return HistoryBlock::Empty;
        }
        if self.forwarded_any {
            // Reconnect history: forward only what the UI has not already
            // seen. The opening slices that follow this block cover a session
            // the chart already holds, so they are refused from now on.
            self.opening_block_resent = true;
            let resent = batch.len();
            let fresh: Vec<_> = batch
                .into_iter()
                .filter(|t| t.timestamp_ms > self.last_forwarded_ms)
                .collect();
            let overlap_dropped = resent - fresh.len();
            return HistoryBlock::Recovered {
                fresh,
                overlap_dropped,
            };
        }
        self.forwarded_any = true;
        self.last_forwarded_ms = batch
            .iter()
            .map(|t| t.timestamp_ms)
            .max()
            .unwrap_or(self.last_forwarded_ms);
        self.oldest_forwarded_ms = earlier(
            self.oldest_forwarded_ms,
            batch.iter().map(|t| t.timestamp_ms).min(),
        );
        self.paging_floor_ms = earlier(self.paging_floor_ms, self.oldest_forwarded_ms);
        HistoryBlock::Opening(batch)
    }

    /// One slice of the opening session arrived behind the chart's first.
    ///
    /// Nobody asked for it, so `page_outstanding` is deliberately left alone:
    /// a trader who pressed *+ older* while the morning was still filling in
    /// is still owed the answer to *that*.
    pub(super) fn opening_slice(&mut self, trades: Vec<Trade>) -> OpeningSlice {
        if self.opening_block_resent {
            return OpeningSlice::AlreadyHeld {
                count: trades.len(),
            };
        }
        let Trimmed {
            older,
            overlap_dropped,
        } = self.trim_older(trades);
        // The next *+ older* press starts below the whole opening block, not
        // below the slice the chart first painted — otherwise the first press
        // would re-fetch the morning that just arrived.
        self.paging_floor_ms = earlier(self.paging_floor_ms, self.oldest_forwarded_ms);
        OpeningSlice::Prepend {
            older,
            overlap_dropped,
        }
    }

    /// The reply a *load older* was owed.
    pub(super) fn history_page(
        &mut self,
        trades: Vec<Trade>,
        scanned_to_utc_ms: Option<i64>,
    ) -> Trimmed {
        // Cleared before anything can fail, so no later exit leaves the flag
        // set on a task that is ending.
        self.page_outstanding = false;
        let trimmed = self.trim_older(trades);
        // The search cursor follows the *search*, so a page that crossed hours
        // of quote-only ticks and mapped none of them still moves the next
        // click past them. A bridge that reports nothing leaves this on the
        // trades, which is the old behaviour and no worse than it.
        self.paging_floor_ms = earlier(
            earlier(self.paging_floor_ms, self.oldest_forwarded_ms),
            scanned_to_utc_ms,
        );
        trimmed
    }

    /// A *load older* went on the wire; its reply is now owed.
    pub(super) fn page_asked(&mut self) {
        self.page_outstanding = true;
    }

    /// The bridge session was lost. Arms the continuity gap when anything was
    /// forwarded, and returns whether a page request died with the session
    /// and must be answered empty here.
    pub(super) fn session_lost(&mut self) -> bool {
        if self.forwarded_any {
            self.pending_gap = Some(self.last_forwarded_ms);
        }
        std::mem::take(&mut self.page_outstanding)
    }

    /// The listener ended. Returns whether a page request is still owed an
    /// answer nobody downstream will give.
    pub(super) fn listener_ended(&mut self) -> bool {
        std::mem::take(&mut self.page_outstanding)
    }

    /// Keep only trades strictly older than the chart's oldest. The bridge
    /// answers on whole-second boundaries (`copy_ticks_range` takes no finer
    /// unit), so a page can carry the far side of the cursor's own
    /// millisecond — prepending those would draw prints twice.
    fn trim_older(&mut self, trades: Vec<Trade>) -> Trimmed {
        let served = trades.len();
        let floor = self.oldest_forwarded_ms.unwrap_or(i64::MAX);
        let older: Vec<_> = trades
            .into_iter()
            .filter(|t| t.timestamp_ms < floor)
            .collect();
        self.oldest_forwarded_ms = earlier(
            self.oldest_forwarded_ms,
            older.iter().map(|t| t.timestamp_ms).min(),
        );
        Trimmed {
            overlap_dropped: served - older.len(),
            older,
        }
    }
}

/// The candle block a bridge pushed, held for whoever asks later.
///
/// Every other provider fetches candles when the pane requests them. Nothing
/// on MetaTrader answers that: the back-channel carries one message and it is
/// for ticks, the Expert Advisor never reads its socket at all, and no bridge
/// implements a candle request — so the block arrives when the bridge decides
/// and simply does. Holding it here is what lets this provider answer the same
/// `FetchOhlcv` as the others: the request does not reach a venue, it reads
/// what already arrived.
#[derive(Default)]
pub(super) struct CandleShelf {
    block: Option<OhlcvBlock>,
    /// How many times the candle answer has changed. The boolean capability
    /// is a latch — it rises with the first block and cannot fall — so an
    /// empty first block would otherwise be the last word: a consumer that
    /// cached that emptiness would never see another edge, and the full block
    /// from the next routine reconnect would be held forever behind a pane
    /// that stopped asking. Every block moves this, including a replacement.
    generation: u64,
}

impl CandleShelf {
    pub(super) fn block(&self) -> Option<&OhlcvBlock> {
        self.block.as_ref()
    }

    pub(super) fn generation(&self) -> u64 {
        self.generation
    }

    /// Hold a new block, replacing any earlier one. Returns the generation the
    /// capability must now publish.
    pub(super) fn store(&mut self, block: OhlcvBlock) -> u64 {
        self.block = Some(block);
        self.generation = self.generation.saturating_add(1);
        self.generation
    }
}

#[cfg(test)]
mod ledger_tests {
    use rust_decimal::Decimal;

    use super::*;

    fn at(ms: i64) -> Trade {
        Trade {
            agg_id: u64::try_from(ms).expect("test times are positive"),
            timestamp_ms: ms,
            price: Decimal::ONE,
            quantity: Decimal::ONE,
            side: quantick_engine::Side::Buy,
        }
    }

    fn times(trades: &[Trade]) -> Vec<i64> {
        trades.iter().map(|t| t.timestamp_ms).collect()
    }

    #[test]
    fn earlier_treats_none_as_no_opinion() {
        assert_eq!(earlier(None, None), None);
        assert_eq!(earlier(Some(5), None), Some(5));
        assert_eq!(earlier(None, Some(7)), Some(7));
        assert_eq!(earlier(Some(5), Some(7)), Some(5));
    }

    #[test]
    fn an_empty_block_changes_nothing() {
        let mut ledger = TapeLedger::default();
        assert_eq!(ledger.history_block(Vec::new()), HistoryBlock::Empty);
        assert_eq!(ledger, TapeLedger::default());
    }

    #[test]
    fn the_first_block_is_prepended_and_sets_the_paging_floor() {
        let mut ledger = TapeLedger::default();
        let block = ledger.history_block(vec![at(10), at(30), at(20)]);
        assert!(matches!(block, HistoryBlock::Opening(trades) if trades.len() == 3));
        assert_eq!(ledger.paging_floor_ms(), Some(10));
    }

    #[test]
    fn reconnect_history_forwards_only_the_unseen_tail() {
        let mut ledger = TapeLedger::default();
        let _ = ledger.history_block(vec![at(10), at(20)]);
        assert_eq!(ledger.forward(&at(30)), None);
        let HistoryBlock::Recovered {
            fresh,
            overlap_dropped,
        } = ledger.history_block(vec![at(20), at(30), at(40)])
        else {
            panic!("a second block is reconnect history");
        };
        // Same millisecond as the last forwarded is dropped too.
        assert_eq!(times(&fresh), [40]);
        assert_eq!(overlap_dropped, 2);
    }

    #[test]
    fn a_lost_session_owes_a_gap_to_the_next_forwarded_trade() {
        let mut ledger = TapeLedger::default();
        assert_eq!(ledger.forward(&at(100)), None);
        assert!(!ledger.session_lost(), "no page was outstanding");
        let gap = ledger.forward(&at(250)).expect("the gap is owed");
        let span = gap.gap.expect("a forward gap");
        assert_eq!((span.from_ms, span.to_ms), (100, 250));
        // Paid once.
        assert_eq!(ledger.forward(&at(260)), None);
    }

    #[test]
    fn a_session_lost_before_any_trade_owes_no_gap() {
        let mut ledger = TapeLedger::default();
        let _ = ledger.session_lost();
        assert_eq!(ledger.forward(&at(5)), None);
    }

    #[test]
    fn an_outstanding_page_is_owed_by_whichever_ending_comes_first() {
        let mut ledger = TapeLedger::default();
        ledger.page_asked();
        assert!(ledger.session_lost(), "the lost session must answer it");
        assert!(!ledger.listener_ended(), "and only once");

        ledger.page_asked();
        assert!(ledger.listener_ended());
        assert!(!ledger.session_lost());
    }

    #[test]
    fn a_page_reply_clears_the_debt_and_trims_the_overlap() {
        let mut ledger = TapeLedger::default();
        let _ = ledger.forward(&at(1_000));
        ledger.page_asked();
        let Trimmed {
            older,
            overlap_dropped,
        } = ledger.history_page(vec![at(900), at(1_000), at(1_100)], None);
        assert_eq!(times(&older), [900]);
        assert_eq!(overlap_dropped, 2);
        assert_eq!(ledger.paging_floor_ms(), Some(900));
        assert!(!ledger.listener_ended(), "the reply paid the debt");
    }

    #[test]
    fn an_empty_page_still_moves_the_floor_to_where_the_search_reached() {
        let mut ledger = TapeLedger::default();
        let _ = ledger.forward(&at(1_000));
        let trimmed = ledger.history_page(Vec::new(), Some(400));
        assert!(trimmed.older.is_empty());
        assert_eq!(ledger.paging_floor_ms(), Some(400));
    }

    #[test]
    fn opening_slices_fill_a_first_session_and_are_refused_after_a_reconnect() {
        let mut ledger = TapeLedger::default();
        let _ = ledger.history_block(vec![at(500), at(600)]);
        let OpeningSlice::Prepend {
            older,
            overlap_dropped,
        } = ledger.opening_slice(vec![at(300), at(500)])
        else {
            panic!("a first session keeps its slices");
        };
        assert_eq!(times(&older), [300]);
        assert_eq!(overlap_dropped, 1);
        assert_eq!(ledger.paging_floor_ms(), Some(300));

        // A reconnect re-sends the block: every later slice is already held.
        let _ = ledger.history_block(vec![at(600)]);
        assert_eq!(
            ledger.opening_slice(vec![at(100), at(200)]),
            OpeningSlice::AlreadyHeld { count: 2 }
        );
    }

    #[test]
    fn an_opening_slice_leaves_an_outstanding_page_owed() {
        let mut ledger = TapeLedger::default();
        let _ = ledger.history_block(vec![at(500)]);
        ledger.page_asked();
        let _ = ledger.opening_slice(vec![at(100)]);
        assert!(
            ledger.listener_ended(),
            "the press is still owed its answer"
        );
    }

    #[test]
    fn every_candle_block_moves_the_generation() {
        let mut shelf = CandleShelf::default();
        assert!(shelf.block().is_none());
        assert_eq!(shelf.generation(), 0);
        let block = |complete| OhlcvBlock {
            interval_ms: 60_000,
            bars: Vec::new(),
            complete,
        };
        assert_eq!(shelf.store(block(false)), 1);
        assert_eq!(shelf.store(block(true)), 2);
        assert!(shelf.block().is_some_and(|held| held.complete));
    }
}
