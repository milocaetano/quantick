//! The History press and its cancel against a real tab session: one press,
//! one page and one cancel through the handlers leave the session exactly as
//! the same acts sent straight to [`TabSession::dispatch`] do.

use std::time::Instant;

use quantick_engine::trade_tape::TradeTape;
use quantick_engine::{Side, Trade};
use quantick_session::{
    Act, Effect, HistoryAct, HistoryOutcome, HistoryReply, Msg, TabSession, TabView,
};
use quantick_sources::config::FeedCapabilities;
use quantick_sources::history_reach::{HistoryReach, Outage, ReachBounds};
use rust_decimal::Decimal;
use serde_json::json;

use super::*;
use crate::contract_roll::AutoRoll;
use crate::market_tab::FeedSelection;
use crate::market_tab::RecoveredFeed;
use crate::market_tab::{LentTab, MarketTab, MarketTabHost};
use crate::tabs::TabDirectory;
use crate::test_support::{FAKE_DEFAULT_REACH, FakeAccess, FakeFeeds, FakeWindow};

const MINUTE: i64 = 60 * 1_000;
const HOUR: i64 = 60 * MINUTE;
const DAY: i64 = 24 * HOUR;
const TAB: u64 = 3;
const YESTERDAY: HistoryReach = HistoryReach::Sessions(1);
const PAGE_STEP: usize = 2_000;

fn trade(ms: i64) -> Trade {
    Trade {
        agg_id: ms.unsigned_abs(),
        timestamp_ms: ms,
        price: Decimal::ONE,
        quantity: Decimal::ONE,
        side: Side::Buy,
    }
}

/// A print a minute from 09:00 to 18:00 of day 20.
fn tape() -> TradeTape {
    (0..=9 * 60)
        .map(|minute| trade(20 * DAY + 9 * HOUR + minute * MINUTE))
        .collect()
}

/// A full page of older trades, a print a second before the tape's open.
fn older_page() -> Vec<Trade> {
    let start = 20 * DAY + 9 * HOUR - (PAGE_STEP as i64) * 1_000;
    (0..PAGE_STEP as i64)
        .map(|second| trade(start + second * 1_000))
        .collect()
}

fn capabilities() -> FeedCapabilities {
    let mut capabilities = FeedCapabilities::none();
    capabilities.history_paging = true;
    capabilities
}

/// The window's half of the bus for one tab: wrap `act` in a [`Msg`] with
/// the tab's [`TabView`], dispatch it at `now`, and carry out what comes
/// back — a feed that takes every request, a resumed act judged at once.
fn run(session: &mut TabSession, tape: &TradeTape, act: Act<'_>, now: Instant) {
    let view = TabView {
        symbol: "WIN",
        capabilities: capabilities(),
        feed_pages: true,
        panes: &[],
        tape,
        history_wait: false,
        outages: &Vec::<Outage>::new,
        provider: None,
        page_step: PAGE_STEP,
        reach_bounds: ReachBounds::default(),
    };
    let Ok(effects) = session.dispatch(Msg { act, tab: &view }, now) else {
        return;
    };
    for effect in effects {
        match effect {
            Effect::Feed(request) => {
                let delivery = quantick_session::Delivery::Sent;
                run(session, tape, Act::FeedAck { request, delivery }, now);
            }
            Effect::Resume(resume) => run(session, tape, Act::Resume(resume), now),
            Effect::InstallPrefix { .. }
            | Effect::HoldPages(_)
            | Effect::RetryRebuilds
            | Effect::Wait(..) => {}
        }
    }
}

/// One tab whose History acts run on a real session, told the time once,
/// reading each act's outcome the way `Tab::load_history` and
/// `Tab::cancel_history` do; the window around it lends the default.
struct Live {
    session: TabSession,
    tape: TradeTape,
    now: Instant,
}

impl MarketTab<FakeFeeds> for Live {
    fn market(&self) -> FeedSelection {
        FeedSelection {
            feed_id: "fake".to_owned(),
            symbol: "FAKE".to_owned(),
            replay: false,
        }
    }
    fn switch_market(&mut self, _config: &FakeFeeds, _feed_id: &str, _symbol: &str) {}
    fn paper_open(&self) -> bool {
        false
    }
    fn unlisted(&self) -> Option<String> {
        None
    }
    fn roll_memory(&mut self) -> &mut AutoRoll {
        unreachable!("History calls never read the roll memory")
    }
    fn recover(&mut self, _config: &FakeFeeds, _keep_timeline: bool) -> RecoveredFeed {
        unreachable!("History calls never recover the feed")
    }
    fn pages_history(&self, _config: &FakeFeeds) -> bool {
        capabilities().history_paging
    }
    fn history(&mut self, _config: &FakeFeeds, act: HistoryAct) -> HistoryReply {
        run(&mut self.session, &self.tape, act.act(), self.now);
        let outcome = match act {
            HistoryAct::Press(_) => {
                HistoryOutcome::Pressed(self.session.last_press().unwrap_or(Press::AlreadyRunning))
            }
            HistoryAct::Cancel => {
                HistoryOutcome::Cancelled(self.session.last_cancel().unwrap_or(Cancelled::Nothing))
            }
        };
        (outcome, self.session.snapshot())
    }
}

