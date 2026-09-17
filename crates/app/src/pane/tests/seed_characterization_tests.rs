use super::*;
use quantick_engine::{DealSample, trade_tape::TradeTape};

#[test]
fn empty_seed_keeps_readings_and_establishes_an_empty_backfill_boundary() {
    let mut pane = ChartPane::flow(1, BarSpec::Tick(2), "TESTUSDT".to_owned());
    let reading = DealSample {
        time_ms: TAPE_START_MS,
        session_deals: 10,
    };
    pane.seed_from(&TradeTape::new(), usize::MAX, &[reading]);
    assert_eq!(pane.state.deal_samples(), &[reading]);
    assert!(pane.state.trades().is_empty());
    assert_eq!(pane.state.backfill_trade_count(), 0);
    assert_eq!(pane.state.backfill_boundary(), Some(0));
    assert_eq!(
        (pane.state.timeline_revision(), pane.state.series_revision()),
        (1, 1)
    );
}

#[test]
fn seed_clamps_only_the_split_and_keeps_the_original_live_prints_live() {
    let mut tape = TradeTape::new();
    for id in 0..5 {
        tape.push(print_at(id));
    }
    for (split, expected_count, boundary, timeline) in
        [(3, 3, 1, 3), (usize::MAX, 5, 2, 1), (0, 0, 0, 6)]
    {
        let mut pane = ChartPane::flow(1, BarSpec::Tick(2), "TESTUSDT".to_owned());
        pane.seed_from(&tape, split, &[]);
        assert_eq!(pane.state.backfill_trade_count(), expected_count);
        assert_eq!(pane.state.backfill_boundary(), Some(boundary));
        assert_eq!(
            (pane.state.timeline_revision(), pane.state.series_revision()),
            (timeline, 1)
        );
        assert_eq!(
            pane.state
                .bars()
                .iter()
                .map(|bar| (bar.open_time, bar.close_time, bar.trade_count))
                .collect::<Vec<_>>(),
            [
                (TAPE_START_MS, TAPE_START_MS + 1_000, 2),
                (TAPE_START_MS + 2_000, TAPE_START_MS + 3_000, 2)
            ]
        );
        assert_eq!(
            pane.state.partial().unwrap().open_time,
            TAPE_START_MS + 4_000
        );
        assert_eq!(
            pane.state
                .trades()
                .iter()
                .map(|trade| trade.agg_id)
                .collect::<Vec<_>>(),
            [0, 1, 2, 3, 4]
        );
    }
}
