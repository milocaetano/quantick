//! *History* against the production Binance host, over local sockets: what
//! the 48 h perpetual retention boundary and a failed fetch tell the chart.
//!
//! Neither is the feed going offline, so neither may raise
//! [`FeedNotice::Attention`]: the app draws every Attention as an offline
//! card with a Reconnect button and stands stall detection down while it is
//! up. The boundary withdraws paging instead, so the run ends `Exhausted`; a
//! failed fetch is logged and answered with an empty page.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use quantick_engine::Trade;
use quantick_feed_binance::{Backoff, BinanceHttp, BinanceMarket, depth::DepthEvent};
use quantick_sources::history_reach::{
    Campaign, CampaignEnd, CampaignStart, CampaignStep, HistoryReach, ReachBounds, TapeFacts,
};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpListener, TcpStream},
    sync::{mpsc, oneshot, watch},
    task::JoinHandle,
    time::timeout,
};
use tracing_subscriber::fmt::MakeWriter;

use crate::config::ProviderKind;
use crate::stall::{SILENT_BUDGET_MS, StallInput, assess};
use crate::{FeedCapabilities, FeedCommand, FeedConnectionState, FeedEvent, FeedNotice};

const WAIT: Duration = Duration::from_secs(10);

/// What the USD-M venue answers for an aggregate id older than it retains.
const HISTORY_EXPIRED: &str = r#"{"code":-4166,"msg":"Historical data is not available"}"#;
const RATE_LIMITED: &str = r#"{"code":-1003,"msg":"Too many requests"}"#;

fn response(status: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

/// The newest page: aggregate ids 1000..2000, one print each.
fn opening_page() -> String {
    let rows: Vec<_> = (1000..2000)
        .map(|id| {
            format!(r#"{{"a":{id},"p":"81200.10","q":"0.001","f":1,"l":1,"T":{id}000,"m":false}}"#)
        })
        .collect();
    response("200 OK", &format!("[{}]", rows.join(",")))
}

async fn read_request(socket: &mut TcpStream) -> String {
    let mut request = Vec::new();
    let mut buffer = [0; 4096];
    while !request.windows(4).any(|part| part == b"\r\n\r\n") {
        let count = socket.read(&mut buffer).await.unwrap_or(0);
        if count == 0 {
            break;
        }
        request.extend_from_slice(&buffer[..count]);
    }
    String::from_utf8_lossy(&request).into_owned()
}

/// A REST venue answering every aggTrades request by whether it pages back:
/// the opening page without a `fromId`, `older` with one.
async fn rest_venue(older: String) -> (String, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let request = read_request(&mut socket).await;
            let answer = if request.contains("fromId=") {
                older.clone()
            } else {
                opening_page()
            };
            let _ = socket.write_all(answer.as_bytes()).await;
        }
    });
    (url, server)
}

/// A trade socket that never prints: its first connection closes when
/// `drop_first` fires, every later one stays open.
async fn quiet_socket(drop_first: oneshot::Receiver<()>) -> (String, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let mut first = tokio_tungstenite::accept_async(socket).await.unwrap();
        let _ = drop_first.await;
        let _ = first.close(None).await;
        drop(first);
        let mut held = Vec::new();
        while let Ok((socket, _)) = listener.accept().await {
            if let Ok(socket) = tokio_tungstenite::accept_async(socket).await {
                held.push(socket);
            }
        }
    });
    (url, server)
}

/// The production host, its channels as the tab holds them.
struct Host {
    events: mpsc::Receiver<FeedEvent>,
    notices: mpsc::Receiver<FeedNotice>,
    commands: mpsc::Sender<FeedCommand>,
    capabilities: watch::Receiver<FeedCapabilities>,
    provider: ProviderKind,
    /// Every notice read so far, in order.
    heard: Vec<FeedNotice>,
    _book: mpsc::Receiver<DepthEvent>,
    task: JoinHandle<()>,
}

