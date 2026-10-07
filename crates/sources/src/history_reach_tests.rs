//! The reach targets and the campaign that serves them, judged on raw pages.
//!
//! Every stop condition is a fixture here rather than a session against a
//! live venue: the campaign is told what the chart held at the press and what
//! each page brought, and answers what to do next.

use super::*;
use quantick_engine::{Side, Trade};
use rust_decimal::Decimal;

/// A print at `ms`. Only the stamp matters: the reach is arithmetic over time.
fn trade(ms: i64) -> Trade {
    Trade {
        agg_id: ms as u64,
        timestamp_ms: ms,
        price: Decimal::ONE,
        quantity: Decimal::ONE,
        side: Side::Buy,
    }
}

/// Prints every `step_ms` from `start_ms`, `count` of them.
fn run(start_ms: i64, step_ms: i64, count: usize) -> Vec<Trade> {
    (0..count)
        .map(|i| trade(start_ms + step_ms * i as i64))
        .collect()
}

const MINUTE: i64 = 60 * 1_000;
const HOUR: i64 = 60 * MINUTE;
const DAY: i64 = 24 * HOUR;

/// One B3-shaped session: a print a minute from 09:00 to 18:00 of `day`.
fn session(day: i64) -> Vec<Trade> {
    run(day * DAY + 9 * HOUR, MINUTE, 9 * 60 + 1)
}

/// Sessions `first..=last`, oldest first, as one tape.
fn sessions(first: i64, last: i64) -> Vec<Trade> {
    (first..=last).flat_map(session).collect()
}

fn bounds() -> ReachBounds {
    ReachBounds::default()
}

/// Start a campaign that must run, or fail the test with what it did instead.
fn running(held: &[Trade], reach: HistoryReach) -> Campaign {
    match Campaign::start(
        held,
        &TapeFacts::default(),
        reach,
        bounds(),
        CAMPAIGN_PAGE_PRINTS,
    ) {
        CampaignStart::Run(campaign) => campaign,
        other => panic!(
            "{reach:?} over {} prints did not run: {other:?}",
            held.len()
        ),
    }
}

// --- targets and tokens ---------------------------------------------------

#[test]
fn the_menu_offers_hours_before_the_oldest_and_sessions_back_from_the_edge() {
    assert_eq!(
        HistoryReach::PRESETS,
        [
            HistoryReach::Hours(2),
            HistoryReach::Hours(4),
            HistoryReach::Sessions(1),
            HistoryReach::Sessions(3),
            HistoryReach::Sessions(5),
        ]
    );
    let labels: Vec<String> = HistoryReach::PRESETS.iter().map(|r| r.label()).collect();
    assert_eq!(labels, ["+2 h", "+4 h", "Yesterday", "3 days", "5 days"]);
    for reach in HistoryReach::PRESETS {
        assert!(!reach.hover().is_empty(), "{reach:?} has no hover");
    }
}

#[test]
fn the_main_click_defaults_to_yesterday() {
    assert_eq!(HistoryReach::default(), HistoryReach::Sessions(1));
}

#[test]
fn every_target_round_trips_through_its_token() {
    for reach in HistoryReach::PRESETS.into_iter().chain([
        HistoryReach::Hours(MAX_REACH_HOURS),
        HistoryReach::Sessions(MAX_REACH_SESSIONS),
    ]) {
        assert_eq!(HistoryReach::parse(&reach.token()), Ok(reach), "{reach:?}");
    }
    assert_eq!(HistoryReach::Hours(2).token(), "hours:2");
    assert_eq!(HistoryReach::Sessions(5).token(), "sessions:5");
    assert_eq!(
        HistoryReach::parse(" sessions : 3 "),
        Ok(HistoryReach::Sessions(3))
    );
}

/// Saved workspaces and launch hooks written before the targets existed keep
/// loading: `page` was a couple of minutes of WIN, `previous-session` was
/// yesterday, and `span` was the configured hours.
#[test]
fn old_tokens_still_parse_onto_the_new_targets() {
    assert_eq!(HistoryReach::parse("page"), Ok(HistoryReach::Hours(2)));
    assert_eq!(
        HistoryReach::parse("previous-session"),
        Ok(HistoryReach::Sessions(1))
    );
    assert_eq!(HistoryReach::parse("span"), Ok(HistoryReach::Hours(2)));
    assert_eq!(
        HistoryReach::from_legacy_span("span", 300),
        Some(HistoryReach::Hours(5)),
        "a saved five-hour span stays five hours"
    );
    assert_eq!(
        HistoryReach::from_legacy_span("span", 90),
        Some(HistoryReach::Hours(2)),
        "a part hour rounds up rather than reaching less than was saved"
    );
    assert_eq!(
        HistoryReach::from_legacy_span("sessions:3", 90),
        Some(HistoryReach::Sessions(3)),
        "the span only reads into the old token"
    );
}

