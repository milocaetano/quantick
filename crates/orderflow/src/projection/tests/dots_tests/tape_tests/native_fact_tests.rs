//! Borrowed native facts obey the former normalized-clone equality contract.

use super::{
    AggressionPrimitive, Group, TapeDotMemory, TapeDotView, TapeReference, merge_groups,
    native_facts, normalized_native_fact, reference_quantities, same_native_fact,
};
use crate::config::HeatmapConfig;
use crate::config::theme::OrderflowRenderStyle;
use crate::history::RestingSide;
use crate::projection::{DotSizing, PriceWindow, TapeDotGeometry};
use quantick_engine::Side;
use rust_decimal::Decimal;
use std::collections::BTreeMap;
use std::hint::black_box;
use std::time::Instant;

fn native() -> AggressionPrimitive {
    AggressionPrimitive {
        agg_id: 1,
        agg_ids: vec![1, 2],
        generation: Some(7),
        side: Side::Buy,
        consumed_side: RestingSide::Ask,
        quantity: Decimal::from(4),
        buy_share: 0.75,
        live: true,
        price_bucket: Decimal::from(100),
        price_span: Decimal::ONE,
        price: Decimal::from(100),
        trade_count: 2,
        first_timestamp_ms: 1_001,
        last_timestamp_ms: 1_017,
        timestamp_quantity: Decimal::from(4_020),
        matched_quantity: Decimal::ONE,
        buy_quantity: Decimal::from(3),
        matched_fraction: 0.25,
        liquidity_event_ids: vec![9, 10],
        x: 0.25,
        y: 0.75,
        size: 0.5,
        folded_marks: 0,
    }
}

fn normalized(mut mark: AggressionPrimitive) -> AggressionPrimitive {
    mark.x = 0.0;
    mark.y = 0.0;
    mark.size = 0.0;
    mark
}

#[test]
fn every_factual_field_matches_the_normalized_clone_equality_oracle() {
    type Change = (&'static str, fn(&mut AggressionPrimitive));
    let changes: [Change; 20] = [
        ("agg_id", |mark| mark.agg_id += 1),
        ("agg_ids", |mark| mark.agg_ids.reverse()),
        ("generation", |mark| mark.generation = None),
        ("side", |mark| mark.side = Side::Sell),
        ("consumed_side", |mark| {
            mark.consumed_side = RestingSide::Bid
        }),
        ("quantity", |mark| mark.quantity += Decimal::ONE),
        ("buy_share", |mark| mark.buy_share = 0.5),
        ("live", |mark| mark.live = false),
        ("price_bucket", |mark| mark.price_bucket += Decimal::ONE),
        ("price_span", |mark| mark.price_span += Decimal::ONE),
        ("price", |mark| mark.price += Decimal::ONE),
        ("trade_count", |mark| mark.trade_count += 1),
        ("first_timestamp_ms", |mark| mark.first_timestamp_ms += 1),
        ("last_timestamp_ms", |mark| mark.last_timestamp_ms += 1),
        ("timestamp_quantity", |mark| {
            mark.timestamp_quantity += Decimal::ONE
        }),
        ("matched_quantity", |mark| {
            mark.matched_quantity += Decimal::ONE
        }),
        ("buy_quantity", |mark| mark.buy_quantity += Decimal::ONE),
        ("matched_fraction", |mark| mark.matched_fraction = 0.5),
        ("liquidity_event_ids", |mark| {
            mark.liquidity_event_ids.reverse()
        }),
        ("folded_marks", |mark| mark.folded_marks = 2),
    ];
    let original = native();
    assert!(same_native_fact(&original, &original));
    for (field, change) in changes {
        let mut changed = original.clone();
        change(&mut changed);
        let expected = normalized(original.clone()) == normalized(changed.clone());
        assert!(!expected, "the fixture actually changes {field}");
        assert_eq!(same_native_fact(&original, &changed), expected, "{field}");
        assert_eq!(
            same_native_fact(&changed, &original),
            expected,
            "symmetric {field}"
        );
    }
}

#[test]
fn only_projection_coordinates_are_ignored_including_nonfinite_values() {
    let original = native();
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.0, 100.0] {
        let mut changed = original.clone();
        changed.x = value;
        changed.y = value;
        changed.size = value as f32;
        assert_eq!(normalized(original.clone()), normalized(changed.clone()));
        assert!(same_native_fact(&original, &changed));
        assert!(same_native_fact(&changed, &original));
    }
    for share in [true, false] {
        let mut left = original.clone();
        let mut right = original.clone();
        if share {
            left.buy_share = f32::NAN;
            right.buy_share = f32::NAN;
        } else {
            left.matched_fraction = f32::NAN;
            right.matched_fraction = f32::NAN;
        }
        let expected = normalized(left.clone()) == normalized(right.clone());
        assert!(
            !expected,
            "factual float NaNs retain ordinary PartialEq semantics"
        );
        assert_eq!(same_native_fact(&left, &right), expected);
    }
    let mut positive = original.clone();
    positive.buy_share = 0.0;
    positive.matched_fraction = 0.0;
    let mut negative = positive.clone();
    negative.buy_share = -0.0;
    negative.matched_fraction = -0.0;
    assert_eq!(normalized(positive.clone()), normalized(negative.clone()));
    assert!(same_native_fact(&positive, &negative));
}

