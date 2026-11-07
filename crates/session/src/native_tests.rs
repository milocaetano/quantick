//! Venue candles on a feed that serves every interval: one series per pane
//! interval, held across interval switches, paged by scrolling.

use super::*;
use quantick_engine::time_bucket::DAY_MS;
use quantick_sources::candle_series::{CANDLE_PAGE_BARS, CandleSeries, REACH_BACK_MARGIN_BARS};
use quantick_sources::history_reach::{Outage, ReachBounds};
use rust_decimal::Decimal;

const MINUTE: i64 = 60_000;
const QUARTER: i64 = 15 * MINUTE;
const TAB: u64 = 3;
/// The live edge the fixtures' pages end at: a day boundary.
const EDGE: i64 = 20_000 * DAY_MS;

fn candle(open_time: i64, interval_ms: i64) -> Bar {
    Bar {
        open_time,
        close_time: open_time + interval_ms - 1,
        open: Decimal::ONE,
        high: Decimal::ONE,
        low: Decimal::ONE,
        close: Decimal::ONE,
        buy_volume: Decimal::ONE,
        sell_volume: Decimal::ZERO,
        trade_count: 1,
    }
}

/// `count` candles at `interval_ms`, the newest opening just before `end_ms`.
fn page(end_ms: i64, count: usize, interval_ms: i64) -> Vec<Bar> {
    let count = count as i64;
    (0..count)
        .map(|index| candle(end_ms - (count - index) * interval_ms, interval_ms))
        .collect()
}

/// One pane as the shell lays it out.
#[derive(Clone, Copy)]
struct Pane {
    interval_ms: Option<i64>,
    first_visible_slot: Option<usize>,
    first_print_open_ms: Option<i64>,
}

/// The window's half: a feed that takes every request, panes that keep the
/// prefix they were handed.
struct Shell {
    tape: quantick_engine::trade_tape::TradeTape,
    panes: Vec<Pane>,
    capabilities: FeedCapabilities,
    sent: Vec<FeedRequest>,
    prefixes: Vec<Vec<Bar>>,
    leads: Vec<(Option<Bar>, bool)>,
}

impl Shell {
    fn new(intervals: &[Option<i64>]) -> Self {
        let mut capabilities = FeedCapabilities::none();
        capabilities.ohlcv_history = true;
        capabilities.native_candle_intervals = true;
        Self {
            tape: quantick_engine::trade_tape::TradeTape::default(),
            panes: intervals
                .iter()
                .map(|&interval_ms| Pane {
                    interval_ms,
                    first_visible_slot: None,
                    first_print_open_ms: None,
                })
                .collect(),
            capabilities,
            sent: Vec::new(),
            prefixes: vec![Vec::new(); intervals.len()],
            leads: vec![(None, false); intervals.len()],
        }
    }

    fn dispatch(&mut self, session: &mut TabSession, act: Act<'_>) {
        let now = Instant::now();
        let panes: Vec<PaneView<'_>> = self
            .panes
            .iter()
            .map(|pane| PaneView {
                interval_ms: pane.interval_ms,
                first_print_open_ms: pane.first_print_open_ms,
                first_bar: None,
                partial: None,
                rebuild_pending: false,
                rebuild_failed: false,
                first_visible_slot: pane.first_visible_slot,
            })
            .collect();
        let view = TabView {
            symbol: "BTCUSDT",
            capabilities: self.capabilities,
            feed_pages: false,
            panes: &panes,
            tape: &self.tape,
            history_wait: false,
            outages: &Vec::<Outage>::new,
            provider: None,
            page_step: 2_000,
            reach_bounds: ReachBounds::default(),
        };
        let Ok(effects) = session.dispatch(Msg { act, tab: &view }, now) else {
            return;
        };
        drop(panes);
        for effect in effects {
            match effect {
                Effect::Feed(request) => {
                    self.sent.push(request);
                    let delivery = Delivery::Sent;
                    self.dispatch(session, Act::FeedAck { request, delivery });
                }
                Effect::InstallPrefix {
                    pane,
                    bars,
                    lead,
                    covers_seam,
                    ..
                } => {
                    self.prefixes[pane] = bars;
                    self.leads[pane] = (lead, covers_seam);
                }
                _ => {}
            }
        }
    }

    fn poll(&mut self, session: &mut TabSession) {
        self.dispatch(session, Act::PollCandles { tab_id: TAB });
    }

    /// Answer the last request with `bars`.
    fn reply(&mut self, session: &mut TabSession, bars: Vec<Bar>) {
        let Some(FeedRequest::Candles { ask, .. }) = self.sent.last().copied() else {
            panic!("no candle request to answer");
        };
        self.dispatch(
            session,
            Act::CandleReply {
                tab_id: TAB,
                interval_ms: ask.interval_ms,
                bars,
                slice: OhlcvSlice::Last { complete: true },
            },
        );
    }