#[test]
fn bad_targets_are_refused_with_the_reason() {
    assert_eq!(HistoryReach::parse(""), Err(ReachParseError::Empty));
    assert!(matches!(
        HistoryReach::parse("weeks:2"),
        Err(ReachParseError::UnknownKind(kind)) if kind == "weeks"
    ));
    assert!(matches!(
        HistoryReach::parse("hours:two"),
        Err(ReachParseError::BadCount(_))
    ));
    assert_eq!(
        HistoryReach::parse("hours:0"),
        Err(ReachParseError::OutOfRange {
            kind: "hours",
            count: 0,
            max: MAX_REACH_HOURS
        })
    );
    assert_eq!(
        HistoryReach::parse(&format!("sessions:{}", MAX_REACH_SESSIONS + 1)),
        Err(ReachParseError::OutOfRange {
            kind: "sessions",
            count: MAX_REACH_SESSIONS + 1,
            max: MAX_REACH_SESSIONS
        })
    );
    for error in [
        ReachParseError::Empty,
        ReachParseError::UnknownKind("weeks".into()),
        ReachParseError::BadCount("two".into()),
    ] {
        assert!(
            error.to_string().contains("hours:N") && error.to_string().contains("sessions:N"),
            "the refusal names the grammar: {error}"
        );
    }
}

// --- budgets scale with the target ------------------------------------------

#[test]
fn the_print_budget_scales_with_the_target() {
    assert_eq!(
        HistoryReach::Sessions(5).print_budget(),
        6 * PRINTS_PER_SESSION_BUDGET,
        "five sessions back crosses into a sixth to see its close"
    );
    assert_eq!(
        HistoryReach::Sessions(1).print_budget(),
        2 * PRINTS_PER_SESSION_BUDGET
    );
    assert_eq!(
        HistoryReach::Hours(4).print_budget(),
        4 * PRINTS_PER_TRADED_HOUR_BUDGET
    );
}

#[test]
fn the_page_budget_covers_the_print_budget_and_is_capped() {
    let pages = HistoryReach::Sessions(5).page_budget(CAMPAIGN_PAGE_PRINTS);
    assert!(
        pages as usize * CAMPAIGN_PAGE_PRINTS >= HistoryReach::Sessions(5).print_budget(),
        "the request count never ends a run its print budget allows: {pages}"
    );
    assert_eq!(
        HistoryReach::Sessions(5).page_budget(1),
        MAX_CAMPAIGN_PAGES,
        "a venue with tiny pages is not asked thousands of times"
    );
}

// --- starting a run -----------------------------------------------------------

#[test]
fn a_target_already_on_the_chart_sends_no_request() {
    // Days 10..=12 held, live edge inside day 12: yesterday's open is there.
    let held = sessions(10, 12);
    match Campaign::start(
        &held[..],
        &TapeFacts::default(),
        HistoryReach::Sessions(1),
        bounds(),
        1_000,
    ) {
        CampaignStart::AlreadyMet(outcome) => {
            assert_eq!(outcome.end, CampaignEnd::AlreadyThere);
            assert_eq!(outcome.oldest_ms, Some(held[0].timestamp_ms));
        }
        other => panic!("expected no request, got {other:?}"),
    }
}

#[test]
fn an_empty_chart_has_nothing_to_page_back_from() {
    assert!(matches!(
        Campaign::start(&[] as &[Trade], &TapeFacts::default(), HistoryReach::Hours(2), bounds(), 1_000),
        CampaignStart::NothingCharted(outcome) if outcome.end == CampaignEnd::NothingCharted
    ));
}

// --- sessions back from the live edge -----------------------------------------

