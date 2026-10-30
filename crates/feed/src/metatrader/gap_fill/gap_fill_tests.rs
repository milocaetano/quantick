use rust_decimal::Decimal;

use super::*;

fn at(ms: i64, id: u64) -> Trade {
    Trade {
        agg_id: id,
        timestamp_ms: ms,
        price: Decimal::ONE,
        quantity: Decimal::ONE,
        side: quantick_engine::Side::Buy,
    }
}

fn ids(trades: &[Trade]) -> Vec<u64> {
    trades.iter().map(|t| t.agg_id).collect()
}

fn seam(last_ms: i64, held_at_last: usize) -> ResumeSeam {
    ResumeSeam {
        last_ms,
        held_at_last,
        oldest_ms: 0,
    }
}

fn released(step: FillStep) -> Release {
    match step {
        FillStep::Release(release) => release,
        FillStep::Wait | FillStep::Ask { .. } => panic!("expected a release"),
    }
}

#[test]
fn a_block_reaching_the_seam_releases_at_once_skipping_exactly_the_held_prints() {
    // The chart holds two of the three prints at 100; the block re-sends all
    // three. One continuous load charts the third, so the refill must too.
    let block = vec![at(90, 1), at(100, 2), at(100, 3), at(100, 4), at(110, 5)];
    let mut fill = GapFill::begin(seam(100, 2), block, true);
    let release = released(fill.step());
    assert_eq!(ids(&release.chunks.concat()), [4, 5]);
    assert_eq!(release.gap, None);
    assert_eq!(release.report.state, BackfillState::Filled);
    assert_eq!(release.report.received, 5);
    assert!(!release.page_owed);
}

#[test]
fn slices_and_a_page_fill_the_hole_and_live_prints_wait_behind_them() {
    let mut fill = GapFill::begin(seam(100, 1), vec![at(500, 50), at(510, 51)], true);
    // Not covered: ask below the block.
    let FillStep::Ask {
        count,
        before_utc_ms,
    } = fill.step()
    else {
        panic!("the hole has to be asked for");
    };
    assert_eq!((count, before_utc_ms), (REFILL_PAGE_PRINTS, 500));
    fill.asked();
    assert!(!fill.hold(FeedEvent::Live(at(520, 52))));
    fill.opening(vec![at(300, 30), at(400, 40)]);
    assert!(matches!(fill.step(), FillStep::Wait), "the page is owed");
    assert!(!fill.hold(FeedEvent::Live(at(530, 53))));
    // The page overlaps the slices it was clamped below and the chart's seam.
    fill.page(
        vec![at(90, 9), at(100, 10), at(200, 20), at(300, 30)],
        false,
        Some(90),
    );
    let release = released(fill.step());
    assert_eq!(ids(&release.chunks.concat()), [20, 30, 40, 50, 51]);
    assert_eq!(release.gap, None);
    let held: Vec<_> = release
        .held
        .iter()
        .map(|event| match event {
            FeedEvent::Live(trade) => trade.agg_id,
            _ => panic!("only live prints were held"),
        })
        .collect();
    assert_eq!(held, [52, 53], "held in arrival order");
}

#[test]
fn a_refill_released_while_its_page_travels_says_the_page_is_owed() {
    let mut fill = GapFill::begin(seam(100, 1), vec![at(500, 50)], true);
    assert!(matches!(fill.step(), FillStep::Ask { .. }));
    fill.asked();
    fill.opening(vec![at(100, 10), at(200, 20)]);
    let release = released(fill.step());
    assert!(
        release.page_owed,
        "its reply has to be dropped when it lands"
    );
    assert_eq!(ids(&release.chunks.concat()), [20, 50]);
}

#[test]
fn a_session_that_cannot_page_leaves_the_rest_of_the_hole_marked() {
    let mut fill = GapFill::begin(seam(100, 1), vec![at(500, 50)], false);
    let release = released(fill.step());
    assert_eq!(
        release.gap,
        Some(FeedGap {
            from_ms: 100,
            to_ms: 500
        })
    );
    assert_eq!(
        release.report.state,
        BackfillState::Partial {
            unfilled_to_ms: 500,
            stop: BackfillStop::NoPaging
        }
    );
    assert_eq!(
        ids(&release.chunks.concat()),
        [50],
        "what did arrive is charted"
    );
}

