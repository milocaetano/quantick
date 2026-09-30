use super::*;
use crate::state::opening;
use crate::state::{BarSpec, ChartState};
use quantick_engine::Side;
fn trade(id: u64, time: i64, price: i64, quantity: i64) -> Trade {
    Trade {
        agg_id: id,
        timestamp_ms: time,
        price: Decimal::from(price),
        quantity: Decimal::from(quantity),
        side: Side::Buy,
    }
}
fn volume(ladder: &BarFootprint) -> Decimal {
    ladder.levels().values().map(|level| level.volume()).sum()
}
fn state(spec: BarSpec) -> ChartState {
    let mut state = ChartState::new(spec);
    state.set_footprint_group(Decimal::ONE);
    state.set_footprint_enabled(true);
    state
}
#[test]
fn opening_contributions_follow_tick_volume_and_time_closing_membership() {
    let prints = [
        trade(1, 1050, 100, 10),
        trade(2, 1099, 105, 2),
        trade(3, 1100, 100, 4),
        trade(4, 1200, 110, 8),
    ];
    for spec in [
        BarSpec::Tick(2),
        BarSpec::Volume(Decimal::from(12)),
        BarSpec::Time(100),
    ] {
        let mut state = state(spec);
        state.ingest_backfill(&prints);
        assert_eq!(opening::recorded_windows(&state), [1000]);
        assert_eq!(opening::closed(&state).len(), 1);
        assert_eq!(volume(&opening::closed(&state)[&0]), Decimal::from(12));
        assert_eq!(opening::closed(&state)[&0], state.bar_footprints()[0]);
        assert!(opening::partial(&state).is_none());
    }
}
#[test]
fn live_partial_and_close_without_never_shift_opening_ownership() {
    let first = trade(1, 1050, 100, 10);
    let next = trade(2, 1200, 105, 2);
    let mut series = FootprintSeries::new(Decimal::ONE);
    series.observe(&first, None);
    assert_eq!(volume(series.opening_partial().unwrap()), Decimal::from(10));
    series.close_without(&Bar::opened_by(&first));
    assert_eq!(volume(&series.opening_closed()[&0]), Decimal::from(10));
    series.observe(&next, None);
    assert!(series.opening_partial().is_none());
    assert_eq!(series.closed().len(), 1);
}
#[test]
fn prepend_refold_spec_change_and_source_reset_reclassify_opening_exactly() {
    let mut state = state(BarSpec::Tick(1));
    state.ingest_live(&trade(2, 1050, 100, 10));
    assert_eq!(volume(&opening::closed(&state)[&0]), Decimal::from(10));
    state.prepend_history(&[trade(1, 250, 95, 100)]);
    assert_eq!(opening::recorded_windows(&state), [200]);
    assert_eq!(opening::closed(&state).len(), 1);
    assert_eq!(volume(&opening::closed(&state)[&0]), Decimal::from(100));
    state.set_footprint_group(Decimal::from(5));
    assert_eq!(opening::closed(&state)[&0].group(), Decimal::from(5));
    state.set_spec(BarSpec::Tick(2));
    assert_eq!(volume(&opening::closed(&state)[&0]), Decimal::from(100));
    assert_eq!(volume(&state.bar_footprints()[0]), Decimal::from(110));
    state.reset_series(BarSpec::Tick(1));
    state.set_footprint_enabled(true);
    state.ingest_backfill(&[trade(1, 86_401_050, 110, 2)]);
    assert_eq!(opening::recorded_windows(&state), [86_401_000]);
    assert_eq!(volume(&opening::closed(&state)[&0]), Decimal::from(2));
}
#[test]
fn opening_metadata_is_bounded_while_exact_sparse_contributions_survive() {
    let mut state = state(BarSpec::Tick(1));
    for day in 0..12 {
        state.ingest_live(&trade(day as u64, day * 86_400_000 + 1050, 100, 1));
    }
    assert_eq!(opening::recorded_windows(&state).len(), 8);
    assert_eq!(opening::closed(&state).len(), 12);
    assert_eq!(volume(&opening::closed(&state)[&0]), Decimal::ONE);
}