#[test]
fn retained_native_provenance_changes_refresh_without_changing_execution_geometry() {
    let mut config = HeatmapConfig::default();
    config.live_lane.tape_only = true;
    config.bubbles.max_radius = 15.0;
    let view = TapeDotView {
        now_ms: 5_000,
        window_ms: 30_000,
        dot_window_ms: 100,
        evicted_through_ms: None,
        prices: PriceWindow::new(Decimal::from(90), Decimal::from(150)).unwrap(),
        geometry: TapeDotGeometry {
            left_x: 0.0,
            right_x: 1.0,
            width_px: 640.0,
            height_px: 400.0,
        },
    };
    let sizing = DotSizing {
        native_tape: config.native_tape(),
        tape_column_px: 1.0,
        candle_column_px: 1.0,
        px_per_price: 1.0,
        typed_full: None,
    };
    let draw = |memory: &mut TapeDotMemory, mark: &AggressionPrimitive| {
        memory.project(
            std::slice::from_ref(mark),
            view,
            sizing,
            &config.bubbles,
            &config.live_lane,
            &[],
        )
    };
    let mut memory = TapeDotMemory::default();
    let mut mark = native();
    let before = draw(&mut memory, &mark);
    mark.generation = Some(8);
    mark.matched_quantity = Decimal::TWO;
    mark.matched_fraction = 0.5;
    mark.liquidity_event_ids = vec![99];
    let adopted = draw(&mut memory, &mark);
    let rebuilt = draw(&mut TapeDotMemory::default(), &mark);
    assert_eq!(
        adopted.marks, rebuilt.marks,
        "a receipt-only update is retained exactly"
    );
    assert_eq!(adopted.max_radius, rebuilt.max_radius);
    assert_eq!(adopted.full_quantity, rebuilt.full_quantity);
    let (old, updated) = (&before.marks[0], &adopted.marks[0]);
    assert_eq!(
        (
            updated.generation,
            updated.matched_quantity,
            updated.matched_fraction
        ),
        (Some(8), Decimal::TWO, 0.5)
    );
    assert_eq!(updated.liquidity_event_ids, [99]);
    assert_eq!(
        (
            updated.x,
            updated.y,
            updated.quantity,
            updated.buy_quantity,
            updated.timestamp_quantity
        ),
        (
            old.x,
            old.y,
            old.quantity,
            old.buy_quantity,
            old.timestamp_quantity
        )
    );
    mark.x = f64::NAN;
    mark.y = f64::INFINITY;
    mark.size = f32::NAN;
    let reprojected = draw(&mut memory, &mark);
    assert_eq!(reprojected.marks, adopted.marks);
}

