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

use super::gap_fill::RefillLimits;
use super::{spawn, spawn_holding};
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
        self.request_within(Duration::from_secs(5)).await
    }

    /// [`Self::request`], waiting at most `wait` for each byte.
    async fn request_within(&mut self, wait: Duration) -> Option<String> {
        use tokio::io::AsyncReadExt as _;
        let mut line = Vec::new();
        let mut byte = [0_u8; 1];
        loop {
            let read = tokio::time::timeout(wait, self.sock.read(&mut byte))
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
    spawn("WIN$N", &settings_on(port), None)
}

/// A feed whose refill lets the live tape go after `stall` without
/// progress, or `hard` in all.
fn feed_holding(port: u16, stall: Duration, hard: Duration) -> FeedHandle {
    spawn_holding(
        "WIN$N",
        &settings_on(port),
        None,
        RefillLimits { stall, hard },
    )
}

fn settings_on(port: u16) -> MetaTraderSettings {
    MetaTraderSettings {
        listen_addr: format!("127.0.0.1:{port}"),
        // Exchange flags carry the side, so no print depends on its neighbour.
        side_source: Mt5SideSource::Flags,
        bridge_autostart: false,
        ..MetaTraderSettings::default()
    }
}

/// The tape a chart holds after these events, the gaps it was told of, and
/// how the refill reported itself.
#[derive(Default)]
struct Chart {
    tape: Vec<Trade>,
    gaps: Vec<crate::FeedGap>,
    reports: Vec<crate::config::TapeBackfill>,
    /// Prints that arrived on the history path rather than as live.
    refilled: usize,
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
            FeedEvent::Refilled(trades) => {
                self.refilled += trades.len();
                self.tape.extend(trades);
            }
            FeedEvent::Continuity(event) => self.gaps.extend(event.gap),
            FeedEvent::RefillStatus(report) => self.reports.push(report),
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
    let mut chart = Chart::default();
    first_session(&mut feed, 19_261, &mut chart, &prints).await;

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
    // The hole is history: 200..560 never reaches the chart as live.
    assert_eq!(chart.refilled, 360, "the hole arrives on the history path");
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

/// Session 1 of the fixture: the opening block, then live prints until the
/// terminal goes away inside the same-millisecond group at 198..=200.
async fn first_session(feed: &mut FeedHandle, port: u16, chart: &mut Chart, prints: &[Print]) {
    let Some(FeedEvent::Backfilled(empty)) = feed.events.recv().await else {
        panic!("expected the immediate empty backfill");
    };
    assert!(empty.is_empty());
    let mut bridge = FakeBridge::dial(port).await;
    bridge.backfill(&prints[..100]).await;
    bridge.live(&prints[100..200]).await;
    chart.fill_to(feed, 200).await;
    assert_eq!(chart.tape.len(), 200, "session 1 charts what it streamed");
    drop(bridge);
    wait_for_notice(feed, |n| matches!(n, FeedNotice::Reconnecting { .. })).await;
}

#[tokio::test]
async fn a_session_lost_mid_refill_marks_the_part_it_did_not_reach() {
    let prints = fixture();
    let expected = reference(&prints);
    let mut feed = feed_on(19_262);
    let mut chart = Chart::default();
    let mut reports = Vec::new();
    first_session(&mut feed, 19_262, &mut chart, &prints).await;

    // The next morning's session sends one opening slice, is asked for the
    // rest, streams a few live prints — and dies before answering.
    let mut bridge = FakeBridge::dial(19_262).await;
    bridge.backfill(&prints[500..560]).await;
    bridge.opening(&prints[400..500], 1).await;
    assert!(bridge.request().await.is_some(), "the hole was asked for");
    bridge.live(&prints[560..580]).await;
    // Give the feed time to hold the live prints before the socket closes.
    tokio::time::sleep(Duration::from_millis(200)).await;
    drop(bridge);

    while chart.tape.len() < 380 {
        match tokio::time::timeout(Duration::from_secs(5), feed.events.recv()).await {
            Ok(Some(FeedEvent::RefillStatus(report))) => reports.push(report),
            Ok(Some(event)) => chart.take(event),
            _ => break,
        }
    }
    let to_ms = prints[400].time_ms + OFFSET_MS;
    assert_eq!(
        chart.gaps,
        [crate::FeedGap {
            from_ms: prints[199].time_ms + OFFSET_MS,
            to_ms
        }],
        "the unreached part of the hole is marked, from the chart's last print"
    );
    let mut want = expected[..200].to_vec();
    want.extend_from_slice(&expected[400..580]);
    assert!(
        same_prints(&chart.tape, &want),
        "what arrived is charted in order ({} prints)",
        chart.tape.len()
    );
    let last = reports.last().expect("the refill reported");
    assert_eq!(
        last.state,
        crate::config::BackfillState::Partial {
            unfilled_to_ms: to_ms,
            stop: crate::config::BackfillStop::SessionLost
        }
    );
    assert_eq!(last.received, 160);
}

#[tokio::test]
async fn a_respawned_feed_told_where_the_chart_ends_refills_the_same_hole() {
    // The tab's *reconnect* spawns a fresh feed: it has forwarded nothing,
    // so the chart has to tell it where its tape ends.
    let prints = fixture();
    let expected = reference(&prints);
    let mut feed = feed_on(19_263);
    let Some(FeedEvent::Backfilled(empty)) = feed.events.recv().await else {
        panic!("expected the immediate empty backfill");
    };
    assert!(empty.is_empty());
    let mut chart = Chart {
        tape: expected[..200].to_vec(),
        ..Chart::default()
    };
    let held: quantick_engine::trade_tape::TradeTape = chart.tape.iter().cloned().collect();
    let seam = crate::ResumeSeam::of_tape(&held).expect("a held tape has a seam");
    assert_eq!(seam.held_at_last, 2, "two of the three prints at 198..=200");
    feed.commands
        .send(crate::FeedCommand::ResumeAfter(seam))
        .await
        .expect("feed alive");

    let mut bridge = FakeBridge::dial(19_263).await;
    bridge.backfill(&prints[500..560]).await;
    bridge.opening(&prints[330..500], 0).await;
    assert!(bridge.request().await.is_some(), "the hole was asked for");
    bridge.page(&prints[150..330], prints[150].time_ms).await;
    bridge.live(&prints[560..]).await;
    chart.fill_to(&mut feed, 600).await;

    assert!(chart.gaps.is_empty(), "{:?}", chart.gaps);
    assert!(same_prints(&chart.tape, &expected));
}

/// Session 2 of the fixture opens, sends the newest slice and one opening
/// slice, and is asked for the rest of the hole.
async fn second_session_asked(port: u16, prints: &[Print]) -> FakeBridge {
    let mut bridge = FakeBridge::dial(port).await;
    bridge.backfill(&prints[500..560]).await;
    bridge.opening(&prints[400..500], 1).await;
    let request = bridge.request().await;
    assert!(
        request
            .as_deref()
            .is_some_and(|r| r.contains("\"load_older\"")),
        "the hole was asked for: {request:?}"
    );
    bridge
}

/// Finding: the live tape was held for as long as the refill took, bounded
/// only by a million events — minutes of a frozen tape while the book moved.
/// A bridge that stops answering is let go at the stall limit: the refill
/// charts what it has, marks the rest, and lets the live tape go while the
/// session is still up.
#[tokio::test]
async fn a_refill_the_bridge_stops_answering_lets_the_live_tape_go_and_marks_the_rest() {
    let prints = fixture();
    let expected = reference(&prints);
    let stall = Duration::from_millis(400);
    let mut feed = feed_holding(19_264, stall, Duration::from_secs(60));
    let mut chart = Chart::default();
    first_session(&mut feed, 19_264, &mut chart, &prints).await;

    // The page is asked for and never answered; the tape runs on.
    let mut bridge = second_session_asked(19_264, &prints).await;
    bridge.live(&prints[560..580]).await;
    chart.fill_to(&mut feed, 380).await;

    let to_ms = prints[400].time_ms + OFFSET_MS;
    assert_eq!(
        chart.gaps,
        [crate::FeedGap {
            from_ms: prints[199].time_ms + OFFSET_MS,
            to_ms
        }],
        "the part the refill never reached is marked"
    );
    let mut want = expected[..200].to_vec();
    want.extend_from_slice(&expected[400..580]);
    assert!(
        same_prints(&chart.tape, &want),
        "what arrived is charted in order ({} prints)",
        chart.tape.len()
    );
    assert_eq!(
        chart.reports.last().map(|report| report.state),
        Some(crate::config::BackfillState::Partial {
            unfilled_to_ms: to_ms,
            stop: crate::config::BackfillStop::HeldTooLong
        })
    );
    // The session lives on, and its tape is no longer held.
    bridge.live(&prints[580..590]).await;
    chart.fill_to(&mut feed, 390).await;
    assert_eq!(chart.tape.len(), 390, "the live tape flows again");
}

/// Finding: a refill whose page the pager refused — a *load older* the
/// chart sent first was still on the wire — was never asked again, and the
/// tape stayed frozen. Settling the chart's page drives the refill.
#[tokio::test]
async fn a_refill_refused_the_pager_asks_again_once_the_charts_page_lands() {
    let prints = fixture();
    let expected = reference(&prints);
    let mut feed = feed_on(19_265);
    let mut chart = Chart::default();
    first_session(&mut feed, 19_265, &mut chart, &prints).await;

    let mut bridge = FakeBridge::dial(19_265).await;
    wait_for_notice(&mut feed, |n| matches!(n, FeedNotice::Connected)).await;
    feed.commands
        .send(crate::FeedCommand::LoadOlder { count: 100 })
        .await
        .expect("feed alive");
    let chart_ask = bridge.request().await;
    assert!(
        chart_ask.is_some_and(|r| r.contains("\"load_older\"")),
        "the chart's page went out first"
    );
    // The refill opens while the chart's page is on the wire: refused.
    bridge.backfill(&prints[500..560]).await;
    bridge.opening(&prints[330..500], 0).await;
    // Nothing is older than session 1's first print.
    bridge.page(&[], prints[0].time_ms - 1).await;
    let refill_ask = bridge.request().await;
    assert!(
        refill_ask.is_some_and(|r| r.contains("\"load_older\"")),
        "the refill asked again once the pager was free"
    );
    bridge.page(&prints[150..330], prints[150].time_ms).await;
    bridge.live(&prints[560..]).await;
    chart.fill_to(&mut feed, 600).await;

    assert!(chart.gaps.is_empty(), "{:?}", chart.gaps);
    assert!(same_prints(&chart.tape, &expected));
    // Nothing else was said meanwhile, so the refill clears its own notice.
    wait_for_notice(&mut feed, |n| matches!(n, FeedNotice::Clear)).await;
}

/// Finding: a finished refill always cleared the notice, so whatever needed
/// the trader meanwhile — here a second bridge dialing the same port — was
/// wiped off the tab by a step that never raised it.
#[tokio::test]
async fn a_refill_never_clears_a_notice_it_did_not_raise() {
    let prints = fixture();
    let mut feed = feed_on(19_266);
    let mut chart = Chart::default();
    first_session(&mut feed, 19_266, &mut chart, &prints).await;

    let mut bridge = second_session_asked(19_266, &prints).await;
    let _intruder = FakeBridge::dial(19_266).await;
    wait_for_notice(&mut feed, |n| matches!(n, FeedNotice::Attention { .. })).await;
    bridge.page(&prints[150..400], prints[150].time_ms).await;
    // A live print behind the refill: once it is charted, the release —
    // and any clear it would send — is over.
    bridge.live(&prints[560..561]).await;
    chart.fill_to(&mut feed, 561).await;
    assert_eq!(chart.tape.len(), 561);
    assert_eq!(
        chart.reports.last().map(|report| report.state),
        Some(crate::config::BackfillState::Filled)
    );
    while let Ok(notice) = feed.notices.try_recv() {
        assert_ne!(notice, FeedNotice::Clear, "the attention must stay up");
    }
}

/// Finding (review round 2, B4): the hold was one fifteen-second limit on
/// the whole refill, so a hole the bridge was still filling — a B3 morning
/// is a million prints — was cut and left a gap. Only a bridge that stops
/// answering trips the stall limit; one still sending pages goes on.
#[tokio::test]
async fn a_refill_still_receiving_pages_outlives_its_stall_limit() {
    let prints = fixture();
    let expected = reference(&prints);
    let stall = Duration::from_millis(700);
    let mut feed = feed_holding(19_267, stall, Duration::from_secs(60));
    let mut chart = Chart::default();
    first_session(&mut feed, 19_267, &mut chart, &prints).await;

    // Three pages, each well inside the stall limit and all three past it.
    let mut bridge = second_session_asked(19_267, &prints).await;
    for (from, to) in [(300, 400), (230, 300), (150, 230)] {
        tokio::time::sleep(stall * 2 / 3).await;
        bridge.page(&prints[from..to], prints[from].time_ms).await;
        if from > 150 {
            assert!(
                bridge.request().await.is_some(),
                "the next page was asked for"
            );
        }
    }
    bridge.live(&prints[560..]).await;
    chart.fill_to(&mut feed, 600).await;

    assert!(chart.gaps.is_empty(), "{:?}", chart.gaps);
    assert!(
        same_prints(&chart.tape, &expected),
        "{} prints",
        chart.tape.len()
    );
    assert_eq!(
        chart.reports.last().map(|report| report.state),
        Some(crate::config::BackfillState::Filled)
    );
}

/// The other half of B4: however steadily the bridge answers, the live tape
/// is held no longer than the hard limit.
#[tokio::test]
async fn a_refill_is_cut_at_its_hard_limit_while_pages_still_arrive() {
    let prints = fixture();
    let expected = reference(&prints);
    let page_every = Duration::from_millis(300);
    let mut feed = feed_holding(19_268, page_every * 2, page_every * 4);
    let mut chart = Chart::default();
    first_session(&mut feed, 19_268, &mut chart, &prints).await;

    // Pages of twenty, each answered 300 ms after it was asked: never late
    // enough for the stall limit, never reaching the chart's seam, and with
    // eight of them on offer, 2.4 s of pages against a 1.2 s cap.
    let mut bridge = second_session_asked(19_268, &prints).await;
    let mut from = 400;
    while from > 240 {
        tokio::time::sleep(page_every).await;
        bridge
            .page(&prints[from - 20..from], prints[from - 20].time_ms)
            .await;
        from -= 20;
        if bridge.request_within(page_every).await.is_none() {
            break; // the refill is over; nobody asks again
        }
    }
    assert!(
        from > 240,
        "the cap cut the refill before the pages ran out"
    );
    bridge.live(&prints[560..570]).await;
    let last = expected[569].timestamp_ms;
    while chart
        .tape
        .last()
        .is_none_or(|trade| trade.timestamp_ms != last)
    {
        match tokio::time::timeout(Duration::from_secs(5), feed.events.recv()).await {
            Ok(Some(event)) => chart.take(event),
            _ => panic!(
                "the live tape never came through ({} prints)",
                chart.tape.len()
            ),
        }
    }

    let Some(crate::config::BackfillState::Partial {
        unfilled_to_ms,
        stop,
    }) = chart.reports.last().map(|report| report.state)
    else {
        panic!("the refill was not cut: {:?}", chart.reports.last());
    };
    assert_eq!(stop, crate::config::BackfillStop::HeldTooLong);
    assert!(
        unfilled_to_ms > expected[240].timestamp_ms,
        "cut while pages were still arriving, not after they stopped"
    );
    assert_eq!(
        chart.gaps,
        [crate::FeedGap {
            from_ms: expected[199].timestamp_ms,
            to_ms: unfilled_to_ms
        }]
    );
    let mut want = expected[..200].to_vec();
    want.extend(
        expected[200..570]
            .iter()
            .filter(|trade| trade.timestamp_ms >= unfilled_to_ms)
            .cloned(),
    );
    assert!(
        same_prints(&chart.tape, &want),
        "{} prints",
        chart.tape.len()
    );
}
