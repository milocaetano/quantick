use quantick_anchored_studies::{FrvpEmpty, ProfileInputs, ProfileRequest, RangeProfile};
use quantick_engine::{Bar, BarFootprint, FootprintBuilder, Side, Trade};
use rust_decimal::Decimal;

fn trade(time: i64, price: i64, quantity: i64) -> Trade {
    Trade {
        agg_id: 1,
        timestamp_ms: time,
        price: Decimal::from(price),
        quantity: Decimal::from(quantity),
        side: Side::Buy,
    }
}

fn ladder(group: i64, trades: &[Trade]) -> BarFootprint {
    let mut builder = FootprintBuilder::new(Decimal::from(group), 100);
    for trade in trades {
        builder.push(trade);
    }
    builder.close_or_empty()
}

fn inputs<'a>(closed: &'a [Bar], ladders: &'a [BarFootprint]) -> ProfileInputs<'a> {
    ProfileInputs {
        closed,
        ladders,
        prefix: &[],
        partial_ladder: None,
        group: Decimal::ONE,
        series_revision: 1,
        partial_version: 1,
        blocked: false,
        side_inferred: false,
        partial_bucket_slot: None,
        budget: 100,
    }
}

fn request() -> ProfileRequest {
    ProfileRequest {
        min_bar: 0.0,
        max_bar: 100.0,
        extend_right: false,
        approximate_history: false,
        value_area_pct: 70,
    }
}

fn volume(cache: &Option<RangeProfile>) -> Decimal {
    cache
        .as_ref()
        .unwrap()
        .output()
        .profile
        .unwrap()
        .0
        .total_volume()
}

#[test]
fn a_closed_ladder_with_incompatible_group_is_excluded_from_coverage() {
    let print = trade(1, 100, 5);
    let bars = [Bar::opened_by(&print)];
    let ladders = [ladder(3, &[print])];
    let mut source = inputs(&bars, &ladders);
    source.group = Decimal::from(2);
    let mut cache = None;
    assert!(!RangeProfile::refresh(&mut cache, request(), &source));
    let output = cache.as_ref().unwrap().output();
    assert_eq!(output.bars_total, 1);
    assert_eq!(output.bars_folded, 1);
    assert_eq!(output.closed_covered, 0);
    assert_eq!(output.bars_covered, 0);
    assert!(output.profile.is_none());
    assert_eq!(output.empty, Some(FrvpEmpty::NoTape));
}

#[test]
fn an_incompatible_live_snapshot_does_not_claim_coverage_or_add_volume() {
    let print = trade(1, 100, 5);
    let bars = [Bar::opened_by(&print)];
    let ladders = [ladder(2, &[print])];
    let partial = ladder(3, &[trade(2, 102, 7)]);
    let mut source = inputs(&bars, &ladders);
    source.group = Decimal::from(2);
    source.partial_ladder = Some(&partial);
    let mut cache = None;
    assert!(!RangeProfile::refresh(&mut cache, request(), &source));
    let output = cache.as_ref().unwrap().output();
    assert!(output.key.include_partial);
    assert_eq!(output.bars_total, 2);
    assert_eq!(output.bars_covered, 1);
    assert_eq!(output.closed_covered, 1);
    assert_eq!(volume(&cache), Decimal::from(5));
    let corrected = ladder(2, &[trade(2, 102, 7)]);
    source.partial_ladder = Some(&corrected);
    source.partial_version += 1;
    assert!(!RangeProfile::refresh(&mut cache, request(), &source));
    assert_eq!(cache.as_ref().unwrap().output().bars_covered, 2);
    assert_eq!(volume(&cache), Decimal::from(12));
}

#[test]
fn an_empty_answered_ladder_counts_as_covered_but_creates_no_volume() {
    let bars = [Bar::opened_by(&trade(1, 100, 0))];
    let ladders = [ladder(1, &[])];
    let mut cache = None;
    assert!(!RangeProfile::refresh(
        &mut cache,
        request(),
        &inputs(&bars, &ladders)
    ));
    let output = cache.as_ref().unwrap().output();
    assert_eq!(output.closed_covered, 1);
    assert_eq!(output.bars_covered, 1);
    assert_eq!(output.bars_folded, 1);
    let (profile, area) = output.profile.unwrap();
    assert_eq!(profile.total_volume(), Decimal::ZERO);
    assert!(area.is_none());
    assert!(
        output.empty.is_none(),
        "an empty answered ladder is still evidence"
    );
}

