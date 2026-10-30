//! A bridge that drops and comes back after a hole: the chart must end up with
//! the bars one continuous load of the same prints builds.
//!
//! The fixture (`crates/feed/fixtures/mt5_reconnect_gap.csv`) is 600 prints
//! across one session close: 330 on the afternoon of 2026-10-29, 270 on the
//! morning of 2026-10-30, with same-millisecond groups — one of them is where
//! the first session is cut, so the seam has to be deduplicated by count, not
//! by time. Sides travel as exchange flags, so mapping needs no context and
//! the reference is the fixture itself.

use std::time::Duration;

use rust_decimal::Decimal;
use tokio::io::AsyncWriteExt as _;
use tokio::net::TcpStream;

use quantick_engine::{Bar, Side, TickBarBuilder, TimeBarBuilder, Trade, golden};

use super::spawn;
use crate::config::{MetaTraderSettings, Mt5SideSource};
use crate::{FeedEvent, FeedHandle, FeedNotice};

const FIXTURE: &str = include_str!("../../fixtures/mt5_reconnect_gap.csv");

/// The broker clock of the fixture: B3, three hours behind UTC.
const OFFSET_MS: i64 = 10_800_000;

/// One fixture row, in the bridge's server time.
#[derive(Clone, Copy)]
struct Print {
    time_ms: i64,
    price: i64,
    volume: i64,
    buy: bool,
}

fn fixture() -> Vec<Print> {
    FIXTURE
        .lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
        .map(|line| {
            let mut cols = line.split(',');
            let mut next = || cols.next().expect("four columns");
            Print {
                time_ms: next().parse().expect("time"),
                price: next().parse().expect("price"),
                volume: next().parse().expect("volume"),
                buy: next() == "B",
            }
        })
        .collect()
}

/// What one continuous load of the fixture charts.
fn reference(prints: &[Print]) -> Vec<Trade> {
    prints
        .iter()
        .enumerate()
        .map(|(i, p)| Trade {
            agg_id: i as u64,
            timestamp_ms: p.time_ms + OFFSET_MS,
            price: Decimal::from(p.price),
            quantity: Decimal::from(p.volume),
            side: if p.buy { Side::Buy } else { Side::Sell },
        })
        .collect()
}

fn tick_line(seq: u64, p: &Print) -> String {
    // LAST | VOLUME plus the aggressor bit.
    let flags = 8 | 16 | if p.buy { 32 } else { 64 };
    format!(
        "{{\"type\":\"tick\",\"seq\":{seq},\"time_ms\":{},\"bid\":\"0\",\"ask\":\"0\",\
         \"last\":\"{}\",\"volume\":{},\"flags\":{flags}}}\n",
        p.time_ms, p.price, p.volume
    )
}

/// The bridge's side of one session, numbering its ticks as a real one does.
struct FakeBridge {
    sock: TcpStream,
    seq: u64,
}

const HELLO: &str = concat!(
    "{\"type\":\"hello\",\"schema\":1,\"bridge\":\"test\",\"bridge_version\":\"0\",",
    "\"symbol\":\"WIN$N\",\"broker_symbol\":\"WINZ26\",\"digits\":0,",
    "\"server_utc_offset_s\":-10800,\"history_paging\":true}\n",
);

