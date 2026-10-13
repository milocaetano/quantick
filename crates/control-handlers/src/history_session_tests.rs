//! The History press and its cancel against a real tab session: one press,
//! one page and one cancel through the handlers leave the session exactly as
//! the same acts sent straight to [`TabSession::dispatch`] do.

use std::time::Instant;

use quantick_engine::trade_tape::TradeTape;
use quantick_engine::{Side, Trade};
use quantick_session::{
    Act, Effect, HistoryAct, HistoryOutcome, Msg, SessionSnapshot, TabSession, TabView,
    sets_window_default,
};
use quantick_sources::config::FeedCapabilities;
use quantick_sources::history_reach::{HistoryReach, Outage, ReachBounds};
use rust_decimal::Decimal;
use serde_json::json;

use super::*;
use crate::tabs::TabDirectory;
use crate::test_support::{FakeAccess, FakeWindow};

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

/// One tab whose History port is a real session: the second adapter after
/// the window's own, told the time once, reading each act's outcome and
/// moving the default the way the window's `Tab::load_history`,
/// `Tab::cancel_history` and `TabsMut::press_history` do.
struct SessionTab {
    session: TabSession,
    tape: TradeTape,
    now: Instant,
    window_default: Option<HistoryReach>,
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

impl HistoryPort for SessionTab {
    fn history_paging(&self, index: usize) -> Option<bool> {
        (index == 0).then_some(capabilities().history_paging)
    }
    fn dispatch_history(
        &mut self,
        index: usize,
        act: HistoryAct,
    ) -> Option<(HistoryOutcome, SessionSnapshot)> {
        if index != 0 {
            return None;
        }
        run(&mut self.session, &self.tape, act.act(), self.now);
        let outcome = match act {
            HistoryAct::Press(reach) => {
                let press = self.session.last_press().unwrap_or(Press::AlreadyRunning);
                if sets_window_default(press) {
                    self.window_default = Some(reach);
                }
                HistoryOutcome::Pressed(press)
            }
            HistoryAct::Cancel => {
                HistoryOutcome::Cancelled(self.session.last_cancel().unwrap_or(Cancelled::Nothing))
            }
        };
        Some((outcome, self.session.snapshot()))
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

    let mut tab = SessionTab {
        session: TabSession::new(),
        tape: tape(),
        now,
        window_default: None,
    };
    let pressed = call(load, &mut tab, json!({ "reach": YESTERDAY.token() }));
    assert_eq!(pressed["press"], "started");
    assert_eq!(pressed["status"]["state"], "loading");
    assert_eq!(tab.window_default, Some(YESTERDAY));
    run(&mut tab.session, &tab.tape, Act::HistoryPage(&page), now);
    let cancelled = call(cancel, &mut tab, json!({}));
    assert_eq!(cancelled["cancelled"], "run");
    assert_eq!(cancelled["status"]["state"], "idle");

    let mut direct = TabSession::new();
    let direct_tape = tape();
    run(&mut direct, &direct_tape, Act::PressHistory(YESTERDAY), now);
    run(&mut direct, &direct_tape, Act::HistoryPage(&page), now);
    run(&mut direct, &direct_tape, Act::CancelHistory, now);

    assert_eq!(tab.session.snapshot(), direct.snapshot());
    assert_eq!(tab.session.drain_events(), direct.drain_events());
}

#[test]
fn a_press_refused_while_a_run_pages_leaves_the_window_default_alone() {
    let mut tab = SessionTab {
        session: TabSession::new(),
        tape: tape(),
        now: Instant::now(),
        window_default: None,
    };
    call(load, &mut tab, json!({ "reach": YESTERDAY.token() }));
    tab.window_default = None;
    let again = call(load, &mut tab, json!({ "reach": "hours:1" }));
    assert_eq!(again["press"], "already_running");
    assert_eq!(again["status"]["main_reach"], YESTERDAY.token());
    assert_eq!(tab.window_default, None);
}