#[test]
fn an_absent_ladder_is_missing_evidence_even_when_the_bar_reports_volume() {
    let bars = [Bar::opened_by(&trade(1, 100, 50))];
    let mut cache = None;
    assert!(!RangeProfile::refresh(
        &mut cache,
        request(),
        &inputs(&bars, &[])
    ));
    let output = cache.as_ref().unwrap().output();
    assert_eq!(output.bars_total, 1);
    assert_eq!(output.bars_folded, 1);
    assert_eq!(output.bars_covered, 0);
    assert_eq!(output.bars_approximated, 0);
    assert!(
        output.profile.is_none(),
        "closed tape bars are not silently approximated"
    );
}

#[test]
fn a_whole_ladder_can_overshoot_the_budget_but_the_next_bar_waits() {
    let prints = [trade(1, 100, 1), trade(2, 101, 2), trade(3, 102, 3)];
    let mut first = Bar::opened_by(&prints[0]);
    first.extend(&prints[1]);
    first.extend(&prints[2]);
    let second = trade(4, 103, 4);
    let bars = [first, Bar::opened_by(&second)];
    let ladders = [ladder(1, &prints), ladder(1, &[second])];
    let mut source = inputs(&bars, &ladders);
    source.budget = 1;
    let mut cache = None;
    assert!(RangeProfile::refresh(&mut cache, request(), &source));
    let output = cache.as_ref().unwrap().output();
    assert_eq!(output.bars_folded, 1);
    assert_eq!(output.closed_covered, 1);
    assert_eq!(volume(&cache), Decimal::from(6));
    assert!(!RangeProfile::refresh(&mut cache, request(), &source));
    assert_eq!(cache.as_ref().unwrap().output().bars_folded, 2);
    assert_eq!(volume(&cache), Decimal::from(10));
}

#[test]
fn a_zero_budget_defers_closed_work_until_a_later_pass() {
    let print = trade(1, 100, 5);
    let bars = [Bar::opened_by(&print)];
    let ladders = [ladder(1, &[print])];
    let mut source = inputs(&bars, &ladders);
    source.budget = 0;
    let mut cache = None;
    assert!(RangeProfile::refresh(&mut cache, request(), &source));
    let deferred = cache.as_ref().unwrap().output();
    assert_eq!(deferred.bars_folded, 0);
    assert_eq!(deferred.bars_covered, 0);
    assert!(deferred.profile.is_none());
    assert!(
        deferred.empty.is_none(),
        "unprocessed is different from no tape"
    );
    source.budget = 1;
    assert!(!RangeProfile::refresh(&mut cache, request(), &source));
    assert_eq!(volume(&cache), Decimal::from(5));
}

#[test]
fn a_block_revokes_a_partial_result_and_unblocking_rebuilds_from_evidence() {
    let prints = [trade(1, 100, 2), trade(2, 101, 3)];
    let bars = prints.each_ref().map(Bar::opened_by);
    let ladders = prints
        .each_ref()
        .map(|print| ladder(1, std::slice::from_ref(print)));
    let mut source = inputs(&bars, &ladders);
    source.budget = 1;
    let mut cache = None;
    assert!(RangeProfile::refresh(&mut cache, request(), &source));
    assert_eq!(volume(&cache), Decimal::from(2));
    source.blocked = true;
    assert!(!RangeProfile::refresh(&mut cache, request(), &source));
    let blocked = cache.as_ref().unwrap().output();
    assert_eq!(blocked.empty, Some(FrvpEmpty::Blocked));
    assert_eq!(blocked.bars_total, 2);
    assert_eq!(blocked.bars_covered, 0);
    assert_eq!(blocked.bars_folded, 0);
    assert!(blocked.profile.is_none());
    source.blocked = false;
    assert!(RangeProfile::refresh(&mut cache, request(), &source));
    assert_eq!(volume(&cache), Decimal::from(2));
    assert!(!RangeProfile::refresh(&mut cache, request(), &source));
    assert_eq!(volume(&cache), Decimal::from(5));
}

#[test]
fn a_series_correction_mid_fold_discards_all_prior_accumulation() {
    let prints = [trade(1, 100, 2), trade(2, 101, 3)];
    let bars = prints.each_ref().map(Bar::opened_by);
    let original = prints
        .each_ref()
        .map(|print| ladder(1, std::slice::from_ref(print)));
    let corrected = [
        ladder(1, &[trade(1, 100, 20)]),
        ladder(1, &[trade(2, 101, 30)]),
    ];
    let mut source = inputs(&bars, &original);
    source.budget = 1;
    let mut cache = None;
    assert!(RangeProfile::refresh(&mut cache, request(), &source));
    source.ladders = &corrected;
    source.series_revision += 1;
    assert!(RangeProfile::refresh(&mut cache, request(), &source));
    assert_eq!(cache.as_ref().unwrap().output().bars_folded, 1);
    assert_eq!(volume(&cache), Decimal::from(20));
    assert!(!RangeProfile::refresh(&mut cache, request(), &source));
    assert_eq!(volume(&cache), Decimal::from(50));
}