#[test]
fn yesterday_lands_on_the_previous_sessions_open_not_three_hours_into_it() {
    // Today only is held; yesterday arrives in two pages, newest first.
    let today = session(20);
    let mut campaign = running(&today, HistoryReach::Sessions(1));
    let yesterday = session(19);
    let (morning, afternoon) = yesterday.split_at(3 * 60);
    assert_eq!(
        campaign.advance(afternoon, true),
        CampaignStep::Ask,
        "the old reach stopped here, three hours into yesterday"
    );
    assert_eq!(
        campaign.advance(morning, true),
        CampaignStep::Ask,
        "the open is on the chart, but nothing yet shows it was an open"
    );
    // The page that crosses the night before yesterday proves the open.
    let day_before = session(18);
    assert_eq!(
        campaign.advance(&day_before[day_before.len() - 10..], true),
        CampaignStep::Stop(CampaignEnd::ReachMet)
    );
    let outcome = campaign.finish(CampaignEnd::ReachMet);
    assert_eq!(outcome.sessions_reached, 1);
    assert_eq!(outcome.reached_open_ms, Some(yesterday[0].timestamp_ms));
    assert!(outcome.complete());
}

#[test]
fn sessions_are_counted_from_the_live_edge_not_from_the_oldest_print() {
    // Today and yesterday already held: three days back needs two more.
    let held = sessions(19, 20);
    let mut campaign = running(&held, HistoryReach::Sessions(3));
    assert_eq!(
        campaign.progress().sessions_reached,
        0,
        "yesterday is held, but no close before it proves its open yet"
    );
    assert_eq!(campaign.advance(&session(18), true), CampaignStep::Ask);
    assert_eq!(campaign.advance(&session(17), true), CampaignStep::Ask);
    assert_eq!(
        campaign.advance(&session(16)[500..], true),
        CampaignStep::Stop(CampaignEnd::ReachMet),
        "the close before day 17 proves day 17's open"
    );
    assert_eq!(campaign.progress().sessions_reached, 3);
}

#[test]
fn five_days_crosses_a_weekend_and_still_counts_sessions() {
    // Monday is day 21; the weekend (days 19 and 20) never traded.
    let monday = session(21);
    let mut campaign = running(&monday, HistoryReach::Sessions(5));
    for day in [18, 17, 16, 15, 14] {
        assert_eq!(
            campaign.advance(&session(day), true),
            CampaignStep::Ask,
            "day {day}"
        );
    }
    assert_eq!(
        campaign.advance(&session(11)[400..], true),
        CampaignStep::Stop(CampaignEnd::ReachMet)
    );
    let outcome = campaign.finish(CampaignEnd::ReachMet);
    assert_eq!(outcome.sessions_reached, 5);
    assert!(!outcome.gapless);
}

#[test]
fn a_gapless_feed_counts_a_day_as_twenty_four_hours_of_tape() {
    // A print every ten minutes, never a close: the crypto case.
    let edge = 40 * DAY;
    let held = run(edge - 2 * HOUR, 10 * MINUTE, 13);
    let mut campaign = running(&held, HistoryReach::Sessions(1));
    let page = run(edge - 20 * HOUR, 10 * MINUTE, 108);
    assert_eq!(campaign.advance(&page, true), CampaignStep::Ask);
    let page = run(edge - 26 * HOUR, 10 * MINUTE, 36);
    assert_eq!(
        campaign.advance(&page, true),
        CampaignStep::Stop(CampaignEnd::ReachMet)
    );
    let outcome = campaign.finish(CampaignEnd::ReachMet);
    assert!(outcome.gapless, "the note has to say what a day meant here");
    assert!(
        outcome
            .sentence(|ms| format!("t{ms}"))
            .contains("24 h of tape"),
        "{}",
        outcome.sentence(|ms| format!("t{ms}"))
    );
}

// --- hours before the oldest print --------------------------------------------

#[test]
fn hours_count_traded_time_before_the_oldest_print_and_cross_nights() {
    // The oldest held print is today's open; +2 h must come out of yesterday.
    let today = session(20);
    let mut campaign = running(&today, HistoryReach::Hours(2));
    let yesterday = session(19);
    assert_eq!(
        campaign.advance(&yesterday[yesterday.len() - 61..], true),
        CampaignStep::Ask,
        "the night adds nothing; one traded hour is not two"
    );
    assert_eq!(
        campaign.advance(
            &yesterday[yesterday.len() - 122..yesterday.len() - 61],
            true
        ),
        CampaignStep::Stop(CampaignEnd::ReachMet)
    );
    assert_eq!(campaign.progress().traded_ms, 2 * HOUR);
}