#[test]
fn an_answer_with_no_search_behind_it_ends_the_refill_as_a_lost_session() {
    let mut fill = GapFill::begin(seam(100, 1), vec![at(500, 50)], true);
    fill.asked();
    fill.page(Vec::new(), false, None);
    let release = released(fill.step());
    assert!(matches!(
        release.report.state,
        BackfillState::Partial {
            stop: BackfillStop::SessionLost,
            ..
        }
    ));
}

#[test]
fn an_exhausted_source_and_idle_pages_both_stop_short() {
    let mut fill = GapFill::begin(seam(100, 1), vec![at(500, 50)], true);
    fill.asked();
    fill.page(vec![at(400, 40)], true, Some(400));
    let release = released(fill.step());
    assert_eq!(
        release.report.state,
        BackfillState::Partial {
            unfilled_to_ms: 400,
            stop: BackfillStop::SourceExhausted
        }
    );

    let mut fill = GapFill::begin(seam(100, 1), vec![at(500, 50)], true);
    for reached in [450, 400, 350] {
        let FillStep::Ask { before_utc_ms, .. } = fill.step() else {
            panic!("an empty page that searched keeps the refill going");
        };
        assert!(before_utc_ms > reached, "the cursor follows the search");
        fill.asked();
        fill.page(Vec::new(), false, Some(reached));
    }
    assert!(matches!(
        released(fill.step()).report.state,
        BackfillState::Partial {
            stop: BackfillStop::NothingFound,
            ..
        }
    ));
}

#[test]
fn a_hole_deeper_than_the_reach_stops_at_the_reach() {
    let hour = 60 * 60 * 1_000;
    let mut fill = GapFill::begin(seam(-1_000 * hour, 1), vec![at(100 * hour, 1)], true);
    // One print per day, each across a close, walking back.
    for day in 1..=6 {
        fill.asked();
        fill.page(
            vec![at(100 * hour - day * 24 * hour, day as u64 + 1)],
            false,
            None,
        );
    }
    assert!(matches!(
        released(fill.step()).report.state,
        BackfillState::Partial {
            stop: BackfillStop::Reach,
            ..
        }
    ));
}

#[test]
fn the_hold_reports_its_bound() {
    let mut fill = GapFill::begin(seam(100, 1), vec![at(500, 50)], true);
    for _ in 0..MAX_HELD_EVENTS - 1 {
        assert!(!fill.hold(FeedEvent::Reset));
    }
    assert!(fill.hold(FeedEvent::Reset));
    let release = fill.stop_with(BackfillStop::HeldTooLong);
    assert_eq!(release.held.len(), MAX_HELD_EVENTS);
    assert!(release.gap.is_some());
}

#[test]
fn a_block_that_reaches_the_seam_is_no_hole_and_opens_no_refill() {
    // Finding: every reconnect opened a refill, so a session that merely
    // blinked said "filling in prints missed" and reported a refill whose
    // end lay before its start.
    let block = vec![at(90, 1), at(100, 2), at(100, 3), at(100, 4), at(110, 5)];
    let Ok(tail) = closed_hole(block, seam(100, 2)) else {
        panic!("a block reaching back past the seam leaves no hole");
    };
    assert_eq!(ids(&tail), [4, 5], "exactly the held prints are skipped");
    // Starting after the seam, the block is handed back whole for a refill.
    let Err(block) = closed_hole(vec![at(101, 6), at(120, 7)], seam(100, 2)) else {
        panic!("a block after the seam leaves a hole");
    };
    assert_eq!(ids(&block), [6, 7]);
}

#[test]
fn a_release_keeps_the_chunks_it_received_oldest_first() {
    let mut fill = GapFill::begin(seam(100, 1), vec![at(500, 50), at(510, 51)], true);
    fill.opening(vec![at(300, 30), at(400, 40)]);
    fill.asked();
    fill.page(vec![at(100, 10), at(200, 20)], false, Some(100));
    let release = released(fill.step());
    let chunks: Vec<Vec<u64>> = release.chunks.iter().map(|chunk| ids(chunk)).collect();
    assert_eq!(chunks, [vec![20], vec![30, 40], vec![50, 51]]);
}
