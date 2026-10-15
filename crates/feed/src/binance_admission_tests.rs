//! Exercise the production backfill/socket host boundary, not a parallel filter.

use std::time::Duration;

use futures_util::SinkExt as _;
use quantick_engine::Side;
use quantick_feed_binance::{Backoff, BinanceHttp, BinanceMarket};
use rust_decimal::Decimal;
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::TcpListener,
    sync::mpsc,
};
use tokio_tungstenite::tungstenite::Message;

use crate::{FeedEvent, FeedIntegrity};

#[tokio::test]
async fn backfill_seeded_spot_and_perpetual_reconnects_keep_unique_quantity_and_sides() {
    for market in [BinanceMarket::Spot, BinanceMarket::UsdM] {
        let rest = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let rest_url = format!("http://{}", rest.local_addr().unwrap());
        let rest_server = tokio::spawn(async move {
            let (mut socket, _) = rest.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 4096];
            while !request.windows(4).any(|part| part == b"\r\n\r\n") {
                let count = socket.read(&mut buffer).await.unwrap();
                assert!(count > 0);
                request.extend_from_slice(&buffer[..count]);
            }
            let request = String::from_utf8(request).unwrap();
            assert!(request.starts_with(&format!("GET {}/aggTrades?", market.rest_prefix())));
            let rows: Vec<_> = (1000..2000)
                .map(|id| {
                    format!(
                        r#"{{"a":{id},"p":"81200.10","q":"0.001","f":1,"l":1,"T":100,"m":false}}"#
                    )
                })
                .collect();
            let body = format!("[{}]", rows.join(","));
            socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
        });
        let websocket = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}", websocket.local_addr().unwrap());
        let socket_server = tokio::spawn(async move {
            // Deliberately repeat a seeded ID across connections, then deliver
            // one previously unseen older ID. Continuity remains diagnostic.
            for prints in [
                vec![(1999, "0.001", false)],
                vec![
                    (2001, "0.123", true),
                    (2000, "0.007", false),
                    (2001, "0.123", true),
                ],
            ] {
                let (socket, _) = websocket.accept().await.unwrap();
                let mut socket = tokio_tungstenite::accept_async(socket).await.unwrap();
                for (id, quantity, maker) in prints {
                    let frame = format!(
                        r#"{{"e":"aggTrade","s":"BTCUSDT","a":{id},"p":"81200.10","q":"{quantity}","f":1,"l":1,"T":100,"m":{maker}}}"#
                    );
                    socket.send(Message::Text(frame)).await.unwrap();
                }
                socket.close(None).await.unwrap();
            }
        });
        let (tx, mut rx) = mpsc::channel(16);
        let (book_tx, _book_rx) = mpsc::channel(8);
        let (notice_tx, _notice_rx) = mpsc::channel(32);
        let (cmd_tx, cmd_rx) = mpsc::channel(8);
        let (caps_tx, _caps) =
            tokio::sync::watch::channel(crate::config::ProviderKind::Binance.capabilities());
        let host = tokio::spawn(super::binance::feed_task(
            market.display_symbol("BTCUSDT"),
            tx,
            book_tx,
            notice_tx,
            caps_tx,
            cmd_rx,
            super::binance::BinanceSource {
                http: BinanceHttp::with_market_base(market, rest_url),
                url,
                backoff: Backoff::new(Duration::from_millis(1), Duration::from_millis(1), 7),
            },
        ));
        let (prints, integrity) = tokio::time::timeout(Duration::from_secs(5), async {
            let mut prints = Vec::new();
            let mut integrity = FeedIntegrity::default();
            for _ in 0..7 {
                match rx.recv().await.unwrap() {
                    FeedEvent::Backfilled(batch) => prints.extend(batch),
                    FeedEvent::Live(trade) => prints.push(trade),
                    FeedEvent::Continuity(event) => integrity.observe(event),
                    _ => panic!("unexpected host event"),
                }
            }
            (prints, integrity)
        })
        .await
        .unwrap();
        assert_eq!(prints.len(), 1002);
        assert_eq!(
            prints
                .iter()
                .filter(|trade| trade.side == Side::Buy)
                .map(|trade| trade.quantity)
                .sum::<Decimal>(),
            Decimal::new(1007, 3)
        );
        assert_eq!(
            prints
                .iter()
                .filter(|trade| trade.side == Side::Sell)
                .map(|trade| trade.quantity)
                .sum::<Decimal>(),
            Decimal::new(123, 3)
        );
        assert_eq!(prints[1000].agg_id, 2001);
        assert_eq!(
            prints[1001].agg_id, 2000,
            "unseen reorder is factual, not a duplicate"
        );
        assert_eq!(integrity.non_monotonic, 3);
        assert_eq!(
            integrity.missing_messages, 1,
            "later recovery does not erase a diagnostic"
        );
        assert!(rx.try_recv().is_err());
        drop(cmd_tx);
        tokio::time::timeout(Duration::from_secs(5), host)
            .await
            .unwrap()
            .unwrap();
        socket_server.await.unwrap();
        rest_server.await.unwrap();
    }
}