// --- stop conditions ----------------------------------------------------------

#[test]
fn a_venue_that_has_run_out_ends_the_run_partial() {
    let today = session(20);
    let mut campaign = running(&today, HistoryReach::Sessions(5));
    assert_eq!(
        campaign.advance(&session(19), false),
        CampaignStep::Stop(CampaignEnd::Exhausted)
    );
    let outcome = campaign.finish(CampaignEnd::Exhausted);
    assert!(!outcome.complete());
    assert_eq!(
        outcome.sessions_reached, 0,
        "yesterday's open is not proven"
    );
}

#[test]
fn the_last_page_still_counts_when_the_venue_runs_out_with_it() {
    let today = session(20);
    let mut campaign = running(&today, HistoryReach::Sessions(1));
    let mut page = session(18)[500..].to_vec();
    page.extend(session(19));
    assert_eq!(
        campaign.advance(&page, false),
        CampaignStep::Stop(CampaignEnd::ReachMet),
        "the target was met by the page that ended the record"
    );
}

#[test]
fn a_venue_that_keeps_answering_empty_is_not_asked_forever() {
    let today = session(20);
    let mut campaign = running(&today, HistoryReach::Sessions(1));
    for page in 1..MAX_IDLE_PAGES {
        assert_eq!(campaign.advance(&[], true), CampaignStep::Ask, "{page}");
    }
    assert_eq!(
        campaign.advance(&[], true),
        CampaignStep::Stop(CampaignEnd::NothingComingBack)
    );
}

#[test]
fn a_page_that_only_repeats_what_is_held_is_idle() {
    let today = session(20);
    let mut campaign = running(&today, HistoryReach::Sessions(1));
    let repeat = vec![today[0].clone(), today[1].clone()];
    for _ in 1..MAX_IDLE_PAGES {
        assert_eq!(campaign.advance(&repeat, true), CampaignStep::Ask);
    }
    assert_eq!(
        campaign.advance(&repeat, true),
        CampaignStep::Stop(CampaignEnd::NothingComingBack)
    );
}

#[test]
fn the_print_budget_ends_a_run_partial_and_clips_the_last_request() {
    let today = session(20);
    let mut campaign = running(&today, HistoryReach::Hours(1));
    let budget = HistoryReach::Hours(1).print_budget();
    // One dense page of a print per millisecond, never an hour of tape.
    let page = run(today[0].timestamp_ms - budget as i64, 1, budget - 7);
    assert_eq!(campaign.advance(&page, true), CampaignStep::Ask);
    assert_eq!(
        campaign.next_request(),
        7,
        "the last request cannot overspend"
    );
    let page = run(page[0].timestamp_ms - 7, 1, 7);
    assert_eq!(
        campaign.advance(&page, true),
        CampaignStep::Stop(CampaignEnd::PrintsPulled)
    );
}

/// Facts for a tab whose every pane holds its own copy of the tape, with as
/// many copies as leave `per_copy` prints of the ceiling to each.
fn copies_leaving(per_copy: usize) -> TapeFacts {
    TapeFacts {
        copies: MAX_HELD_PRINTS / per_copy,
        outages: Vec::new(),
    }
}

#[test]
fn the_memory_ceiling_counts_every_panes_copy_of_the_tape() {
    let today = session(20);
    let facts = copies_leaving(today.len() + 10);
    let per_copy = MAX_HELD_PRINTS / facts.copies;
    let mut campaign = match Campaign::start(
        &today[..],
        &facts,
        HistoryReach::Sessions(5),
        bounds(),
        CAMPAIGN_PAGE_PRINTS,
    ) {
        CampaignStart::Run(campaign) => campaign,
        other => panic!("{other:?}"),
    };
    let room = per_copy - today.len();
    assert_eq!(
        campaign.next_request(),
        room,
        "every pane holds the page, so each copy's share bounds the request"
    );
    let page = run(today[0].timestamp_ms - room as i64 * MINUTE, MINUTE, room);
    assert_eq!(
        campaign.advance(&page, true),
        CampaignStep::Stop(CampaignEnd::MemoryCeiling)
    );
}