/// The one-tab window around [`Live`].
struct SessionTab {
    live: Live,
    window_default: HistoryReach,
    feeds: FakeFeeds,
}

impl SessionTab {
    fn new(now: Instant) -> Self {
        Self {
            live: Live {
                session: TabSession::new(),
                tape: tape(),
                now,
            },
            window_default: FAKE_DEFAULT_REACH,
            feeds: FakeFeeds(Vec::new()),
        }
    }
}

impl TabDirectory for SessionTab {
    fn active_tab_index(&self) -> usize {
        0
    }
    fn tab_position(&self, tab_id: u64) -> Option<usize> {
        (tab_id == TAB).then_some(0)
    }
    fn tab_id_at(&self, _index: usize) -> u64 {
        TAB
    }
}

impl MarketTabHost for SessionTab {
    type Config = FakeFeeds;
    fn tab_count(&self) -> usize {
        1
    }
    fn feed_table(&self) -> &FakeFeeds {
        &self.feeds
    }
    fn market_tab(&self, index: usize) -> Option<&dyn MarketTab<FakeFeeds>> {
        (index == 0).then_some(&self.live as &dyn MarketTab<FakeFeeds>)
    }
    fn market_tab_mut(&mut self, index: usize) -> Option<LentTab<'_, FakeFeeds>> {
        (index == 0).then_some(LentTab {
            tab: &mut self.live,
            config: &self.feeds,
            history_default: &mut self.window_default,
        })
    }
    fn drawing_gesture_on(&self, _feed_id: &str, _from: &str, _to: &str) -> bool {
        false
    }
    fn carry_market_drawings(&mut self, _feed_id: &str, _from: &str, _to: &str) -> usize {
        0
    }
}

fn call(
    handler: fn(
        &mut SessionTab,
        &mut FakeAccess,
        &ActorContext,
        &Value,
    ) -> Result<Value, ControlError>,
    tab: &mut SessionTab,
    input: Value,
) -> Value {
    handler(
        tab,
        &mut FakeAccess::with_budget(0),
        &FakeWindow::assistant(),
        &input,
    )
    .expect("answered")
}

#[test]
fn press_page_and_cancel_through_the_handlers_match_the_session_bus() {
    let now = Instant::now();
    let page = older_page();

    let mut tab = SessionTab::new(now);
    let pressed = call(load, &mut tab, json!({ "reach": YESTERDAY.token() }));
    assert_eq!(pressed["press"], "started");
    assert_eq!(pressed["status"]["state"], "loading");
    assert_eq!(tab.window_default, YESTERDAY);
    run(
        &mut tab.live.session,
        &tab.live.tape,
        Act::HistoryPage(&page),
        now,
    );
    let cancelled = call(cancel, &mut tab, json!({}));
    assert_eq!(cancelled["cancelled"], "run");
    assert_eq!(cancelled["status"]["state"], "idle");

    let mut direct = TabSession::new();
    let direct_tape = tape();
    run(&mut direct, &direct_tape, Act::PressHistory(YESTERDAY), now);
    run(&mut direct, &direct_tape, Act::HistoryPage(&page), now);
    run(&mut direct, &direct_tape, Act::CancelHistory, now);

    assert_eq!(tab.live.session.snapshot(), direct.snapshot());
    assert_eq!(tab.live.session.drain_events(), direct.drain_events());
}

#[test]
fn a_press_refused_while_a_run_pages_leaves_the_window_default_alone() {
    let mut tab = SessionTab::new(Instant::now());
    call(load, &mut tab, json!({ "reach": YESTERDAY.token() }));
    tab.window_default = FAKE_DEFAULT_REACH;
    let again = call(load, &mut tab, json!({ "reach": "hours:1" }));
    assert_eq!(again["press"], "already_running");
    assert_eq!(again["status"]["main_reach"], YESTERDAY.token());
    assert_eq!(tab.window_default, FAKE_DEFAULT_REACH);
}
