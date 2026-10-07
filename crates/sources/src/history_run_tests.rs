//! One tab's history run: presses, the queue behind the opening fill, the
//! hidden-tab pause, cancellation and the honest end.

use super::*;
use crate::history_reach::{
    CAMPAIGN_PAGE_PRINTS, CampaignEnd, HistoryReach, MAX_HELD_PRINTS, ReachBounds, TapeFacts,
};
use quantick_engine::{Side, Trade};
use rust_decimal::Decimal;

const MINUTE: i64 = 60 * 1_000;
const HOUR: i64 = 60 * MINUTE;
const DAY: i64 = 24 * HOUR;

fn trade(ms: i64) -> Trade {
    Trade {
        agg_id: ms as u64,
        timestamp_ms: ms,
        price: Decimal::ONE,
        quantity: Decimal::ONE,
        side: Side::Buy,
    }
}

/// A print a minute from 09:00 to 18:00 of `day`.
fn session(day: i64) -> Vec<Trade> {
    (0..=9 * 60)
        .map(|minute| trade(day * DAY + 9 * HOUR + minute * MINUTE))
        .collect()
}

const YESTERDAY: HistoryReach = HistoryReach::Sessions(1);

/// A run that pressed Yesterday on a chart holding today, with its first
/// request out.
fn loading() -> HistoryRun {
    let mut run = HistoryRun::default();
    assert_eq!(run.press(YESTERDAY, true), Press::Start);
    let today = session(20);
    assert_eq!(
        run.begin(
            YESTERDAY,
            &today[..],
            &TapeFacts::default(),
            ReachBounds::default(),
            CAMPAIGN_PAGE_PRINTS
        ),
        RunAction::Send(CAMPAIGN_PAGE_PRINTS)
    );
    run
}

#[test]
fn the_main_click_repeats_this_tabs_last_action() {
    let mut run = HistoryRun::default();
    assert_eq!(
        run.main_reach(HistoryReach::default()),
        YESTERDAY,
        "a tab that never pressed repeats the window's default"
    );
    run.press(HistoryReach::Sessions(5), true);
    assert_eq!(
        run.main_reach(HistoryReach::Hours(2)),
        HistoryReach::Sessions(5)
    );
}

#[test]
fn a_press_during_the_opening_fill_is_queued_not_dropped() {
    let mut run = HistoryRun::default();
    assert_eq!(run.press(YESTERDAY, false), Press::Queued);
    assert_eq!(run.status(), RunStatus::Queued(YESTERDAY));
    assert_eq!(run.poll(false, true), Poll::Nothing, "still filling");
    assert_eq!(run.poll(true, true), Poll::Begin(YESTERDAY));
    assert_eq!(
        run.status(),
        RunStatus::Idle,
        "handed over, no longer queued"
    );
}

#[test]
fn the_latest_queued_press_wins() {
    let mut run = HistoryRun::default();
    run.press(YESTERDAY, false);
    run.press(HistoryReach::Sessions(3), false);
    assert_eq!(run.poll(true, true), Poll::Begin(HistoryReach::Sessions(3)));
}

#[test]
fn a_press_while_loading_starts_nothing_new() {
    let mut run = loading();
    assert_eq!(
        run.press(HistoryReach::Hours(2), true),
        Press::AlreadyRunning
    );
    assert!(matches!(run.status(), RunStatus::Loading(progress) if progress.reach == YESTERDAY));
}

#[test]
fn each_reply_on_a_visible_tab_asks_for_the_next_page_at_once() {
    let mut run = loading();
    let yesterday = session(19);
    assert_eq!(
        run.on_reply(&yesterday[300..], true, true),
        RunAction::Send(CAMPAIGN_PAGE_PRINTS)
    );
    assert!(
        run.holds_pages(),
        "the chart keeps the pages until the run ends"
    );
}

#[test]
fn a_hidden_tab_stops_paging_until_it_is_shown() {
    let mut run = loading();
    let yesterday = session(19);
    assert_eq!(
        run.on_reply(&yesterday[300..], true, false),
        RunAction::Wait
    );
    assert!(matches!(run.status(), RunStatus::Paused(_)));
    assert_eq!(run.poll(true, false), Poll::Nothing, "still hidden");
    assert_eq!(run.poll(true, true), Poll::Send(CAMPAIGN_PAGE_PRINTS));
    assert!(matches!(run.status(), RunStatus::Loading(_)));
}

#[test]
fn the_run_ends_on_its_target_and_releases_the_pages() {
    let mut run = loading();
    run.on_reply(&session(19), true, true);
    match run.on_reply(&session(18)[500..], true, true) {
        RunAction::Finished(outcome) => {
            assert_eq!(outcome.end, CampaignEnd::ReachMet);
            assert!(outcome.complete());
        }
        other => panic!("expected the end, got {other:?}"),
    }
    assert!(!run.holds_pages());
    assert_eq!(run.status(), RunStatus::Idle);
}

