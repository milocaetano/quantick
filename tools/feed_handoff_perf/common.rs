use std::time::{Duration, Instant};
use futures_util::{SinkExt as _, StreamExt as _};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use crate::{FeedCommand, FeedEvent, FeedNotice};

const CAPACITY: usize = 4096;
const TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Default, serde::Serialize)]
struct Totals {
    trades: u64,
    checksum: u64,
    anomalies: u64,
    missing: u64,
    unknown: u64,
    non_monotonic: u64,
    batch_messages: u64,
    invalid_gaps: u64,
    diagnostic_signature: u64,
    excluded_events: u64,
    malformed_rows: u64,
    stale_rows: u64,
}

pub(crate) enum Observation {
    Feed(FeedEvent),
    Excluded { malformed: u64, stale: u64 },
}

impl Totals {
    fn observe(&mut self, event: crate::F2BenchEvent) {
        let event = match crate::f2_bench_observation(event) {
            Observation::Feed(event) => event,
            Observation::Excluded { malformed, stale } => {
                self.excluded_events += 1;
                self.malformed_rows += malformed;
                self.stale_rows += stale;
                let signature = if malformed > 0 {1} else {2};
                self.diagnostic_signature = self.diagnostic_signature.wrapping_mul(4).wrapping_add(signature);
                return;
            }
        };
        match event {
            FeedEvent::Live(trade) => self.trade(&trade),
            FeedEvent::LiveBatch(trades) | FeedEvent::Backfilled(trades) => {
                self.batch_messages += 1;
                for trade in trades { self.trade(&trade); }
            }
            FeedEvent::Continuity(event) => {
                self.anomalies += 1;
                self.non_monotonic += u64::from(event.non_monotonic);
                if let Some(count) = event.missing_messages { self.missing += count; }
                else { self.unknown += 1; }
                self.invalid_gaps += u64::from(event.gap.is_some());
                let signature = if event.missing_messages == Some(1) {
                    1 + u64::from(event.non_monotonic)
                } else { 3 };
                self.diagnostic_signature = self.diagnostic_signature.wrapping_mul(4).wrapping_add(signature);
            }
            _ => panic!("unexpected fixture event"),
        }
    }
    fn trade(&mut self, trade: &quantick_engine::Trade) {
        self.trades += 1;
        for value in [trade.agg_id, trade.timestamp_ms as u64, trade.price.mantissa() as u64,
            trade.price.scale() as u64, trade.quantity.mantissa() as u64,
            trade.quantity.scale() as u64, u64::from(trade.side == quantick_engine::Side::Buy)] {
            self.checksum = (self.checksum ^ value).wrapping_mul(1099511628211);
        }
    }
}

async fn drain(rx: &mut crate::F2BenchReceiver, totals: &mut Totals, target: u64) {
    while totals.trades < target {
        totals.observe(rx.recv().await.expect("host ended before qualifying trade"));
    }
}

fn channels<T>() -> (mpsc::Sender<T>, mpsc::Receiver<T>, mpsc::Sender<FeedNotice>, mpsc::Receiver<FeedNotice>, mpsc::Sender<FeedCommand>, mpsc::Receiver<FeedCommand>) {
    let (tx, rx) = mpsc::channel(CAPACITY);
    let (notice_tx, notice_rx) = mpsc::channel(32);
    let (cmd_tx, cmd_rx) = mpsc::channel(16);
    (tx, rx, notice_tx, notice_rx, cmd_tx, cmd_rx)
}

#[tokio::test]
#[ignore = "external fixed paired protocol, requires coordinator quiet-host lease"]
async fn f2_protocol_sample() {
    let case = std::env::var("F2_BENCH_CASE").unwrap();
    let fixture: serde_json::Value = serde_json::from_str(include_str!("f2-fixture.json")).unwrap();
    let (elapsed, totals) = tokio::time::timeout(TIMEOUT, async {
        if case == "binance" { binance(&fixture).await } else { hyperliquid(&fixture, &case).await }
    }).await.expect("bounded sample timed out");
    println!("F2_SAMPLE {}", serde_json::json!({"case":case,"elapsed_ns":elapsed,"totals":totals,"event_capacity":CAPACITY}));
}

async fn binance(fixture: &serde_json::Value) -> (u128, Totals) {
    let spec = &fixture["binance"];
    let count = spec["live_rows_per_sample"].as_u64().unwrap();
    let seed = &spec["backfill_seed"];
    let rest_body = serde_json::to_string(&serde_json::json!([seed])).unwrap();
    let mut payloads = Vec::with_capacity(count as usize);
    for index in 0..count {
        let mut frame = spec["live_template"].clone();
        frame["a"] = (frame["a"].as_u64().unwrap() + index).into();
        frame["T"] = (frame["T"].as_u64().unwrap() + index).into();
        payloads.push(serde_json::to_string(&frame).unwrap());
    }
    let rest = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let rest_url = format!("http://{}", rest.local_addr().unwrap());
    let rest_task = tokio::spawn(async move {
        loop {
            let (mut socket, _) = rest.accept().await.unwrap();
            let mut request = [0;4096]; let mut used = 0;
            while !request[..used].ends_with(b"\r\n\r\n") {
                let n = socket.read(&mut request[used..]).await.unwrap();
                assert!(n > 0); used += n;
            }
            let response = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{rest_body}", rest_body.len());
            socket.write_all(response.as_bytes()).await.unwrap();
        }
    });
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}",listener.local_addr().unwrap());
    let (tx,rx,notice_tx,_notice_rx,cmd_tx,cmd_rx) = channels();
    let host = tokio::spawn(crate::binance::f2_bench_host(rest_url,url,tx,notice_tx,cmd_rx));
    let mut rx = crate::f2_bench_receiver(rx);
    let (socket,_) = listener.accept().await.unwrap();
    socket.set_nodelay(true).unwrap();
    let mut socket = tokio_tungstenite::accept_async(socket).await.unwrap();
    let mut prime = Totals::default(); drain(&mut rx,&mut prime,1).await;
    assert_eq!(prime.trades,1);
    assert_eq!((prime.excluded_events,prime.malformed_rows,prime.stale_rows),(0,0,0));
    let (start_tx, start_rx) = tokio::sync::oneshot::channel();
    let sender = tokio::spawn(async move {
        start_rx.await.unwrap();
        for payload in payloads { socket.send(Message::Text(payload)).await.unwrap(); }
        socket
    });
    let start = Instant::now();
    start_tx.send(()).unwrap();
    let mut totals = Totals::default(); drain(&mut rx,&mut totals,count).await;
    let elapsed = start.elapsed().as_nanos();
    let _socket = sender.await.unwrap();
    drop(cmd_tx); host.await.unwrap();
    assert!(matches!(rx.try_recv(),Err(mpsc::error::TryRecvError::Disconnected)));
    rest_task.abort(); assert!(rest_task.await.unwrap_err().is_cancelled());
    assert_eq!(totals.trades,count); assert_eq!(totals.anomalies,0); assert_eq!(totals.invalid_gaps,0);
    assert_eq!((totals.excluded_events,totals.malformed_rows,totals.stale_rows),(0,0,0));
    (elapsed,totals)
}