impl Host {
    fn start(market: BinanceMarket, rest_url: String, url: String) -> Self {
        let (tx, events) = mpsc::channel(64);
        let (book_tx, book) = mpsc::channel(8);
        let (notice_tx, notices) = mpsc::channel(32);
        let (commands, cmd_rx) = mpsc::channel(8);
        let provider = match market {
            BinanceMarket::Spot => ProviderKind::Binance,
            BinanceMarket::UsdM => ProviderKind::BinanceFutures,
        };
        let (caps_tx, capabilities) = watch::channel(provider.capabilities());
        let task = tokio::spawn(super::binance::feed_task(
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
        Self {
            events,
            notices,
            commands,
            capabilities,
            provider,
            heard: Vec::new(),
            _book: book,
            task,
        }
    }

    async fn backfill(&mut self) -> Vec<Trade> {
        match timeout(WAIT, self.events.recv()).await.unwrap() {
            Some(FeedEvent::Backfilled(trades)) => trades,
            _ => panic!("the opening backfill comes first"),
        }
    }

    /// Read notices until `count` more `Connected` have arrived.
    async fn connected(&mut self, count: usize) {
        let mut seen = 0;
        while seen < count {
            let notice = timeout(WAIT, self.notices.recv()).await.unwrap().unwrap();
            seen += usize::from(notice == FeedNotice::Connected);
            self.heard.push(notice);
        }
    }

    /// Press *load older* once and return the page it answered with.
    async fn load_older(&mut self) -> Vec<Trade> {
        self.commands
            .send(FeedCommand::LoadOlder { count: 1_000 })
            .await
            .unwrap();
        loop {
            match timeout(WAIT, self.events.recv()).await.unwrap() {
                Some(FeedEvent::HistoryPrepended(page)) => {
                    // Whatever the host said before answering is in the
                    // channel by now.
                    while let Ok(notice) = self.notices.try_recv() {
                        self.heard.push(notice);
                    }
                    return page;
                }
                Some(FeedEvent::Continuity(_)) => {}
                _ => panic!("a load older is answered by a page"),
            }
        }
    }

    fn can_page(&self) -> bool {
        self.capabilities.borrow().history_paging
    }

    /// Neither the boundary nor a failed fetch is the feed going offline.
    fn assert_no_attention(&self) {
        assert!(
            self.heard
                .iter()
                .all(|notice| !matches!(notice, FeedNotice::Attention { .. })),
            "nothing here is the feed going offline: {:?}",
            self.heard
        );
    }

    /// The chart's own stall judgement still runs over the notice that
    /// stands: a connected tape silent past its budget is reported.
    fn assert_stall_detection_armed(&self) {
        let standing = self.heard.last().expect("the host reported its link");
        let input = StallInput {
            notice: standing,
            connection_since_ms: 0,
            connection: FeedConnectionState::Connected,
            tape_age_ms: Some(SILENT_BUDGET_MS),
            attached_ms: 0,
            provider: self.provider,
            provider_name: "Binance",
            replaying: false,
        };
        assert!(
            assess(&input, SILENT_BUDGET_MS).is_some(),
            "stall detection stays armed under {standing:?}"
        );
    }

    async fn stop(self) {
        drop(self.commands);
        timeout(WAIT, self.task).await.unwrap().unwrap();
    }
}

/// A run toward a day behind the opening page, as the History press starts it.
fn history_run(held: &[Trade]) -> Campaign {
    match Campaign::start(
        held,
        &TapeFacts::default(),
        HistoryReach::Hours(24),
        ReachBounds::default(),
        1_000,
    ) {
        CampaignStart::Run(campaign) => campaign,
        other => panic!("a day behind the opening page is not held yet: {other:?}"),
    }
}

#[tokio::test]
async fn the_retention_boundary_withdraws_paging_and_ends_history_exhausted() {
    let (rest_url, rest) = rest_venue(response("400 Bad Request", HISTORY_EXPIRED)).await;
    let (drop_first, first_dropped) = oneshot::channel();
    let (url, socket) = quiet_socket(first_dropped).await;
    let mut host = Host::start(BinanceMarket::UsdM, rest_url, url);
    let held = host.backfill().await;
    host.connected(1).await;
    assert!(
        host.can_page(),
        "the opening page did not reach the boundary"
    );

    let page = host.load_older().await;
    assert!(page.is_empty(), "nothing older is retained");
    assert!(
        !host.can_page(),
        "the boundary withdraws paging before the page is judged"
    );
    let mut run = history_run(&held);
    assert_eq!(
        run.advance(&page, host.can_page()),
        CampaignStep::Stop(CampaignEnd::Exhausted),
        "the run ends on the venue having nothing older"
    );

    // A reconnect re-announces the link and nothing else.
    drop_first.send(()).unwrap();
    host.connected(1).await;
    host.assert_no_attention();
    host.assert_stall_detection_armed();
    assert!(!host.can_page(), "a reconnect does not re-offer paging");
    host.stop().await;
    socket.abort();
    rest.abort();
}

/// A `MakeWriter` appending every log line to a shared buffer.
#[derive(Clone)]
struct SharedLog(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for SharedLog {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for SharedLog {
    type Writer = SharedLog;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

#[tokio::test]
async fn a_failed_history_fetch_is_logged_and_answered_with_an_empty_page() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let subscriber = tracing_subscriber::fmt()
        .with_writer(SharedLog(log.clone()))
        .with_max_level(tracing::Level::WARN)
        .with_ansi(false)
        .finish();
    // The host runs on this test's single-threaded runtime, so the scoped
    // subscriber sees its lines.
    let _logging = tracing::subscriber::set_default(subscriber);
    let (rest_url, rest) = rest_venue(response("429 Too Many Requests", RATE_LIMITED)).await;
    let (_drop_first, first_dropped) = oneshot::channel();
    let (url, socket) = quiet_socket(first_dropped).await;
    let mut host = Host::start(BinanceMarket::Spot, rest_url, url);
    let held = host.backfill().await;
    host.connected(1).await;

    let page = host.load_older().await;
    assert!(page.is_empty(), "a refused fetch brings nothing");
    assert!(
        host.can_page(),
        "a refused fetch is no boundary: press again"
    );
    let mut run = history_run(&held);
    assert_eq!(
        run.advance(&page, host.can_page()),
        CampaignStep::Ask,
        "the run is judged on an empty page and may ask again"
    );
    host.assert_no_attention();
    host.assert_stall_detection_armed();
    let log = String::from_utf8(log.lock().unwrap().clone()).unwrap();
    assert!(
        log.contains("load older failed"),
        "the failure is logged: {log}"
    );
    assert!(log.contains("429"), "with the venue's status: {log}");
    host.stop().await;
    socket.abort();
    rest.abort();
}
