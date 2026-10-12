//! The command bus, driven the way the window drives it: one act, the facts
//! it is judged against, and the effects carried out by a stub shell.

use super::*;
use quantick_engine::{Side, Trade};
use quantick_sources::candles::OlderCandles;
use quantick_sources::history_reach::ReachBounds;
use rust_decimal::Decimal;

const MINUTE: i64 = 60 * 1_000;
const HOUR: i64 = 60 * MINUTE;
const DAY: i64 = 24 * HOUR;
const TAB: u64 = 7;

fn trade(ms: i64) -> Trade {
    Trade {
        agg_id: ms as u64,
        timestamp_ms: ms,
        price: Decimal::ONE,
        quantity: Decimal::ONE,
        side: Side::Buy,
    }
}

fn candle(open_time: i64) -> Bar {
    Bar {
        open_time,
        close_time: open_time + MINUTE - 1,
        open: Decimal::ONE,
        high: Decimal::ONE,
        low: Decimal::ONE,
        close: Decimal::ONE,
        buy_volume: Decimal::ONE,
        sell_volume: Decimal::ZERO,
        trade_count: 1,
    }
}

/// A print a minute from 09:00 to 18:00 of `day`.
fn session(day: i64) -> Vec<Trade> {
    (0..=9 * 60)
        .map(|minute| trade(day * DAY + 9 * HOUR + minute * MINUTE))
        .collect()
}

/// The window's half of the bus: the tab facts it hands in, and what it
/// does with each effect — a feed that takes every request, panes that hold
/// what they were told.
struct Shell {
    tape: quantick_engine::trade_tape::TradeTape,
    intervals: Vec<Option<i64>>,
    capabilities: FeedCapabilities,
    opening_fill: bool,
    sent: Vec<FeedRequest>,
    delivery: Delivery,
    prefixes: Vec<usize>,
    held: bool,
    waits: Vec<(Wait, WaitChange)>,
    refusals: Vec<Refusal>,
}

impl Shell {
    fn new(intervals: &[Option<i64>]) -> Self {
        let mut capabilities = FeedCapabilities::none();
        capabilities.history_paging = true;
        capabilities.ohlcv_history = true;
        Self {
            tape: session(20).into_iter().collect(),
            intervals: intervals.to_vec(),
            capabilities,
            opening_fill: false,
            sent: Vec::new(),
            delivery: Delivery::Sent,
            prefixes: vec![0; intervals.len()],
            held: false,
            waits: Vec::new(),
            refusals: Vec::new(),
        }
    }

    fn dispatch(&mut self, session: &mut TabSession, act: Act<'_>, now: Instant) {
        let first_bar = candle(20 * DAY + 9 * HOUR);
        let panes: Vec<PaneView<'_>> = self
            .intervals
            .iter()
            .map(|&interval_ms| PaneView {
                interval_ms,
                first_print_open_ms: None,
                first_bar: Some(&first_bar),
                partial: None,
                rebuild_pending: false,
                rebuild_failed: false,
            })
            .collect();
        let view = TabView {
            symbol: "TEST",
            capabilities: self.capabilities,
            feed_pages: self.capabilities.history_paging,
            panes: &panes,
            tape: &self.tape,
            opening_fill: self.opening_fill,
            history_wait: false,
            outages: Vec::new(),
            provider: None,
            page_step: 2_000,
            reach_bounds: ReachBounds::default(),
        };
        let effects = match session.dispatch(Msg { act, tab: &view }, now) {
            Ok(effects) => effects,
            Err(refusal) => {
                self.refusals.push(refusal);
                return;
            }
        };
        drop(panes);
        for effect in effects {
            match effect {
                Effect::Feed(request) => {
                    if self.delivery == Delivery::Sent {
                        self.sent.push(request);
                    }
                    let delivery = self.delivery;
                    self.dispatch(session, Act::FeedAck { request, delivery }, now);
                }
                Effect::InstallPrefix { pane, bars, .. } => self.prefixes[pane] = bars.len(),
                Effect::HoldPages(held) => self.held = held,
                Effect::RetryRebuilds => {}
                Effect::Wait(wait, change) => self.waits.push((wait, change)),
                Effect::Resume(resume) => self.dispatch(session, Act::Resume(resume), now),
            }
        }
    }
}

const YESTERDAY: HistoryReach = HistoryReach::Sessions(1);

#[test]
fn a_press_on_an_idle_chart_asks_for_one_page_and_holds_the_panes() {
    let mut session = TabSession::new();
    let mut shell = Shell::new(&[None]);
    let now = Instant::now();
    shell.dispatch(&mut session, Act::PressHistory(YESTERDAY), now);
    assert!(matches!(
        shell.sent.as_slice(),
        [FeedRequest::OlderTrades { .. }]
    ));
    assert!(shell.held, "a run holds every pane's rebuild");
    assert_eq!(shell.waits, [(Wait::OlderTrades, WaitChange::Begin)]);
    let snapshot = session.snapshot();
    assert_eq!(snapshot.last_press, Some(Press::Start));
    assert!(matches!(snapshot.run, RunStatus::Loading(_)));
    assert_eq!(
        snapshot.main_reach, YESTERDAY,
        "the press sets the main click"
    );
}