/// Setup and result destruction are outside the timed interval. Index
/// construction includes its node allocation; replacement includes removal
/// and node release. This measures CPU stages, not an allocation count.
fn measure_stage<Input, Output>(
    label: &str,
    mut setup: impl FnMut() -> Input,
    mut run: impl FnMut(Input) -> Output,
) -> Output {
    const SAMPLES: usize = 30;
    for _ in 0..3 {
        black_box(run(setup()));
    }
    let mut times = Vec::with_capacity(SAMPLES);
    let mut last = None;
    for _ in 0..SAMPLES {
        let input = setup();
        let started = Instant::now();
        let output = black_box(run(input));
        times.push(started.elapsed().as_secs_f64() * 1_000.0);
        last = Some(output);
    }
    let mean = times.iter().sum::<f64>() / SAMPLES as f64;
    times.sort_by(f64::total_cmp);
    let median = (times[SAMPLES / 2 - 1] + times[SAMPLES / 2]) / 2.0;
    eprintln!(
        "TAPE_MEMORY_STAGE {label}: mean_ms={mean:.3} median_ms={median:.3} samples={SAMPLES} warmups=3"
    );
    last.unwrap()
}

impl TapeDotMemory {
    /// Called only by the existing ignored dense fixture. No production
    /// instrumentation or public-domain API is compiled into the app.
    pub(crate) fn measure_native_stages(
        &mut self,
        marks: &[AggressionPrimitive],
        view: TapeDotView,
        frame: &crate::projection::TapeDotFrame,
        style: &OrderflowRenderStyle,
        openings: &[i64],
    ) {
        let sizing = style.dot_sizing.unwrap();
        let index = measure_stage(
            "native_index_construction",
            || (),
            |_| native_facts(marks, view),
        );
        assert_eq!(index.len(), marks.len());
        let (changed, remaining) = measure_stage(
            "retained_source_replacement",
            || native_facts(marks, view),
            |mut index| {
                let changed = self.replace_facts(&mut index, view.evicted_through_ms, None);
                (changed, index)
            },
        );
        assert!(!changed, "the measured prefix is already retained");
        assert!(
            remaining
                .keys()
                .all(|key| key.0 + view.dot_window_ms > view.now_ms)
        );
        assert!(
            !remaining.is_empty(),
            "the benchmark includes a forming window"
        );
        let _ = measure_stage(
            "frontier_clone_and_forming_preview",
            || (),
            |_| {
                let mut active = self.frontier.clone();
                active.extend(remaining.iter().map(|(key, mark)| {
                    Group::of(BTreeMap::from([(*key, normalized_native_fact(mark))]))
                }));
                let quantities = reference_quantities(&active, openings);
                let reference = TapeReference {
                    minimum_full: self.reference(
                        &active,
                        sizing,
                        view.now_ms,
                        view.window_ms,
                        openings,
                    ),
                    quantities: &quantities,
                };
                merge_groups(
                    active,
                    view,
                    sizing,
                    &style.bubbles,
                    &style.live_lane,
                    reference,
                    Some(view.now_ms),
                )
            },
        );
        let _ = measure_stage("rendered_group_clone", || (), |_| frame.marks.clone());
        let (_, radius) = measure_stage(
            "coordinates_area_and_radius_cap",
            || frame.marks.clone(),
            |mut shown| {
                crate::projection::position_tape_at(
                    &mut shown,
                    view.now_ms,
                    view.window_ms,
                    view.geometry.left_x,
                    view.dot_window_ms,
                );
                for mark in &mut shown {
                    mark.y = view.prices.y_unclamped(mark.price).unwrap_or(mark.y);
                    mark.size =
                        crate::projection::normalized_area_size(mark.quantity, frame.full_quantity);
                }
                let radius = super::tape_radius_limit(
                    &shown,
                    DotSizing {
                        typed_full: Some(frame.full_quantity),
                        ..sizing
                    },
                    &style.bubbles,
                    &style.live_lane,
                    view.geometry,
                );
                (shown, radius)
            },
        );
        assert_eq!(radius, frame.max_radius);
        eprintln!(
            "TAPE_MEMORY_STAGE_BOUNDARY native={} retained_groups={} forming_keys={} rendered={}; fixed warmed forming frame; excludes epoch prep, seal/coincidence passes, final reference selection/sort, input clone, UI/pending/GPU; stages are not a total",
            marks.len(),
            self.retained_group_count(),
            remaining.len(),
            frame.marks.len()
        );
    }
}
