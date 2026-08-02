//! Manual public-mainnet proof for every symbol shipped in the default config.

use std::time::Duration;

use quantick_feed_hyperliquid::{
    HYPERLIQUID_WS_URL, TradeMapper,
    depth::{BookMapper, run_depth_session},
    run_trade_session,
};
use quantick_orderbook::DepthEvent;

const SEEDED_MARKETS: [&str; 5] = ["BTC", "ETH", "HYPE", "SOL", "ZEC"];
const MARKET_TIMEOUT: Duration = Duration::from_secs(20);

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "connects to Hyperliquid mainnet; run manually before release"]
async fn all_seeded_markets_stream_trades_and_l2() {
    for symbol in SEEDED_MARKETS {
        let (trade_tx, mut trade_rx) = tokio::sync::mpsc::channel(64);
        let symbol_owned = symbol.to_owned();
        let trade_task = tokio::spawn(async move {
            let mut mapper = TradeMapper::new(&symbol_owned);
            run_trade_session(HYPERLIQUID_WS_URL, &symbol_owned, &trade_tx, &mut mapper).await
        });
        let trades = tokio::time::timeout(MARKET_TIMEOUT, trade_rx.recv())
            .await
            .unwrap_or_else(|_| panic!("{symbol}: timed out waiting for a trade batch"))
            .unwrap_or_else(|| panic!("{symbol}: trade channel closed"));
        assert!(!trades.is_empty(), "{symbol}: empty trade batch");
        assert!(
            trades
                .iter()
                .all(|trade| trade.price > rust_decimal::Decimal::ZERO),
            "{symbol}"
        );
        assert!(
            trades
                .iter()
                .all(|trade| trade.quantity > rust_decimal::Decimal::ZERO),
            "{symbol}"
        );
        trade_task.abort();
        let _ = trade_task.await;

        let (depth_tx, mut depth_rx) = tokio::sync::mpsc::channel(64);
        let symbol_owned = symbol.to_owned();
        let depth_task = tokio::spawn(async move {
            let mut mapper = BookMapper::new(&symbol_owned, 1);
            run_depth_session(HYPERLIQUID_WS_URL, &symbol_owned, &depth_tx, &mut mapper, 1).await
        });
        let snapshot = tokio::time::timeout(MARKET_TIMEOUT, async {
            loop {
                match depth_rx.recv().await {
                    Some(event @ DepthEvent::Snapshot { .. }) => return event,
                    Some(_) => {}
                    None => panic!("{symbol}: depth channel closed"),
                }
            }
        })
        .await
        .unwrap_or_else(|_| panic!("{symbol}: timed out waiting for an L2 snapshot"));
        let DepthEvent::Snapshot { snapshot, .. } = snapshot else {
            unreachable!()
        };
        assert!(!snapshot.bids().is_empty(), "{symbol}: empty bids");
        assert!(!snapshot.asks().is_empty(), "{symbol}: empty asks");
        depth_task.abort();
        let _ = depth_task.await;

        println!("{symbol}: {} trades + L2 OK", trades.len());
    }
}