    fn set_interval(&mut self, session: &mut TabSession, pane: usize, interval_ms: i64) {
        self.panes[pane].interval_ms = Some(interval_ms);
        self.dispatch(session, Act::Refold);
    }

    fn asks(&self) -> Vec<(CandleAsk, CandlePurpose)> {
        self.sent
            .iter()
            .filter_map(|request| match request {
                FeedRequest::Candles { ask, purpose, .. } => Some((*ask, *purpose)),
                FeedRequest::OlderTrades { .. } => None,
            })
            .collect()
    }
}

#[test]
fn a_15m_pane_opens_with_one_request_at_its_own_interval() {
    let mut session = TabSession::new();
    let mut shell = Shell::new(&[Some(QUARTER)]);
    shell.poll(&mut session);
    shell.poll(&mut session);
    assert_eq!(
        shell.asks(),
        [(CandleSeries::opening_ask(QUARTER), CandlePurpose::Opening)]
    );
    assert_eq!(
        session
            .candle_status(Some(QUARTER))
            .map(|status| status.state),
        Some(SeriesState::Loading)
    );
    shell.reply(&mut session, page(EDGE, CANDLE_PAGE_BARS, QUARTER));
    assert_eq!(
        shell.prefixes[0].len(),
        CANDLE_PAGE_BARS,
        "installed unfolded"
    );
    shell.poll(&mut session);
    assert_eq!(shell.asks().len(), 1, "a held series asks nothing more");
    assert_eq!(
        session.candle_status(Some(QUARTER)),
        Some(SeriesStatus {
            state: SeriesState::Idle,
            interval_ms: QUARTER,
            candles_held: CANDLE_PAGE_BARS,
            oldest_open_ms: Some(EDGE - CANDLE_PAGE_BARS as i64 * QUARTER),
        })
    );
}

#[test]
fn a_trip_to_1d_and_back_costs_one_request_for_the_days() {
    let mut session = TabSession::new();
    let mut shell = Shell::new(&[Some(QUARTER)]);
    shell.poll(&mut session);
    shell.reply(&mut session, page(EDGE, CANDLE_PAGE_BARS, QUARTER));
    shell.set_interval(&mut session, 0, DAY_MS);
    shell.poll(&mut session);
    shell.poll(&mut session);
    assert_eq!(shell.asks().len(), 2);
    assert_eq!(shell.asks()[1].0, CandleSeries::opening_ask(DAY_MS));
    shell.reply(&mut session, page(EDGE, 2_000 / 2, DAY_MS));
    assert_eq!(shell.prefixes[0].len(), 1_000);
    shell.set_interval(&mut session, 0, QUARTER);
    shell.poll(&mut session);
    assert_eq!(shell.asks().len(), 2, "15m is held: no request");
    assert_eq!(shell.prefixes[0].len(), CANDLE_PAGE_BARS);
    assert_eq!(
        shell.prefixes[0][0].close_time - shell.prefixes[0][0].open_time,
        QUARTER - 1
    );
}

#[test]
fn a_daily_pane_beside_a_15m_pane_reads_its_own_series() {
    let mut session = TabSession::new();
    let mut shell = Shell::new(&[Some(QUARTER), Some(DAY_MS)]);
    shell.poll(&mut session);
    shell.poll(&mut session);
    assert_eq!(shell.asks().len(), 1, "one request out at a time");
    shell.reply(&mut session, page(EDGE, CANDLE_PAGE_BARS, QUARTER));
    shell.poll(&mut session);
    assert_eq!(shell.asks().len(), 2);
    assert_eq!(shell.asks()[1].0, CandleSeries::opening_ask(DAY_MS));
    // A market listed 2,500 days ago: years of days, short of a full page.
    shell.reply(&mut session, page(EDGE, 900, DAY_MS));
    assert_eq!(shell.prefixes[0].len(), CANDLE_PAGE_BARS);
    assert_eq!(shell.prefixes[1].len(), 900);
    assert_eq!(
        session
            .candle_status(Some(DAY_MS))
            .map(|status| status.state),
        Some(SeriesState::Exhausted),
        "a short daily page is the record's start"
    );
}

