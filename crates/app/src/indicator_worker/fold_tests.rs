//! [`fold_parked`]: which parked indicator commands a newer one may absorb.

use super::*;
use quantick_engine::Side;
use rust_decimal::Decimal;

fn print(id: u64) -> Trade {
    Trade {
        agg_id: id,
        timestamp_ms: 1_000 + id as i64,
        price: Decimal::from(100),
        quantity: Decimal::ONE,
        side: Side::Buy,
    }
}

fn forming(ids: std::ops::RangeInclusive<u64>) -> Option<Bar> {
    let run: Vec<Trade> = ids.map(print).collect();
    let mut bar = Bar::opened_by(&run[0]);
    for trade in &run[1..] {
        bar.extend(trade);
    }
    Some(bar)
}

fn partial(bar: Option<Bar>, ids: impl IntoIterator<Item = u64>, rungs: usize) -> IndicatorCommand {
    IndicatorCommand::PartialUpdated {
        partial: bar,
        run: ids.into_iter().map(print).collect(),
        rungs,
    }
}

fn run_of(command: &IndicatorCommand) -> (Vec<u64>, Option<u64>, usize) {
    match command {
        IndicatorCommand::PartialUpdated {
            partial,
            run,
            rungs,
        } => (
            run.iter().map(|t| t.agg_id).collect(),
            partial.as_ref().map(|b| b.trade_count),
            *rungs,
        ),
        _ => panic!("not a forming-bar update"),
    }
}

#[test]
fn a_forming_update_that_extends_the_run_absorbs_the_next_one_in_order() {
    let mut older = partial(forming(1..=3), 1..=3, 8);
    assert!(fold_parked(&mut older, partial(forming(1..=5), 4..=5, 16)).is_none());
    assert_eq!(run_of(&older), (vec![1, 2, 3, 4, 5], Some(5), 16));
}

#[test]
fn a_forming_update_that_clears_the_run_replaces_the_one_before_it() {
    let mut older = partial(forming(1..=3), 1..=3, 8);
    assert!(fold_parked(&mut older, partial(None, [], 8)).is_none());
    assert_eq!(run_of(&older), (vec![], None, 8));
    let mut older = partial(forming(1..=3), 1..=3, 8);
    assert!(fold_parked(&mut older, partial(forming(1..=4), 4..=4, 0)).is_none());
    assert_eq!(run_of(&older), (vec![4], Some(4), 0));
}

#[test]
fn a_clearing_update_followed_by_an_extending_one_keeps_both() {
    // The worker would clear the run and then extend it from empty; one
    // command cannot say both, so the second keeps its own place.
    let mut older = partial(None, [], 8);
    let kept = fold_parked(&mut older, partial(forming(1..=2), 1..=2, 8));
    assert_eq!(run_of(&kept.expect("kept")), (vec![1, 2], Some(2), 8));
    assert_eq!(run_of(&older), (vec![], None, 8));
}

#[test]
fn inputs_fold_per_slot_and_replays_fold_into_the_newest_replay() {
    let set = |slot, value| IndicatorCommand::SetInputs {
        slot: SlotId(slot),
        values: vec![InputValue::Int(value)],
    };
    let mut older = set(1, 10);
    assert!(fold_parked(&mut older, set(1, 20)).is_none());
    assert!(
        matches!(&older, IndicatorCommand::SetInputs { values, .. } if values == &vec![InputValue::Int(20)])
    );
    assert!(
        fold_parked(&mut older, set(2, 30)).is_some(),
        "another slot"
    );

    let mut replay = IndicatorCommand::Backfilled(Vec::new());
    let newest = forming(1..=2);
    assert!(
        fold_parked(
            &mut replay,
            IndicatorCommand::Rebuild(Vec::new(), newest.clone())
        )
        .is_none()
    );
    assert!(matches!(&replay, IndicatorCommand::Rebuild(_, partial) if partial == &newest));
}

#[test]
fn closed_bars_and_lifecycle_commands_never_fold() {
    let bar = forming(1..=2).expect("a bar");
    let mut older = IndicatorCommand::BarClosed(bar.clone());
    assert!(fold_parked(&mut older, IndicatorCommand::BarClosed(bar.clone())).is_some());
    assert!(fold_parked(&mut older, partial(forming(3..=3), 3..=3, 8)).is_some());
    let mut forming_update = partial(forming(1..=2), 1..=2, 8);
    assert!(fold_parked(&mut forming_update, IndicatorCommand::BarClosed(bar)).is_some());
    assert!(fold_parked(&mut forming_update, IndicatorCommand::Remove(SlotId(1))).is_some());
}