#[test]
fn a_press_behind_the_opening_fill_queues_and_begins_when_it_lands() {
    let mut session = TabSession::new();
    let mut shell = Shell::new(&[None]);
    shell.opening_fill = true;
    let now = Instant::now();
    shell.dispatch(&mut session, Act::PressHistory(YESTERDAY), now);
    assert!(shell.sent.is_empty());
    assert_eq!(session.snapshot().last_press, Some(Press::Queued));
    shell.opening_fill = false;
    let policy = HistoryPolicy {
        progressive: true,
        venue_lead_in: false,
        frame: HistoryFrame::default(),
    };
    shell.dispatch(&mut session, Act::MirrorPolicy(policy), now);
    assert_eq!(shell.sent.len(), 1, "the queued press began on the poll");
}

#[test]
fn a_second_press_while_one_runs_is_refused_and_changes_nothing() {
    let mut session = TabSession::new();
    let mut shell = Shell::new(&[None]);
    let now = Instant::now();
    shell.dispatch(&mut session, Act::PressHistory(YESTERDAY), now);
    shell.dispatch(
        &mut session,
        Act::PressHistory(HistoryReach::Sessions(5)),
        now,
    );
    assert_eq!(shell.refusals, [Refusal::AlreadyRunning]);
    assert_eq!(shell.sent.len(), 1);
    assert_eq!(session.main_history_reach(), YESTERDAY);
}

#[test]
fn a_page_that_never_left_ends_the_run_and_releases_the_panes() {
    let mut session = TabSession::new();
    let mut shell = Shell::new(&[None]);
    shell.delivery = Delivery::Gone;
    shell.dispatch(&mut session, Act::PressHistory(YESTERDAY), Instant::now());
    assert!(!shell.held);
    assert_eq!(session.history_status(), RunStatus::Idle);
    assert!(session.history_note().is_some(), "never silent");
    assert!(
        shell.waits.is_empty(),
        "no wait for a request that is not out"
    );
}

#[test]
fn cancel_ends_the_run_with_a_note_that_lingers_for_the_time_it_is_told() {
    let mut session = TabSession::new();
    let mut shell = Shell::new(&[None]);
    let pressed = Instant::now();
    shell.dispatch(&mut session, Act::PressHistory(YESTERDAY), pressed);
    shell.dispatch(&mut session, Act::CancelHistory, pressed);
    assert!(matches!(
        session.snapshot().last_cancel,
        Some(Cancelled::Run(_))
    ));
    assert!(!shell.held);
    assert!(session.history_note().is_some());
    shell.dispatch(
        &mut session,
        Act::ExpireNote,
        pressed + HISTORY_NOTE_LINGER / 2,
    );
    assert!(session.history_note().is_some(), "still lingering");
    shell.dispatch(&mut session, Act::ExpireNote, pressed + HISTORY_NOTE_LINGER);
    assert!(
        session.history_note().is_none(),
        "gone once told it is time"
    );
    shell.dispatch(&mut session, Act::CancelHistory, pressed);
    assert_eq!(session.snapshot().last_cancel, Some(Cancelled::Nothing));
}

#[test]
fn a_time_pane_asks_for_candles_once_and_installs_what_arrives() {
    let mut session = TabSession::new();
    let mut shell = Shell::new(&[None, Some(5 * MINUTE)]);
    let now = Instant::now();
    shell.dispatch(&mut session, Act::PollCandles { tab_id: TAB }, now);
    shell.dispatch(&mut session, Act::PollCandles { tab_id: TAB }, now);
    let [FeedRequest::Candles { purpose, .. }] = shell.sent.as_slice() else {
        panic!("one candle request, got {:?}", shell.sent);
    };
    assert_eq!(*purpose, CandlePurpose::Opening);
    assert!(session.snapshot().candles_pending);
    let bars: Vec<Bar> = (0..60)
        .map(|minute| candle(19 * DAY + minute * MINUTE))
        .collect();
    shell.dispatch(
        &mut session,
        Act::CandleReply {
            tab_id: TAB,
            interval_ms: quantick_sources::ohlcv::OHLCV_BASE_INTERVAL_MS,
            bars,
            slice: OhlcvSlice::Last { complete: true },
        },
        now,
    );
    assert_eq!(session.venue_candles_held(), 60);
    assert_eq!(
        shell.prefixes,
        [0, 12],
        "folded to 5m on the time pane only"
    );
    assert_eq!(
        shell.waits,
        [
            (Wait::VenueCandles, WaitChange::Begin),
            (Wait::VenueCandles, WaitChange::End)
        ]
    );
}