fn shifted(rows:&serde_json::Value,index:u64)->String {
    let mut rows=rows.clone();
    for row in rows.as_array_mut().unwrap() {
        row["time"]=(row["time"].as_u64().unwrap()+10*index).into();
        row["tid"]=(row["tid"].as_u64().unwrap()+100*index).into();
    }
    serde_json::to_string(&serde_json::json!({"channel":"trades","data":rows})).unwrap()
}

async fn hyperliquid(fixture:&serde_json::Value,case:&str)->(u128,Totals) {
    let exclusion=case=="exclusion"; assert!(exclusion||case=="dense");
    let spec=&fixture["hyperliquid"]; let count=spec["batches_per_sample"].as_u64().unwrap();
    let template=if exclusion {"exclusion_bearing_template"} else {"dense_valid_template"};
    let payloads:Vec<_>=(0..count).map(|i|shifted(&spec[template],i)).collect();
    let primes:Vec<_>=if exclusion {(0..count).map(|i|shifted(&spec["exclusion_prime"],i)).collect()}else{Vec::new()};
    let listener=TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url=format!("ws://{}",listener.local_addr().unwrap());
    let (tx,rx,notice_tx,mut notice_rx,cmd_tx,cmd_rx)=channels();
    let host=tokio::spawn(crate::hyperliquid::f2_bench_host(url,tx,notice_tx,cmd_rx));
    let mut rx = crate::f2_bench_receiver(rx);
    let (socket,_)=listener.accept().await.unwrap(); socket.set_nodelay(true).unwrap();
    let mut socket=tokio_tungstenite::accept_async(socket).await.unwrap();
    let subscription=socket.next().await.unwrap().unwrap();
    assert!(matches!(subscription,Message::Text(text) if text.contains("trades")));
    socket.send(Message::Text(r#"{"channel":"subscriptionResponse","data":{}}"#.into())).await.unwrap();
    notice_rx.recv().await.unwrap();
    let mut totals=Totals::default(); let mut elapsed=0;
    let mut prime_totals=Totals::default();
    for (index,payload) in payloads.into_iter().enumerate() {
        if exclusion {
            // Payload cloning, prime parsing/mapping and delivery are all
            // outside the timed segment, identically on both sides.
            socket.send(Message::Text(primes[index].clone())).await.unwrap();
            drain(&mut rx,&mut prime_totals,index as u64+1).await;
        }
        let target=totals.trades+if exclusion {1}else{4};
        let prior_batches=totals.batch_messages;
        let prior_exclusions=totals.excluded_events;
        totals.diagnostic_signature=0;
        let start=Instant::now();
        socket.send(Message::Text(payload)).await.unwrap();
        drain(&mut rx,&mut totals,target).await;
        elapsed+=start.elapsed().as_nanos();
        // Constant-sized per-batch signatures; all correctness assertions
        // occur after this segment's stopwatch, with no per-trade allocation.
        assert_eq!(totals.batch_messages-prior_batches,1);
        let diagnostics=exclusion&&crate::f2_bench_candidate();
        assert_eq!(totals.excluded_events-prior_exclusions,if diagnostics {2}else{0});
        assert_eq!(totals.diagnostic_signature,if diagnostics {6}else{0});
    }
    drop(cmd_tx); host.await.unwrap();
    assert!(matches!(rx.try_recv(),Err(mpsc::error::TryRecvError::Disconnected)));
    assert_eq!(prime_totals.trades,if exclusion {count}else{0});
    assert_eq!(prime_totals.anomalies,0);
    assert_eq!((prime_totals.excluded_events,prime_totals.malformed_rows,prime_totals.stale_rows),(0,0,0));
    assert_eq!(totals.trades,count*if exclusion {1}else{4});
    let expected=if exclusion&&crate::f2_bench_candidate(){count}else{0};
    assert_eq!(totals.excluded_events,expected*2);
    assert_eq!(totals.malformed_rows,expected); assert_eq!(totals.stale_rows,expected);
    assert_eq!(totals.anomalies,0); assert_eq!(totals.missing,0);
    assert_eq!(totals.non_monotonic,0); assert_eq!(totals.unknown,0);
    assert_eq!(totals.invalid_gaps,0);
    (elapsed,totals)
}