#[test]
fn scrolling_near_the_oldest_bar_asks_once_and_prepends_behind_the_view() {
    let mut session = TabSession::new();
    let mut shell = Shell::new(&[Some(QUARTER)]);
    shell.poll(&mut session);
    shell.reply(&mut session, page(EDGE, CANDLE_PAGE_BARS, QUARTER));
    let before = shell.prefixes[0].clone();
    let oldest = before[0].open_time;

    shell.panes[0].first_visible_slot = Some(REACH_BACK_MARGIN_BARS);
    shell.poll(&mut session);
    assert_eq!(shell.asks().len(), 1, "outside the margin nothing is asked");

    shell.panes[0].first_visible_slot = Some(12);
    shell.poll(&mut session);
    shell.poll(&mut session);
    assert_eq!(
        shell.asks().len(),
        2,
        "one older request, however many frames"
    );
    let (ask, purpose) = shell.asks()[1];
    assert_eq!(ask.before_ms, Some(oldest - 1));
    assert_eq!(ask.span_ms, CANDLE_PAGE_BARS as i64 * QUARTER);
    assert_eq!(purpose, CandlePurpose::Older { oldest_ms: oldest });

    shell.reply(&mut session, page(oldest, CANDLE_PAGE_BARS, QUARTER));
    let after = &shell.prefixes[0];
    assert_eq!(after.len(), 2 * CANDLE_PAGE_BARS);
    assert_eq!(
        after[CANDLE_PAGE_BARS..],
        before[..],
        "the newest candles stay where they were: only the left grows"
    );
}

#[test]
fn an_empty_answer_at_an_interval_is_not_asked_again() {
    let mut session = TabSession::new();
    let mut shell = Shell::new(&[Some(QUARTER)]);
    shell.poll(&mut session);
    shell.reply(&mut session, Vec::new());
    shell.panes[0].first_visible_slot = Some(0);
    shell.poll(&mut session);
    shell.poll(&mut session);
    assert_eq!(shell.asks().len(), 1);
    assert_eq!(
        session
            .candle_status(Some(QUARTER))
            .map(|status| status.state),
        Some(SeriesState::Exhausted)
    );
}

#[test]
fn a_pane_between_served_intervals_folds_the_coarsest_that_fits() {
    let mut session = TabSession::new();
    let mut shell = Shell::new(&[Some(6 * 60 * MINUTE)]);
    shell.poll(&mut session);
    assert_eq!(shell.asks()[0].0.interval_ms, 2 * 60 * MINUTE);
    shell.reply(&mut session, page(EDGE, 900, 2 * 60 * MINUTE));
    assert_eq!(shell.prefixes[0].len(), 300, "three 2h candles per 6h bar");
}

#[test]
fn a_market_switch_drops_every_series() {
    let mut session = TabSession::new();
    let mut shell = Shell::new(&[Some(QUARTER)]);
    shell.poll(&mut session);
    shell.reply(&mut session, page(EDGE, CANDLE_PAGE_BARS, QUARTER));
    shell.dispatch(
        &mut session,
        Act::Attach {
            keep_timeline: false,
        },
    );
    assert!(shell.prefixes[0].is_empty());
    assert_eq!(
        session.candle_status(Some(QUARTER)).map(|s| s.candles_held),
        Some(0)
    );
    shell.poll(&mut session);
    assert_eq!(shell.asks().len(), 2, "the new market opens afresh");
}

#[test]
fn a_feed_without_the_capability_keeps_the_minute_base() {
    let mut session = TabSession::new();
    let mut shell = Shell::new(&[Some(QUARTER)]);
    shell.capabilities.native_candle_intervals = false;
    shell.poll(&mut session);
    assert_eq!(
        shell.asks()[0].0.interval_ms,
        quantick_sources::ohlcv::OHLCV_BASE_INTERVAL_MS
    );
}

#[test]
fn the_seam_bar_reads_the_minutes_before_the_first_trade() {
    let mut session = TabSession::new();
    let mut shell = Shell::new(&[Some(QUARTER)]);
    let first_trade = EDGE + 7 * MINUTE;
    shell.panes[0].first_print_open_ms = Some(first_trade);
    shell.poll(&mut session);
    shell.reply(&mut session, page(EDGE, CANDLE_PAGE_BARS, QUARTER));
    shell.poll(&mut session);
    shell.poll(&mut session);
    assert_eq!(shell.asks().len(), 2, "one small request for the seam");
    let (ask, purpose) = shell.asks()[1];
    assert_eq!(purpose, CandlePurpose::SeamMinutes);
    assert_eq!(ask.interval_ms, MINUTE);
    assert_eq!(ask.before_ms, Some(first_trade - 1));
    assert_eq!(ask.span_ms, 7 * MINUTE);
    shell.reply(&mut session, page(first_trade, 7, MINUTE));
    let (lead, covers_seam) = &shell.leads[0];
    assert!(covers_seam, "seven minutes vouch for the stretch");
    assert_eq!(lead.as_ref().map(|bar| bar.trade_count), Some(7));
    shell.poll(&mut session);
    assert_eq!(shell.asks().len(), 2, "asked once");
}