#[test]
fn cancel_keeps_what_arrived_and_says_where_it_stopped() {
    let mut run = loading();
    run.on_reply(&session(19), true, true);
    let Cancelled::Run(outcome) = run.cancel() else {
        panic!("a running run cancels with an outcome");
    };
    assert_eq!(outcome.end, CampaignEnd::Cancelled);
    assert_eq!(outcome.oldest_ms, Some(session(19)[0].timestamp_ms));
    assert!(!run.holds_pages());
    assert_eq!(
        run.cancel(),
        Cancelled::Nothing,
        "a second cancel is a no-op"
    );
}

#[test]
fn the_reply_to_a_cancelled_request_asks_for_nothing_more() {
    let mut run = loading();
    run.cancel();
    assert_eq!(
        run.press(YESTERDAY, true),
        Press::Queued,
        "the cancelled request is still out; a new run waits for its reply"
    );
    assert_eq!(run.on_reply(&session(19), true, true), RunAction::Wait);
    assert_eq!(run.poll(true, true), Poll::Begin(YESTERDAY));
}

#[test]
fn cancelling_a_queued_press_drops_it() {
    let mut run = HistoryRun::default();
    run.press(YESTERDAY, false);
    assert_eq!(run.cancel(), Cancelled::Queued(YESTERDAY));
    assert_eq!(run.poll(true, true), Poll::Nothing);
}

#[test]
fn a_symbol_change_drops_the_run_and_the_request_it_had_out() {
    let mut run = loading();
    run.press(HistoryReach::Hours(2), false);
    run.reset();
    assert_eq!(run.status(), RunStatus::Idle);
    assert!(!run.holds_pages());
    assert_eq!(
        run.press(YESTERDAY, true),
        Press::Start,
        "the old feed's request died with its channel"
    );
}

#[test]
fn a_request_that_could_not_be_sent_ends_the_run_partial() {
    let mut run = loading();
    let outcome = run.send_failed().expect("the run ends");
    assert_eq!(outcome.end, CampaignEnd::RequestRefused);
    assert!(!outcome.complete());
    assert_eq!(run.status(), RunStatus::Idle);
}

#[test]
fn a_target_already_on_the_chart_finishes_without_a_request() {
    let mut run = HistoryRun::default();
    run.press(YESTERDAY, true);
    let held: Vec<Trade> = (18..=20).flat_map(session).collect();
    match run.begin(
        YESTERDAY,
        &held[..],
        &TapeFacts::default(),
        ReachBounds::default(),
        1_000,
    ) {
        RunAction::Finished(outcome) => assert_eq!(outcome.end, CampaignEnd::AlreadyThere),
        other => panic!("{other:?}"),
    }
    assert_eq!(run.press(YESTERDAY, true), Press::Start, "nothing is out");
}

#[test]
fn a_press_on_a_chart_at_the_memory_ceiling_finishes_without_a_request() {
    let mut run = HistoryRun::default();
    run.press(YESTERDAY, true);
    let today = session(20);
    let full = TapeFacts {
        copies: MAX_HELD_PRINTS / (today.len() - 1),
        outages: Vec::new(),
    };
    match run.begin(YESTERDAY, &today[..], &full, ReachBounds::default(), 1_000) {
        RunAction::Finished(outcome) => assert_eq!(outcome.end, CampaignEnd::MemoryCeiling),
        other => panic!("{other:?}"),
    }
    assert!(!run.awaiting_reply(), "nothing was sent");
    assert_eq!(run.status(), RunStatus::Idle);
}

#[test]
fn a_reply_clears_the_request_out_even_after_a_cancel() {
    let mut run = loading();
    assert!(run.awaiting_reply());
    assert!(matches!(run.cancel(), Cancelled::Run(_)));
    assert!(
        run.awaiting_reply(),
        "the feed still owes the reply to the request already out"
    );
    assert_eq!(run.press(YESTERDAY, true), Press::Queued);
    assert_eq!(run.on_reply(&[], true, true), RunAction::Wait);
    assert!(!run.awaiting_reply());
    assert_eq!(run.poll(true, true), Poll::Begin(YESTERDAY));
}

#[test]
fn a_failed_rebuild_ends_the_run_but_keeps_the_press_queued_behind_it() {
    let mut run = HistoryRun::default();
    assert_eq!(run.press(YESTERDAY, false), Press::Queued);
    run.stop_keeping_queued();
    assert_eq!(run.status(), RunStatus::Queued(YESTERDAY));

    let mut run = loading();
    run.stop_keeping_queued();
    assert!(!run.holds_pages(), "the run ended");
    assert!(run.awaiting_reply(), "its request is still owed a reply");
}
