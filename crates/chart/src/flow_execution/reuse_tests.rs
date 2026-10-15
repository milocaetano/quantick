use super::*;
use quantick_engine::{Side, Trade};
use quantick_orderflow::projection::{
    PriceWindow,
    flow_tape::{FlowExecution, FlowReference, FlowTapeFrame, FlowTapeView, project_flow_tape},
};
use rust_decimal::{Decimal, prelude::ToPrimitive as _};

const HISTORY: FlowBounds = [[0.0, 0.0], [600.0, 400.0]];
const TICK: quantick_orderflow::TickBubbleStyle = quantick_orderflow::TickBubbleStyle {
    reference_quantity: 40.0,
    max_radius: 24.0,
    label_min_radius: 8.0,
    opacity: 0.9,
    reference_mode: quantick_orderflow::TickReferenceMode::Fixed,
    viewport_scale: None,
};

/// Prints a second apart at their own prices, so each is a region of its own.
fn prints(count: u64) -> Vec<Trade> {
    (0..count)
        .map(|index| Trade {
            agg_id: index + 1,
            timestamp_ms: 1_000 * (index as i64 + 1),
            price: Decimal::from(100 + index as i64 % 7 * 4),
            quantity: Decimal::from(3 + index as i64 * 5 % 37),
            side: if index.is_multiple_of(3) {
                Side::Sell
            } else {
                Side::Buy
            },
        })
        .collect()
}

fn frame(trades: &[Trade], reference: FlowReference) -> FlowTapeFrame {
    let slots = trades.len().div_ceil(4);
    project_flow_tape(
        trades
            .iter()
            .enumerate()
            .map(|(ordinal, trade)| FlowExecution {
                ordinal,
                slot: ordinal / 4,
                accepted_ordinal: ordinal % 4,
                ticks_per_bar: 4.into(),
                trade,
                opening: false,
            }),
        1,
        trades.len(),
        trades.len(),
        FlowTapeView {
            first_slot: 0,
            end_slot: slots,
            clip_left: 0.into(),
            clip_right: slots.into(),
            width_px: 600.0,
            height_px: 400.0,
            prices: PriceWindow::new(90.into(), 130.into()).unwrap(),
            reference,
            radius_limit: 24.0,
            merge_support_radius: 6.0,
            exclude_opening: false,
        },
    )
}

/// A tick-sphere plan of `frame` under a camera panned `pan` pixels, and its keys.
fn planned(frame: &FlowTapeFrame, pan: f32, tick: bool) -> (FlowPresentation, Vec<FlowRegionKey>) {
    let plan = FlowPresentation::styled(frame, HISTORY, tick.then_some(TICK), |dot| {
        Some([
            dot.candle_position.to_f32()? * 5.0 + pan,
            (130.0 - dot.mark.price.to_f32()?) * 10.0,
        ])
    });
    let keys = plan.region_keys(frame, tick);
    (plan, keys)
}

/// The first print of each region, which names it across frames.
fn first_print(frame: &FlowTapeFrame, plan: &FlowPresentation, region: usize) -> usize {
    let dot = &frame.dots[plan.regions[region].dot_index];
    dot.members
        .iter()
        .map(|member| member.ordinal)
        .min()
        .unwrap()
}

/// A print that joins one region changes that region alone: every other
/// region of the next frame finds its own key in the frame before.
#[test]
fn a_print_joining_one_region_keeps_every_other_regions_key() {
    let mut trades = prints(11);
    let before = frame(&trades, FlowReference::Typed(TICK.reference()));
    let mut joining = trades[10].clone();
    joining.agg_id = 99;
    joining.timestamp_ms += 50;
    trades.push(joining);
    let after = frame(&trades, FlowReference::Typed(TICK.reference()));
    let (old_plan, old_keys) = planned(&before, 0.0, true);
    let (new_plan, new_keys) = planned(&after, 0.0, true);
    assert_eq!(new_plan.regions.len(), 11, "the print joined its region");
    let reused = reusable_regions(&old_keys, &new_keys);
    for (region, held) in reused.iter().enumerate() {
        let first = first_print(&after, &new_plan, region);
        match held {
            Some(held) => {
                assert_eq!(old_keys[*held], new_keys[region]);
                assert_eq!(first_print(&before, &old_plan, *held), first);
            }
            None => assert_eq!(first, 10, "only the joined region is built again"),
        }
    }
    assert_eq!(reused.iter().flatten().count(), 10);
}

