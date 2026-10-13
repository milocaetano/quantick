//! USD-M routes and continuity exercised independently from Spot fixtures.

use quantick_feed_binance::{
    AggTradeSource, BinanceHttp, BinanceKlineHttp, BinanceMarket, KlineSource,
    depth::{
        BinanceDepthHttp, DepthApplyError, DepthSnapshotSource, DepthSynchronizer,
        SyncApplyOutcome, SyncPhase, parse_snapshot, parse_update,
    },
};
use quantick_orderbook::{BookError, OrderBook};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpListener;

async fn response(body: &'static str) -> (String, tokio::task::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut buffer = [0; 1024];
        while !request.windows(4).any(|part| part == b"\r\n\r\n") {
            let count = stream.read(&mut buffer).await.unwrap();
            assert!(count > 0, "client must send its request");
            request.extend_from_slice(&buffer[..count]);
        }
        let reply = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(reply.as_bytes()).await.unwrap();
        String::from_utf8(request)
            .unwrap()
            .lines()
            .next()
            .unwrap()
            .to_string()
    });
    (base, task)
}

#[tokio::test]
async fn perpetual_trades_use_futures_ids_and_total_quantity_without_display_suffix() {
    let (base, request) = response(
        r#"[{"a":42,"p":"81200.10","q":"0.123","nq":"0.100","f":81,"l":88,"T":1000,"m":true}]"#,
    )
    .await;
    let source = BinanceHttp::with_market_base(BinanceMarket::UsdM, base);
    let trades = source.fetch("BTCUSDT.p", Some(42), 1000).await.unwrap();
    assert_eq!(trades.len(), 1);
    assert_eq!(trades[0].to_trade().unwrap().quantity.to_string(), "0.123");
    assert_eq!(
        trades[0].to_trade().unwrap().side,
        quantick_engine::Side::Sell
    );
    let request = request.await.unwrap();
    assert!(request.starts_with("GET /fapi/v1/aggTrades?"), "{request}");
    assert!(request.contains("symbol=BTCUSDT&"), "{request}");
    assert!(request.contains("fromId=42"), "{request}");
    assert!(!request.contains(".p"), "{request}");
}

#[tokio::test]
async fn perpetual_depth_and_candles_share_the_selected_market() {
    let (base, request) =
        response(r#"{"lastUpdateId":100,"bids":[["99","1"]],"asks":[["101","2"]]}"#).await;
    let source = BinanceDepthHttp::with_market_base(BinanceMarket::UsdM, base);
    source.fetch_depth("BTCUSDT.p", 5000).await.unwrap();
    let request = request.await.unwrap();
    assert_eq!(
        request,
        "GET /fapi/v1/depth?symbol=BTCUSDT&limit=1000 HTTP/1.1"
    );

    let (base, request) = response("[]").await;
    let source = BinanceKlineHttp::with_market_base(BinanceMarket::UsdM, base);
    assert!(
        source
            .fetch("BTCUSDT.p", "1m", 0, 60_000, 1000)
            .await
            .unwrap()
            .is_empty()
    );
    let request = request.await.unwrap();
    assert!(
        request.starts_with("GET /fapi/v1/klines?symbol=BTCUSDT&"),
        "{request}"
    );
}

fn synchronizer() -> DepthSynchronizer {
    let mut sync = DepthSynchronizer::for_market("BTCUSDT.p", 1, BinanceMarket::UsdM);
    let snapshot =
        parse_snapshot(r#"{"lastUpdateId":100,"bids":[["99","1"]],"asks":[["101","2"]]}"#).unwrap();
    sync.install_snapshot(&snapshot, 1000).unwrap();
    sync
}

#[test]
fn futures_bridge_includes_snapshot_id_and_later_events_follow_pu_not_u_plus_one() {
    let mut sync = synchronizer();
    let mut consumer = sync.book().clone();
    let bridge = parse_update(
        r#"{"E":1000,"s":"BTCUSDT","U":95,"u":100,"pu":90,"b":[["99","999"]],"a":[]}"#,
    )
    .unwrap();
    assert!(matches!(
        sync.apply(&bridge).unwrap(),
        SyncApplyOutcome::Applied {
            bridged_snapshot: true,
            changed_levels: 0,
            ..
        }
    ));
    consumer
        .apply_delta(&bridge.to_snapshot_bridge_delta().unwrap())
        .unwrap();
    assert_eq!(
        sync.book(),
        &consumer,
        "equal snapshot id must not replay older values"
    );
    let later = parse_update(
        r#"{"E":1100,"s":"BTCUSDT","U":150,"u":175,"pu":100,"b":[["99","3"]],"a":[]}"#,
    )
    .unwrap();
    assert!(matches!(
        sync.apply(&later).unwrap(),
        SyncApplyOutcome::Applied {
            bridged_snapshot: false,
            ..
        }
    ));
    let delta = later.to_book_delta().unwrap();
    assert_eq!(delta.first_update_id(), 150, "wire id must remain factual");
    assert_eq!(delta.previous_update_id(), Some(100));
    consumer.apply_delta(&delta).unwrap();
    assert_eq!(sync.book(), &consumer);
    assert!(matches!(
        sync.apply(&later).unwrap(),
        SyncApplyOutcome::Stale { .. }
    ));
    assert_eq!(sync.book(), &consumer);
}

#[test]
fn futures_refuses_missing_link_and_wrong_chain_even_with_an_overlapping_range() {
    let mut sync = synchronizer();
    let missing = parse_update(r#"{"E":1,"s":"BTCUSDT","U":99,"u":101,"b":[],"a":[]}"#).unwrap();
    assert_eq!(
        sync.apply(&missing),
        Err(DepthApplyError::MissingPreviousUpdate)
    );
    assert!(matches!(sync.phase(), SyncPhase::AwaitingBridge { .. }));
    let bridge =
        parse_update(r#"{"E":1,"s":"BTCUSDT","U":99,"u":101,"pu":95,"b":[],"a":[]}"#).unwrap();
    sync.apply(&bridge).unwrap();
    let before: OrderBook = sync.book().clone();
    let gap =
        parse_update(r#"{"E":2,"s":"BTCUSDT","U":100,"u":102,"pu":100,"b":[["99","9"]],"a":[]}"#)
            .unwrap();
    assert_eq!(
        sync.apply(&gap),
        Err(DepthApplyError::Book(BookError::PreviousUpdateMismatch {
            expected: 101,
            got: 100
        }))
    );
    assert_eq!(sync.book(), &before, "a chain gap never mutates the book");
}

#[test]
fn futures_requires_snapshot_overlap_even_when_the_next_numeric_id_is_present() {
    let mut sync = synchronizer();
    let next =
        parse_update(r#"{"E":1,"s":"BTCUSDT","U":101,"u":102,"pu":100,"b":[],"a":[]}"#).unwrap();
    assert!(matches!(
        sync.apply(&next),
        Err(DepthApplyError::Gap { .. })
    ));
    assert!(matches!(sync.phase(), SyncPhase::AwaitingBridge { .. }));
}