#[test]
fn load_older_candles_says_why_it_cannot_and_reaches_behind_the_oldest_when_it_can() {
    let mut session = TabSession::new();
    let mut shell = Shell::new(&[Some(5 * MINUTE)]);
    let now = Instant::now();
    shell.dispatch(&mut session, Act::LoadOlderCandles { tab_id: TAB }, now);
    assert_eq!(
        shell.refusals,
        [Refusal::OlderCandles(OlderCandles::NotArrivedYet)]
    );
    shell.dispatch(&mut session, Act::PollCandles { tab_id: TAB }, now);
    let oldest = 19 * DAY;
    shell.dispatch(
        &mut session,
        Act::CandleReply {
            tab_id: TAB,
            interval_ms: quantick_sources::ohlcv::OHLCV_BASE_INTERVAL_MS,
            bars: vec![candle(oldest)],
            slice: OhlcvSlice::Last { complete: true },
        },
        now,
    );
    shell.dispatch(&mut session, Act::LoadOlderCandles { tab_id: TAB }, now);
    let Some(FeedRequest::Candles { ask, purpose, .. }) = shell.sent.last() else {
        panic!("an older request went out");
    };
    assert_eq!(ask.before_ms, Some(oldest - 1));
    assert_eq!(*purpose, CandlePurpose::Older { oldest_ms: oldest });
    // The venue re-sends what is held: nothing older, so the record starts.
    shell.dispatch(
        &mut session,
        Act::CandleReply {
            tab_id: TAB,
            interval_ms: quantick_sources::ohlcv::OHLCV_BASE_INTERVAL_MS,
            bars: vec![candle(oldest)],
            slice: OhlcvSlice::Last { complete: true },
        },
        now,
    );
    assert!(session.snapshot().older_exhausted);
    let panes = [PaneView {
        interval_ms: Some(5 * MINUTE),
        first_print_open_ms: None,
        first_bar: None,
        partial: None,
        rebuild_pending: false,
        rebuild_failed: false,
    }];
    assert_eq!(
        session.older_candles(shell.capabilities, &panes),
        OlderCandles::RecordStartsHere
    );
}

#[test]
fn a_tick_chart_wants_no_candles_until_the_lead_in_is_asked_for() {
    let mut session = TabSession::new();
    let mut shell = Shell::new(&[None]);
    let now = Instant::now();
    shell.dispatch(&mut session, Act::PollCandles { tab_id: TAB }, now);
    assert!(shell.sent.is_empty());
    shell.dispatch(&mut session, Act::SetVenueLeadIn(true), now);
    shell.dispatch(&mut session, Act::PollCandles { tab_id: TAB }, now);
    assert_eq!(shell.sent.len(), 1);
}

#[test]
fn a_market_switch_forgets_the_candles_and_the_run() {
    let mut session = TabSession::new();
    let mut shell = Shell::new(&[Some(5 * MINUTE)]);
    let now = Instant::now();
    shell.dispatch(&mut session, Act::PollCandles { tab_id: TAB }, now);
    shell.dispatch(
        &mut session,
        Act::CandleReply {
            tab_id: TAB,
            interval_ms: quantick_sources::ohlcv::OHLCV_BASE_INTERVAL_MS,
            bars: vec![candle(19 * DAY)],
            slice: OhlcvSlice::Last { complete: true },
        },
        now,
    );
    shell.dispatch(&mut session, Act::PressHistory(YESTERDAY), now);
    shell.dispatch(
        &mut session,
        Act::Attach {
            keep_timeline: true,
        },
        now,
    );
    assert_eq!(session.venue_candles_held(), 1, "a reconnect keeps them");
    assert_eq!(session.history_status(), RunStatus::Idle);
    shell.dispatch(
        &mut session,
        Act::Attach {
            keep_timeline: false,
        },
        now,
    );
    assert_eq!(session.venue_candles_held(), 0);
    assert!(!session.holds_venue_candles());
    assert_eq!(shell.prefixes, [0]);
    assert_eq!(
        shell.waits.last(),
        Some(&(Wait::VenueCandles, WaitChange::Clear))
    );
}

#[test]
fn the_trail_records_each_decision_in_order_and_stays_bounded() {
    let mut session = TabSession::new();
    let mut shell = Shell::new(&[None]);
    let now = Instant::now();
    shell.dispatch(&mut session, Act::PressHistory(YESTERDAY), now);
    let trail = session.drain_events();
    assert!(matches!(
        trail.as_slice(),
        [
            SessionEvent::Pressed {
                press: Press::Start,
                ..
            },
            SessionEvent::Asked(FeedRequest::OlderTrades { .. })
        ]
    ));
    assert!(session.drain_events().is_empty(), "drained once");
    for _ in 0..MAX_RETAINED_EVENTS + 10 {
        shell.dispatch(&mut session, Act::RaiseNote("n".to_owned()), now);
    }
    assert_eq!(session.drain_events().len(), MAX_RETAINED_EVENTS);
}