#[test]
fn earlier_live_window_retracts_closed_contributions_without_reordering_bars() {
    let mut state = state(BarSpec::Tick(1));
    for print in [
        trade(1, 1050, 100, 10000),
        trade(2, 950, 105, 100),
        trade(3, 1150, 110, 400),
        trade(4, 990, 115, 50),
    ] {
        state.ingest_live(&print);
    }
    assert_eq!(opening::recorded_windows(&state), [900]);
    assert_eq!(
        opening::closed(&state).keys().copied().collect::<Vec<_>>(),
        [1, 3]
    );
    assert_eq!(volume(&opening::closed(&state)[&1]), Decimal::from(100));
    assert_eq!(volume(&opening::closed(&state)[&3]), Decimal::from(50));
    let bars = state.bars().to_vec();
    assert_eq!(
        bars.iter().map(|bar| bar.open).collect::<Vec<_>>(),
        [100, 105, 110, 115].map(Decimal::from)
    );
    assert_eq!(
        state
            .bar_footprints()
            .iter()
            .map(volume)
            .collect::<Vec<_>>(),
        [10000, 100, 400, 50].map(Decimal::from)
    );
    state.set_footprint_group(Decimal::from(5));
    assert_eq!(state.bars(), bars);
    assert_eq!(
        opening::closed(&state).keys().copied().collect::<Vec<_>>(),
        [1, 3]
    );
    state.set_spec(BarSpec::Tick(2));
    assert_eq!(volume(&opening::closed(&state)[&0]), Decimal::from(100));
    assert_eq!(volume(&opening::closed(&state)[&1]), Decimal::from(50));
}

#[test]
fn earlier_windows_retract_partial_and_mixed_date_closed_contributions() {
    const DAY: i64 = 86_400_000;
    let mut state = state(BarSpec::Tick(4));
    state.ingest_live(&trade(1, 1050, 100, 10000));
    state.ingest_live(&trade(2, DAY + 1050, 105, 2000));
    state.ingest_live(&trade(3, 950, 110, 100));
    assert_eq!(
        volume(opening::partial(&state).unwrap()),
        Decimal::from(2100)
    );
    state.ingest_live(&trade(4, 990, 115, 50));
    assert_eq!(volume(&opening::closed(&state)[&0]), Decimal::from(2150));
    let bars = state.bars().to_vec();
    let ladders = state.bar_footprints().to_vec();
    state.ingest_live(&trade(5, DAY + 950, 120, 200));
    assert_eq!(volume(&opening::closed(&state)[&0]), Decimal::from(150));
    assert_eq!(
        opening::closed(&state)[&0]
            .levels()
            .values()
            .map(|row| row.trade_count)
            .sum::<u64>(),
        2
    );
    assert_eq!(
        volume(opening::partial(&state).unwrap()),
        Decimal::from(200)
    );
    assert_eq!(state.bars(), bars);
    assert_eq!(state.bar_footprints(), ladders);
    assert_eq!(volume(&ladders[0]), Decimal::from(12150));
}

#[test]
fn old_date_provenance_survives_readback_window_retention() {
    let mut state = state(BarSpec::Tick(1));
    for day in 0..12 {
        state.ingest_live(&trade(day as u64, day * 86_400_000 + 1050, 100, 10000));
    }
    state.ingest_live(&trade(20, 950, 105, 100));
    assert_eq!(opening::recorded_windows(&state).len(), 8);
    assert!(!opening::closed(&state).contains_key(&0));
    assert_eq!(volume(&opening::closed(&state)[&12]), Decimal::from(100));
    state.ingest_live(&trade(21, 1055, 110, 500));
    assert!(!opening::closed(&state).contains_key(&13));
    assert_eq!(volume(&state.bar_footprints()[0]), Decimal::from(10000));
}

#[test]
fn earlier_live_window_retracts_a_close_without_candidate_and_resets_with_series() {
    let first = trade(1, 1050, 100, 10000);
    let earlier = trade(2, 950, 105, 100);
    let mut series = FootprintSeries::new(Decimal::ONE);
    series.observe(&first, None);
    series.close_without(&Bar::opened_by(&first));
    series.observe(&earlier, None);
    assert!(series.opening_closed().is_empty());
    assert_eq!(
        volume(series.opening_partial().unwrap()),
        Decimal::from(100)
    );
    assert_eq!(volume(&series.closed()[0]), Decimal::from(10000));
    series.reset(Decimal::ONE);
    series.observe(&first, None);
    assert_eq!(series.recorded_openings(), [1000]);
    assert_eq!(
        volume(series.opening_partial().unwrap()),
        Decimal::from(10000)
    );
}