/// A new region is built; the rest keep theirs.
#[test]
fn a_new_region_is_built_and_the_rest_reused() {
    let trades = prints(12);
    let reference = FlowReference::Typed(TICK.reference());
    let (_, old_keys) = planned(&frame(&trades[..11], reference), 0.0, true);
    let (new_plan, new_keys) = planned(&frame(&trades, reference), 0.0, true);
    let reused = reusable_regions(&old_keys, &new_keys);
    assert_eq!(new_plan.regions.len(), 12);
    assert_eq!(reused.iter().flatten().count(), 11);
}

/// A camera that moved moves every circle: nothing is kept.
#[test]
fn a_moved_camera_keeps_no_region() {
    let trades = prints(12);
    let frame = frame(&trades, FlowReference::Typed(TICK.reference()));
    let (_, keys) = planned(&frame, 0.0, true);
    let (_, panned) = planned(&frame, 8.0, true);
    assert!(reusable_regions(&keys, &panned).iter().all(Option::is_none));
    assert!(
        reusable_regions(&keys, &keys)
            .iter()
            .enumerate()
            .all(|(region, held)| *held == Some(region)),
        "an unchanged frame keeps every region in place"
    );
}

/// A regional peak's colours follow its emphasis, so its key carries it;
/// tick spheres, coloured by their side alone, never do.
#[test]
fn regional_peaks_key_their_emphasis_and_tick_spheres_do_not() {
    let trades = prints(12);
    let frame = frame(&trades, FlowReference::VisibleRegions);
    let (plan, regional) = planned(&frame, 0.0, false);
    let tick = plan.region_keys(&frame, true);
    let peaks = plan
        .regions
        .iter()
        .filter(|region| region.role == FlowRegionRole::Peak)
        .count();
    assert!(peaks > 0, "the fixture has peaks");
    for (region, (regional, tick)) in plan.regions.iter().zip(regional.iter().zip(&tick)) {
        assert_eq!(regional == tick, region.role != FlowRegionRole::Peak);
    }
}

/// Every match is an equal key, whatever the orders: a shuffled frame keeps
/// what it finds and builds the rest.
#[test]
fn every_match_is_an_equal_key() {
    let trades = prints(40);
    let (_, keys) = planned(
        &frame(&trades, FlowReference::Typed(TICK.reference())),
        0.0,
        true,
    );
    let mut shuffled = keys.clone();
    shuffled.reverse();
    shuffled.rotate_left(7);
    for (region, held) in reusable_regions(&keys, &shuffled).iter().enumerate() {
        if let Some(held) = held {
            assert_eq!(keys[*held], shuffled[region]);
        }
    }
}

/// Runs cover the paint order in order, each inside one pass.
#[test]
fn runs_cut_the_paint_order_within_each_pass() {
    let frame = frame(&prints(400), FlowReference::Typed(TICK.reference()));
    let (plan, keys) = planned(&frame, 0.0, true);
    let runs = paint_runs(&keys);
    assert!(runs.len() > 4, "{} runs", runs.len());
    assert_eq!((runs[0].start, runs[runs.len() - 1].end), (0, keys.len()));
    assert!(runs.windows(2).all(|pair| pair[0].end == pair[1].start));
    let peak = |index: usize| plan.regions[index].role == FlowRegionRole::Peak;
    for run in &runs {
        assert!(run.clone().all(|index| peak(index) == peak(run.start)));
    }
}

/// A new print changes the run its region joins, or the two it splits:
/// every other run is found whole in the frame before; a moved camera keeps
/// none.
#[test]
fn a_new_print_keeps_every_run_but_its_own() {
    let trades = prints(401);
    let reference = FlowReference::Typed(TICK.reference());
    let (_, old_keys) = planned(&frame(&trades[..400], reference), 0.0, true);
    let (_, new_keys) = planned(&frame(&trades, reference), 0.0, true);
    let (old_runs, new_runs) = (paint_runs(&old_keys), paint_runs(&new_keys));
    let kept = reusable_runs((&old_keys, &old_runs), (&new_keys, &new_runs));
    for (run, held) in new_runs.iter().zip(&kept) {
        if let Some(held) = held {
            assert_eq!(old_keys[old_runs[*held].clone()], new_keys[run.clone()]);
        }
    }
    let rebuilt = kept.iter().filter(|held| held.is_none()).count();
    assert!(
        (1..=2).contains(&rebuilt),
        "{rebuilt} of {} runs",
        new_runs.len()
    );
    let (_, panned) = planned(&frame(&trades, reference), 8.0, true);
    let panned_runs = paint_runs(&panned);
    let moved = reusable_runs((&new_keys, &new_runs), (&panned, &panned_runs));
    assert!(moved.iter().all(Option::is_none));
}