#[test]
fn a_press_at_the_memory_ceiling_asks_for_nothing_and_says_why() {
    let today = session(20);
    let facts = copies_leaving(today.len() - 1);
    match Campaign::start(
        &today[..],
        &facts,
        HistoryReach::Sessions(1),
        bounds(),
        CAMPAIGN_PAGE_PRINTS,
    ) {
        CampaignStart::AtCeiling(outcome) => {
            assert_eq!(outcome.end, CampaignEnd::MemoryCeiling);
            assert!(!outcome.complete());
            assert!(
                outcome
                    .sentence(|ms| format!("<{ms}>"))
                    .contains(CampaignEnd::MemoryCeiling.reason()),
                "the note names the ceiling"
            );
        }
        other => panic!("a full chart must not send a one-print request: {other:?}"),
    }
}

/// Today with a two-hour hole at 11:00 that the feed marked as its own
/// outage, and the outage it marked.
fn today_with_an_outage() -> (Vec<Trade>, Outage) {
    let today: Vec<Trade> = session(20)
        .into_iter()
        .filter(|trade| {
            let minute_of_day = (trade.timestamp_ms - 20 * DAY) / MINUTE;
            !(11 * 60 < minute_of_day && minute_of_day < 13 * 60)
        })
        .collect();
    let outage = Outage {
        from_ms: 20 * DAY + 11 * HOUR,
        to_ms: 20 * DAY + 13 * HOUR,
    };
    (today, outage)
}

#[test]
fn a_feed_outage_inside_today_is_not_counted_as_a_close() {
    let (today, outage) = today_with_an_outage();
    let facts = TapeFacts {
        copies: 1,
        outages: vec![outage],
    };
    let mut campaign = match Campaign::start(
        &today[..],
        &facts,
        HistoryReach::Sessions(1),
        bounds(),
        CAMPAIGN_PAGE_PRINTS,
    ) {
        CampaignStart::Run(campaign) => campaign,
        other => panic!("{other:?}"),
    };
    assert_eq!(
        campaign.advance(&session(19), true),
        CampaignStep::Ask,
        "the night before today is the first close, not the outage"
    );
    assert_eq!(
        campaign.advance(&session(18)[500..], true),
        CampaignStep::Stop(CampaignEnd::ReachMet)
    );
    let outcome = campaign.finish(CampaignEnd::ReachMet);
    assert_eq!(
        outcome.reached_open_ms,
        Some(session(19)[0].timestamp_ms),
        "yesterday's open, not today's"
    );
    assert_eq!(outcome.outages_crossed, 1);
    let sentence = outcome.sentence(|ms| format!("<{ms}>"));
    assert!(
        sentence.contains("feed outage"),
        "the note says a hole was crossed: {sentence}"
    );
}

/// The residual limit, pinned: a hole nobody marked reads as a close, so
/// the count is off by one session. Only outages the feed reported are known.
#[test]
fn an_unmarked_hole_inside_today_still_reads_as_a_close() {
    let (today, _) = today_with_an_outage();
    let mut campaign = running(&today, HistoryReach::Sessions(1));
    assert_eq!(
        campaign.advance(&session(19)[500..], true),
        CampaignStep::Stop(CampaignEnd::ReachMet)
    );
    assert_eq!(
        campaign.finish(CampaignEnd::ReachMet).reached_open_ms,
        Some(today[0].timestamp_ms)
    );
}

/// `trades` without the prints strictly between `from_minute` and
/// `to_minute` of `day`, and the outage that marks the hole.
fn holed(trades: Vec<Trade>, day: i64, from_minute: i64, to_minute: i64) -> (Vec<Trade>, Outage) {
    let kept = trades
        .into_iter()
        .filter(|trade| {
            let minute = (trade.timestamp_ms - day * DAY) / MINUTE;
            !(from_minute < minute && minute < to_minute)
        })
        .collect();
    let outage = Outage {
        from_ms: day * DAY + from_minute * MINUTE,
        to_ms: day * DAY + to_minute * MINUTE,
    };
    (kept, outage)
}