impl FakeBridge {
    async fn dial(port: u16) -> Self {
        for _ in 0..100 {
            if let Ok(sock) = TcpStream::connect(("127.0.0.1", port)).await {
                let mut bridge = Self { sock, seq: 0 };
                bridge.write(HELLO.to_owned()).await;
                return bridge;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("could not reach the feed listener on {port}");
    }

    fn ticks(&mut self, prints: &[Print]) -> String {
        prints
            .iter()
            .map(|p| {
                self.seq += 1;
                tick_line(self.seq, p)
            })
            .collect()
    }

    async fn write(&mut self, text: String) {
        self.sock.write_all(text.as_bytes()).await.expect("write");
        self.sock.flush().await.expect("flush");
    }

    async fn backfill(&mut self, prints: &[Print]) {
        let body = self.ticks(prints);
        self.write(format!(
            "{{\"type\":\"backfill_start\",\"count_hint\":{}}}\n{body}{{\"type\":\"backfill_end\"}}\n",
            prints.len()
        ))
        .await;
    }

    async fn opening(&mut self, prints: &[Print], remaining: u64) {
        let body = self.ticks(prints);
        self.write(format!(
            "{{\"type\":\"history_start\",\"count_hint\":{},\"opening\":true}}\n{body}\
             {{\"type\":\"history_end\",\"opening\":true,\"remaining\":{remaining}}}\n",
            prints.len()
        ))
        .await;
    }

    async fn page(&mut self, prints: &[Print], scanned_to_ms: i64) {
        let body = self.ticks(prints);
        self.write(format!(
            "{{\"type\":\"history_start\"}}\n{body}\
             {{\"type\":\"history_end\",\"exhausted\":false,\"scanned_to_ms\":{scanned_to_ms}}}\n"
        ))
        .await;
    }

    async fn live(&mut self, prints: &[Print]) {
        let body = self.ticks(prints);
        self.write(body).await;
    }

    /// The next line the feed wrote back, or `None` if it never asked.
    async fn request(&mut self) -> Option<String> {
        use tokio::io::AsyncReadExt as _;
        let mut line = Vec::new();
        let mut byte = [0_u8; 1];
        loop {
            let read = tokio::time::timeout(Duration::from_secs(5), self.sock.read(&mut byte))
                .await
                .ok()?
                .ok()?;
            if read == 0 {
                return None;
            }
            if byte[0] == b'\n' {
                return Some(String::from_utf8(line).expect("the feed writes UTF-8"));
            }
            line.push(byte[0]);
        }
    }
}

fn feed_on(port: u16) -> FeedHandle {
    let settings = MetaTraderSettings {
        listen_addr: format!("127.0.0.1:{port}"),
        // Exchange flags carry the side, so no print depends on its neighbour.
        side_source: Mt5SideSource::Flags,
        bridge_autostart: false,
        ..MetaTraderSettings::default()
    };
    spawn("WIN$N", &settings, None)
}

/// The tape a chart holds after these events, and the gaps it was told of.
#[derive(Default)]
struct Chart {
    tape: Vec<Trade>,
    gaps: Vec<crate::FeedGap>,
}

impl Chart {
    fn take(&mut self, event: FeedEvent) {
        match event {
            FeedEvent::HistoryPrepended(older)
            | FeedEvent::OpeningPrepended { trades: older, .. } => {
                self.tape.splice(0..0, older);
            }
            FeedEvent::Live(trade) => self.tape.push(trade),
            FeedEvent::LiveBatch(trades) => self.tape.extend(trades),
            FeedEvent::Continuity(event) => self.gaps.extend(event.gap),
            _ => {}
        }
    }

    /// Drain events until the tape holds `count` prints or nothing more comes.
    async fn fill_to(&mut self, feed: &mut FeedHandle, count: usize) {
        while self.tape.len() < count {
            match tokio::time::timeout(Duration::from_secs(5), feed.events.recv()).await {
                Ok(Some(event)) => self.take(event),
                _ => return,
            }
        }
    }
}

async fn wait_for_notice(feed: &mut FeedHandle, want: impl Fn(&FeedNotice) -> bool) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let notice = feed.notices.recv().await.expect("notice channel open");
            if want(&notice) {
                return;
            }
        }
    })
    .await
    .expect("timed out waiting for a feed notice");
}

fn bars(trades: &[Trade]) -> (Vec<Bar>, Vec<Bar>) {
    (
        golden::replay(&mut TickBarBuilder::new(7), trades),
        golden::replay(&mut TimeBarBuilder::new(60_000), trades),
    )
}

fn same_prints(a: &[Trade], b: &[Trade]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(x, y)| {
            (x.timestamp_ms, x.price, x.quantity, x.side)
                == (y.timestamp_ms, y.price, y.quantity, y.side)
        })
}

#[tokio::test]
async fn a_reconnect_after_a_hole_charts_what_one_continuous_load_charts() {
    let prints = fixture();
    assert_eq!(prints.len(), 600, "the frozen fixture");
    let expected = reference(&prints);

    let mut feed = feed_on(19_261);
    let Some(FeedEvent::Backfilled(empty)) = feed.events.recv().await else {
        panic!("expected the immediate empty backfill");
    };
    assert!(empty.is_empty());
    let mut chart = Chart::default();

    // Session 1: the afternoon's first hundred prints as the opening block,
    // then live prints until the terminal goes away in the middle of the
    // three prints sharing one millisecond at 198..=200.
    let mut bridge = FakeBridge::dial(19_261).await;
    bridge.backfill(&prints[..100]).await;
    bridge.live(&prints[100..200]).await;
    chart.fill_to(&mut feed, 200).await;
    assert_eq!(chart.tape.len(), 200, "session 1 charts what it streamed");
    drop(bridge);
    wait_for_notice(&mut feed, |n| matches!(n, FeedNotice::Reconnecting { .. })).await;

    // Session 2, the next morning: the newest slice of today's session, the
    // rest of today newest-first, and — once asked — the tail of yesterday
    // the chart never saw. The bridge serves a page only below its opening
    // floor, so the page reaches up to today's first print and no further.
    let mut bridge = FakeBridge::dial(19_261).await;
    bridge.backfill(&prints[500..560]).await;
    bridge.opening(&prints[400..500], 1).await;
    bridge.opening(&prints[330..400], 0).await;
    let request = bridge.request().await;
    assert!(
        request
            .as_deref()
            .is_some_and(|r| r.contains("\"load_older\"")),
        "the feed never asked the bridge for the hole the reconnect left: {request:?}"
    );
    bridge.page(&prints[150..330], prints[150].time_ms).await;
    bridge.live(&prints[560..]).await;
    chart.fill_to(&mut feed, 600).await;

    assert!(
        chart.gaps.is_empty(),
        "a hole the terminal could fill is not a gap: {:?}",
        chart.gaps
    );
    assert!(
        same_prints(&chart.tape, &expected),
        "the reconnected tape differs from one continuous load ({} prints vs {})",
        chart.tape.len(),
        expected.len()
    );
    let (tick_bars, time_bars) = bars(&chart.tape);
    let (want_tick, want_time) = bars(&expected);
    assert_eq!(golden::diff_bars(&want_tick, &tick_bars), None);
    assert_eq!(golden::diff_bars(&want_time, &time_bars), None);
    // Frozen so a change to the fixture or the builders is seen here.
    assert_eq!(want_tick.len(), 85);
    assert_eq!(want_time.len(), 112);
}