#[test]
fn an_empty_live_ladder_counts_once_when_combined_with_closed_tape() {
    let print = trade(1, 100, 5);
    let bars = [Bar::opened_by(&print)];
    let ladders = [ladder(1, &[print])];
    let partial = ladder(1, &[]);
    let mut source = inputs(&bars, &ladders);
    source.partial_ladder = Some(&partial);
    let mut cache = None;
    assert!(!RangeProfile::refresh(&mut cache, request(), &source));
    assert_eq!(cache.as_ref().unwrap().output().bars_covered, 2);
    assert_eq!(volume(&cache), Decimal::from(5));
    source.partial_version += 1;
    assert!(!RangeProfile::refresh(&mut cache, request(), &source));
    let output = cache.as_ref().unwrap().output();
    assert_eq!(output.bars_covered, 2);
    assert_eq!(output.closed_covered, 1);
    assert_eq!(volume(&cache), Decimal::from(5));
}

#[test]
fn removing_the_live_ladder_removes_its_volume_and_coverage() {
    let print = trade(1, 100, 5);
    let bars = [Bar::opened_by(&print)];
    let ladders = [ladder(1, &[print])];
    let partial = ladder(1, &[trade(2, 101, 7)]);
    let mut source = inputs(&bars, &ladders);
    source.partial_ladder = Some(&partial);
    let mut cache = None;
    RangeProfile::refresh(&mut cache, request(), &source);
    assert_eq!(volume(&cache), Decimal::from(12));
    source.partial_ladder = None;
    assert!(!RangeProfile::refresh(&mut cache, request(), &source));
    let output = cache.as_ref().unwrap().output();
    assert!(!output.key.include_partial);
    assert_eq!(output.key.partial_snapshot, 0);
    assert_eq!(output.bars_total, 1);
    assert_eq!(output.bars_covered, 1);
    assert_eq!(volume(&cache), Decimal::from(5));
}

#[test]
fn a_partial_outside_the_range_cannot_revise_a_completed_profile() {
    let print = trade(1, 100, 5);
    let bars = [Bar::opened_by(&print)];
    let ladders = [ladder(1, &[print])];
    let partial = ladder(1, &[trade(2, 101, 7)]);
    let mut source = inputs(&bars, &ladders);
    source.partial_ladder = Some(&partial);
    let mut cache = None;
    let fixed = || ProfileRequest {
        max_bar: 0.0,
        ..request()
    };
    assert!(!RangeProfile::refresh(&mut cache, fixed(), &source));
    let key = cache.as_ref().unwrap().output().key;
    source.partial_version += 1;
    assert!(!RangeProfile::refresh(&mut cache, fixed(), &source));
    assert_eq!(cache.as_ref().unwrap().output().key, key);
    assert_eq!(volume(&cache), Decimal::from(5));
    assert_eq!(cache.as_ref().unwrap().output().bars_total, 1);
}

#[test]
fn side_inference_change_restamps_the_result_without_altering_measured_volume() {
    let print = trade(1, 100, 5);
    let bars = [Bar::opened_by(&print)];
    let ladders = [ladder(1, &[print])];
    let mut source = inputs(&bars, &ladders);
    let mut cache = None;
    RangeProfile::refresh(&mut cache, request(), &source);
    assert!(!cache.as_ref().unwrap().output().key.side_inferred);
    source.side_inferred = true;
    assert!(!RangeProfile::refresh(&mut cache, request(), &source));
    assert!(cache.as_ref().unwrap().output().key.side_inferred);
    assert_eq!(volume(&cache), Decimal::from(5));
    source.side_inferred = false;
    RangeProfile::refresh(&mut cache, request(), &source);
    assert!(!cache.as_ref().unwrap().output().key.side_inferred);
}

#[test]
fn zero_volume_prefix_candles_are_processed_without_claiming_approximation() {
    let prefix = [
        Bar::opened_by(&trade(1, 100, 0)),
        Bar::opened_by(&trade(2, 101, 5)),
    ];
    let mut source = inputs(&[], &[]);
    source.prefix = &prefix;
    source.budget = 1;
    let approximate = || ProfileRequest {
        approximate_history: true,
        ..request()
    };
    let mut cache = None;
    assert!(RangeProfile::refresh(&mut cache, approximate(), &source));
    let first = cache.as_ref().unwrap().output();
    assert_eq!(first.bars_folded, 1);
    assert_eq!(first.bars_approximated, 0);
    assert!(first.profile.is_none());
    assert!(!RangeProfile::refresh(&mut cache, approximate(), &source));
    let complete = cache.as_ref().unwrap().output();
    assert_eq!(complete.bars_folded, 2);
    assert_eq!(complete.bars_approximated, 1);
    assert_eq!(complete.bars_covered, 0);
    assert_eq!(volume(&cache), Decimal::from(5));
}