#[test]
fn an_hours_target_counts_no_outage_time_as_trading_and_says_it_crossed_one() {
    let today = session(20);
    // Yesterday from 14:30, with a two-hour outage the feed marked at 15:00.
    let (page, outage) = holed(session(19), 19, 15 * 60, 17 * 60);
    let page: Vec<Trade> = page
        .into_iter()
        .filter(|trade| trade.timestamp_ms >= 19 * DAY + 14 * HOUR + 30 * MINUTE)
        .collect();
    let facts = TapeFacts {
        copies: 1,
        outages: vec![outage],
    };
    let mut campaign = match Campaign::start(
        &today[..],
        &facts,
        HistoryReach::Hours(2),
        bounds(),
        CAMPAIGN_PAGE_PRINTS,
    ) {
        CampaignStart::Run(campaign) => campaign,
        other => panic!("{other:?}"),
    };
    assert_eq!(
        campaign.advance(&page, true),
        CampaignStep::Ask,
        "18:00 back to 14:30 is ninety traded minutes once the outage is left out"
    );
    assert_eq!(campaign.progress().traded_ms, 90 * MINUTE);
    let outcome = campaign.finish(CampaignEnd::Cancelled);
    assert_eq!(outcome.outages_crossed, 1);
    let sentence = outcome.sentence(|ms| format!("<{ms}>"));
    assert!(
        sentence.contains("feed outage"),
        "an hours note says an outage was crossed too: {sentence}"
    );
}

#[test]
fn overlapping_marked_outages_explain_a_silence_once() {
    // A four-hour hole inside today, of which the feed marked only the first
    // two hours — twice, and once more overlapping. The two unexplained hours
    // are longer than a close, so the hole still reads as one.
    let (today, outage) = holed(session(20), 20, 11 * 60, 15 * 60);
    let first_half = Outage {
        from_ms: outage.from_ms,
        to_ms: outage.from_ms + 2 * HOUR,
    };
    let overlapping = Outage {
        from_ms: outage.from_ms + 30 * MINUTE,
        to_ms: outage.from_ms + 90 * MINUTE,
    };
    let facts = TapeFacts {
        copies: 1,
        outages: vec![first_half, first_half, overlapping],
    };
    let mut campaign = match Campaign::start(
        &today[..],
        &facts,
        HistoryReach::Sessions(1),
        bounds(),
        CAMPAIGN_PAGE_PRINTS,
    ) {
        CampaignStart::Run(campaign) => campaign,
        other => panic!("{other:?}"),
    };
    assert_eq!(
        campaign.advance(&session(19)[500..], true),
        CampaignStep::Stop(CampaignEnd::ReachMet)
    );
    let outcome = campaign.finish(CampaignEnd::ReachMet);
    assert_eq!(
        outcome.reached_open_ms,
        Some(today[0].timestamp_ms),
        "the unexplained half of the hole is a close, as with no outage marked"
    );
    assert_eq!(outcome.outages_crossed, 0);
}

#[test]
fn the_page_budget_ends_a_run_that_keeps_making_progress() {
    let today = session(20);
    let mut campaign = running(&today, HistoryReach::Sessions(1));
    let mut oldest = today[0].timestamp_ms;
    let pages = HistoryReach::Sessions(1).page_budget(CAMPAIGN_PAGE_PRINTS);
    for page in 0..pages {
        assert_eq!(campaign.next_request(), CAMPAIGN_PAGE_PRINTS);
        oldest -= 1;
        let step = campaign.advance(&[trade(oldest)], true);
        if page + 1 < pages {
            assert_eq!(step, CampaignStep::Ask, "page {page}");
        } else {
            assert_eq!(step, CampaignStep::Stop(CampaignEnd::PagesSpent));
        }
    }
}

// --- what the trader is told ------------------------------------------------------

#[test]
fn every_ending_has_a_token_and_a_reason() {
    for end in CampaignEnd::ALL {
        assert_eq!(CampaignEnd::from_action(end.action()), Some(end));
        assert!(!end.reason().is_empty(), "{end:?} has no reason");
    }
    let mut tokens: Vec<_> = CampaignEnd::ALL.iter().map(|end| end.action()).collect();
    tokens.sort_unstable();
    tokens.dedup();
    assert_eq!(
        tokens.len(),
        CampaignEnd::ALL.len(),
        "two endings share a token"
    );
    assert_eq!(CampaignEnd::from_action("span_cap_covered"), None);
}

#[test]
fn a_finished_run_says_where_it_landed() {
    let today = session(20);
    let mut campaign = running(&today, HistoryReach::Sessions(1));
    campaign.advance(&session(19), true);
    campaign.advance(&session(18)[500..], true);
    let sentence = campaign
        .finish(CampaignEnd::ReachMet)
        .sentence(|ms| format!("<{ms}>"));
    let open = session(19)[0].timestamp_ms;
    assert_eq!(
        sentence,
        format!("Loaded back to <{open}> (1 session)"),
        "the open the trader asked for, not the stray prints past it"
    );
}

