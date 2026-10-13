//! Deterministic USD-M retention, inclusive paging and boundary recovery.

use std::cell::RefCell;

use quantick_feed_binance::{
    AggTradeSource, FeedError, backfill_before_with_boundary, backfill_with_boundary,
    wire::AggTrade,
};

struct RetainedSource {
    first: u64,
    failure_at: Option<u64>,
    requests: RefCell<Vec<Option<u64>>>,
}

impl RetainedSource {
    fn new(first: u64) -> Self {
        Self {
            first,
            failure_at: None,
            requests: RefCell::new(Vec::new()),
        }
    }
}

impl AggTradeSource for RetainedSource {
    async fn fetch(
        &self,
        _: &str,
        from: Option<u64>,
        limit: u32,
    ) -> Result<Vec<AggTrade>, FeedError> {
        self.requests.borrow_mut().push(from);
        if from.is_some() && from == self.failure_at {
            return Err(FeedError::Http("probe transport failed".to_owned()));
        }
        if from.is_some_and(|id| id < self.first) {
            return Err(FeedError::HistoryExpired);
        }
        let from = from.unwrap_or(3_001 - u64::from(limit));
        Ok((from..=3_000)
            .take(limit as usize)
            .map(|id| AggTrade {
                agg_id: id,
                price: "81200.10".to_owned(),
                quantity: "0.003".to_owned(),
                trade_time_ms: id as i64,
                is_buyer_maker: id % 2 == 0,
            })
            .collect())
    }
}

#[tokio::test]
async fn initial_backfill_keeps_recent_pages_and_recovers_the_exact_retained_suffix() {
    let source = RetainedSource::new(1_550);
    let batch = backfill_with_boundary(&source, "BTCUSDT.p", 2_000)
        .await
        .unwrap();
    assert!(batch.retention_limited);
    assert_eq!(batch.trades.len(), 1_451);
    assert_eq!(batch.trades.first().unwrap().agg_id, 1_550);
    assert_eq!(batch.trades.last().unwrap().agg_id, 3_000);
    assert!(
        batch
            .trades
            .windows(2)
            .all(|pair| pair[1].agg_id == pair[0].agg_id + 1)
    );
    assert!(source.requests.borrow().len() <= 13, "bounded probing");
}

#[tokio::test]
async fn first_expired_page_returns_empty_bounded_history_instead_of_a_failure() {
    let source = RetainedSource::new(1_550);
    let batch = backfill_before_with_boundary(&source, "BTCUSDT.p", 1_550, 2_000)
        .await
        .unwrap();
    assert!(batch.retention_limited);
    assert!(batch.trades.is_empty());
    assert_eq!(source.requests.borrow().len(), 2);
}

#[tokio::test]
async fn older_backfill_preserves_partial_success_and_excludes_the_loaded_cursor() {
    let source = RetainedSource::new(1_550);
    let batch = backfill_before_with_boundary(&source, "BTCUSDT.p", 3_001, 2_000)
        .await
        .unwrap();
    assert!(batch.retention_limited);
    assert_eq!(batch.trades.len(), 1_451);
    assert_eq!(batch.trades.first().unwrap().agg_id, 1_550);
    assert_eq!(batch.trades.last().unwrap().agg_id, 3_000);
    assert!(
        batch
            .trades
            .windows(2)
            .all(|pair| pair[1].agg_id == pair[0].agg_id + 1)
    );
}

#[tokio::test]
async fn normal_page_completion_is_not_labelled_retention_limited() {
    let source = RetainedSource::new(1_550);
    let batch = backfill_with_boundary(&source, "BTCUSDT.p", 50)
        .await
        .unwrap();
    assert!(!batch.retention_limited);
    assert_eq!(batch.trades.len(), 50);
    assert_eq!(source.requests.borrow().len(), 1);
}

#[tokio::test]
async fn trimmed_retention_pages_leave_the_remaining_older_trades_reachable() {
    for opening in [false, true] {
        let source = RetainedSource::new(1_550);
        let batch = if opening {
            backfill_with_boundary(&source, "BTCUSDT.p", 1_200).await
        } else {
            backfill_before_with_boundary(&source, "BTCUSDT.p", 3_001, 1_200).await
        }
        .unwrap();
        assert!(
            !batch.retention_limited,
            "the returned prefix has not reached expiry"
        );
        assert_eq!(batch.trades.len(), 1_200);
        assert_eq!(batch.trades.first().unwrap().agg_id, 1_801);
        let older = backfill_before_with_boundary(&source, "BTCUSDT.p", 1_801, 1_200)
            .await
            .unwrap();
        assert!(older.retention_limited);
        let ids: Vec<_> = older
            .trades
            .iter()
            .chain(&batch.trades)
            .map(|trade| trade.agg_id)
            .collect();
        assert_eq!(ids, (1_550..=3_000).collect::<Vec<_>>());
        let exhausted = backfill_before_with_boundary(&source, "BTCUSDT.p", 1_550, 1_200)
            .await
            .unwrap();
        assert!(exhausted.retention_limited);
        assert!(exhausted.trades.is_empty());
    }
}

#[tokio::test]
async fn an_exact_retained_count_reports_the_returned_boundary() {
    let source = RetainedSource::new(1_550);
    for opening in [false, true] {
        let batch = if opening {
            backfill_with_boundary(&source, "BTCUSDT.p", 1_451).await
        } else {
            backfill_before_with_boundary(&source, "BTCUSDT.p", 3_001, 1_451).await
        }
        .unwrap();
        assert_eq!(batch.trades.first().unwrap().agg_id, 1_550);
        assert!(batch.retention_limited);
    }
}

#[tokio::test]
async fn a_probe_transport_failure_is_not_misreported_as_expired_history() {
    let mut source = RetainedSource::new(1_550);
    source.failure_at = Some(2_000);
    let error = backfill_with_boundary(&source, "BTCUSDT.p", 2_000)
        .await
        .unwrap_err();
    assert_eq!(error, FeedError::Http("probe transport failed".to_owned()));
}