#[test]
fn a_partial_run_says_where_it_stopped_why_and_how_far_it_got() {
    let today = session(20);
    let mut campaign = running(&today, HistoryReach::Sessions(5));
    campaign.advance(&session(19), true);
    campaign.advance(&session(18), true);
    campaign.advance(&session(17), true);
    let outcome = campaign.finish(CampaignEnd::Cancelled);
    assert!(!outcome.complete());
    assert_eq!(
        outcome.sentence(|ms| format!("<{ms}>")),
        format!(
            "Stopped at <{}> \u{2014} cancelled (2 of 5 sessions)",
            session(17)[0].timestamp_ms
        )
    );
}

#[test]
fn an_hours_run_reports_traded_time() {
    let today = session(20);
    let mut campaign = running(&today, HistoryReach::Hours(4));
    let yesterday = session(19);
    campaign.advance(&yesterday[yesterday.len() - 91..], true);
    let outcome = campaign.finish(CampaignEnd::Exhausted);
    assert!(
        outcome
            .sentence(|ms| format!("<{ms}>"))
            .ends_with("(1 h 30 m of 4 h)"),
        "{}",
        outcome.sentence(|ms| format!("<{ms}>"))
    );
}

#[test]
fn a_run_already_there_says_so() {
    let held = sessions(10, 12);
    let CampaignStart::AlreadyMet(outcome) = Campaign::start(
        &held[..],
        &TapeFacts::default(),
        HistoryReach::Sessions(1),
        bounds(),
        1_000,
    ) else {
        panic!("met");
    };
    assert_eq!(
        outcome.sentence(|ms| format!("<{ms}>")),
        format!("Already loaded back to <{}>", held[0].timestamp_ms)
    );
}

/// The chart hands the start its chunked tape rather than a slice.
#[test]
fn the_start_reads_a_chunked_tape_as_it_reads_a_slice() {
    use quantick_engine::trade_tape::{CHUNK_TRADES, TradeTape};
    let mut tape = run(0, 1_000, CHUNK_TRADES + 500);
    let close = tape.last().expect("not empty").timestamp_ms;
    tape.extend(run(close + 14 * HOUR, 1_000, CHUNK_TRADES - 300));
    let chunked: TradeTape = tape.iter().cloned().collect();
    for reach in HistoryReach::PRESETS {
        let over_slice = Campaign::start(&tape[..], &TapeFacts::default(), reach, bounds(), 1_000);
        let over_tape = Campaign::start(&chunked, &TapeFacts::default(), reach, bounds(), 1_000);
        assert_eq!(over_slice, over_tape, "{reach:?}");
    }
}

/// What a press costs the frame when the chart already holds five dense
/// sessions and the target is further still: the start walks every held
/// print once. Run with `--release --ignored` to read the number.
#[test]
#[ignore = "manual press-cost measurement"]
fn the_press_walk_over_five_dense_sessions_costs_one_pass() {
    use quantick_engine::trade_tape::TradeTape;
    let per_session = MEASURED_DENSE_SESSION_PRINTS;
    let tape: TradeTape = (0..5_i64)
        .flat_map(|day| run(day * DAY + 9 * HOUR, 20, per_session))
        .collect();
    let started = std::time::Instant::now();
    let start = Campaign::start(
        &tape,
        &TapeFacts::default(),
        HistoryReach::Sessions(10),
        bounds(),
        CAMPAIGN_PAGE_PRINTS,
    );
    println!(
        "press walk over {} held prints: {:.3} ms",
        tape.len(),
        started.elapsed().as_secs_f64() * 1000.0
    );
    assert!(matches!(start, CampaignStart::Run(_)));
    let slice: Vec<Trade> = tape.iter().cloned().collect();
    let started = std::time::Instant::now();
    let _ = Campaign::start(
        &slice[..],
        &TapeFacts::default(),
        HistoryReach::Sessions(10),
        bounds(),
        CAMPAIGN_PAGE_PRINTS,
    );
    println!(
        "over a slice: {:.3} ms",
        started.elapsed().as_secs_f64() * 1000.0
    );
}
