//! Short execution intervals retain their volume differences at coarse candle zoom.
use super::*;

fn project(trades: &[Trade], width: f32, opening: bool, exclude: bool) -> FlowTapeFrame {
    project_flow_tape(
        trades
            .iter()
            .enumerate()
            .map(|(ordinal, trade)| FlowExecution {
                ordinal,
                slot: 0,
                accepted_ordinal: ordinal,
                ticks_per_bar: Decimal::from(trades.len()),
                trade,
                opening: opening && ordinal == 0,
            }),
        1,
        trades.len(),
        trades.len(),
        FlowTapeView {
            first_slot: 0,
            end_slot: 1,
            clip_left: Decimal::ZERO,
            clip_right: Decimal::ONE,
            width_px: width,
            height_px: 100.0,
            prices: PriceWindow::new(90.into(), 110.into()).unwrap(),
            reference: FlowReference::VisibleRegions,
            radius_limit: 12.0,
            merge_support_radius: 6.0,
            exclude_opening: exclude,
        },
    )
}

fn quantity(frame: &FlowTapeFrame, quantity: i64) -> &FlowTapeDot {
    frame
        .dots
        .iter()
        .find(|dot| dot.mark.quantity == Decimal::from(quantity))
        .expect("the execution interval retains its factual quantity")
}

fn assert_partition(frame: &FlowTapeFrame, count: usize, buy: i64, sell: i64) {
    let mut ordinals = frame
        .dots
        .iter()
        .flat_map(|dot| dot.members.iter().map(|member| member.ordinal))
        .collect::<Vec<_>>();
    ordinals.sort_unstable();
    assert_eq!(ordinals, (0..count).collect::<Vec<_>>());
    assert_eq!(
        frame
            .dots
            .iter()
            .map(|dot| dot.mark.buy_quantity)
            .sum::<Decimal>(),
        Decimal::from(buy)
    );
    assert_eq!(
        frame
            .dots
            .iter()
            .map(|dot| dot.mark.quantity - dot.mark.buy_quantity)
            .sum::<Decimal>(),
        Decimal::from(sell)
    );
    assert_eq!(frame.omitted_executions, 0);
}

#[test]
fn overlapping_intervals_keep_4500_to_600_area_and_allow_a_single_print_to_be_tiny() {
    let trades = [
        trade(0, 100, 4500, Side::Buy),
        trade(FLOW_REGION_WINDOW_MS, 100, 600, Side::Sell),
        trade(2 * FLOW_REGION_WINDOW_MS, 100, 1, Side::Buy),
    ];
    let frame = project(&trades, 1.0, false, false);
    assert_eq!(frame.dots.len(), 3);
    assert_eq!(frame.effective_reference, Some(4500.into()));
    let large = quantity(&frame, 4500).radius;
    let small = quantity(&frame, 600).radius;
    assert_eq!(large, 12.0);
    assert!((large.powi(2) / small.powi(2) - 7.5).abs() < 0.00001);
    assert!(quantity(&frame, 1).radius < 0.2);
    assert_partition(&frame, 3, 4501, 600);
}

#[test]
fn nearby_prices_in_one_interval_pool_and_zoom_recovers_the_execution_path() {
    let trades = [trade(1, 99, 450, Side::Buy), trade(2, 100, 450, Side::Sell)];
    let coarse = project(&trades, 12.0, false, false);
    assert_eq!(coarse.dots.len(), 1);
    let region = quantity(&coarse, 900);
    assert_eq!(region.native_cells, 2);
    assert_eq!(region.mark.buy_quantity, 450.into());
    assert_eq!(region.mark.price, Decimal::new(995, 1));
    let (buy, sell) = region.side_radii();
    assert!((buy.powi(2) - sell.powi(2)).abs() < 0.00001);
    assert!((buy.powi(2) + sell.powi(2) - region.radius.powi(2)).abs() < 0.0001);
    let detail = project(&trades, 200.0, false, false);
    assert_eq!(detail.dots.len(), 2);
    assert_eq!(
        detail
            .dots
            .iter()
            .map(|dot| dot.mark.price)
            .collect::<Vec<_>>(),
        [Decimal::from(99), Decimal::from(100)]
    );
    assert_partition(&coarse, 2, 450, 450);
    assert_partition(&detail, 2, 450, 450);
    assert_eq!(project(&trades, 12.0, false, false), coarse);
}

#[test]
fn equal_routine_intervals_remain_equal_without_manufacturing_prominent_events() {
    let trades = (0..12)
        .map(|index| trade(index * FLOW_REGION_WINDOW_MS, 100, 600, Side::Buy))
        .collect::<Vec<_>>();
    let frame = project(&trades, 1.0, false, false);
    assert_eq!(frame.dots.len(), trades.len());
    assert!(frame.dots.iter().all(|dot| dot.mark.quantity == 600.into()));
    assert!(frame.dots.iter().all(|dot| dot.radius == 12.0));
    assert_partition(&frame, 12, 7200, 0);
}

#[test]
fn aligned_interval_boundary_is_explicit_and_never_discards_either_side() {
    assert_eq!(FLOW_REGION_WINDOW_MS % 100, 0);
    let trades = [
        trade(FLOW_REGION_WINDOW_MS - 1, 100, 2000, Side::Buy),
        trade(FLOW_REGION_WINDOW_MS, 100, 2500, Side::Sell),
    ];
    let frame = project(&trades, 1.0, false, false);
    assert_eq!(frame.dots.len(), 2);
    assert_eq!(quantity(&frame, 2000).members.len(), 1);
    assert_eq!(quantity(&frame, 2500).members.len(), 1);
    assert_partition(&frame, 2, 2000, 2500);
}

#[test]
fn opening_exclusion_changes_only_scale_across_separate_intervals() {
    let trades = [
        trade(0, 100, 9000, Side::Buy),
        trade(FLOW_REGION_WINDOW_MS, 100, 600, Side::Sell),
        trade(2 * FLOW_REGION_WINDOW_MS, 100, 4500, Side::Buy),
    ];
    let excluded = project(&trades, 1.0, true, true);
    let included = project(&trades, 1.0, true, false);
    assert_eq!(excluded.effective_reference, Some(4500.into()));
    assert_eq!(included.effective_reference, Some(9000.into()));
    let first = quantity(&excluded, 9000);
    assert!(first.opening_anchor && first.opening_oversized);
    assert!((first.radius.powi(2) - 288.0).abs() < 0.0001);
    assert_eq!(
        excluded
            .dots
            .iter()
            .filter(|dot| dot.opening_oversized)
            .count(),
        1
    );
    for (a, b) in excluded.dots.iter().zip(&included.dots) {
        assert_eq!(a.members, b.members);
        assert_eq!(a.mark.quantity, b.mark.quantity);
        assert_eq!(a.mark.buy_quantity, b.mark.buy_quantity);
        assert_eq!(a.candle_position, b.candle_position);
    }
    assert_partition(&excluded, 3, 13_500, 600);
    assert_partition(&included, 3, 13_500, 600);
}

#[test]
fn paint_order_keeps_the_opening_underneath_then_reveals_larger_ordinary_volume() {
    let trades = [
        trade(0, 100, 9000, Side::Buy),
        trade(FLOW_REGION_WINDOW_MS, 100, 4500, Side::Sell),
        trade(2 * FLOW_REGION_WINDOW_MS, 100, 600, Side::Buy),
        trade(3 * FLOW_REGION_WINDOW_MS, 100, 600, Side::Sell),
        trade(4 * FLOW_REGION_WINDOW_MS, 100, 1, Side::Buy),
    ];
    let frame = project(&trades, 1.0, true, true);
    let chronological = frame.dots.clone();
    let painted = frame.dots_in_paint_order().collect::<Vec<_>>();
    assert_eq!(
        painted
            .iter()
            .map(|dot| dot.members.iter().next().unwrap().ordinal)
            .collect::<Vec<_>>(),
        [0, 4, 2, 3, 1],
        "the giant stays behind; equal ordinary volumes keep source order"
    );
    assert!(painted[0].opening_oversized);
    assert_eq!(painted.last().unwrap().mark.quantity, 4500.into());
    assert_eq!(painted.len(), frame.dots.len());
    assert_eq!(
        painted.iter().map(|dot| dot.mark.quantity).sum::<Decimal>(),
        Decimal::from(14_701)
    );
    assert_eq!(frame.dots, chronological);
    assert_eq!(
        frame
            .dots
            .iter()
            .map(|dot| dot.mark.first_timestamp_ms)
            .collect::<Vec<_>>(),
        (0..5)
            .map(|index| index * FLOW_REGION_WINDOW_MS)
            .collect::<Vec<_>>()
    );
    assert_partition(&frame, 5, 9601, 5100);
}

#[test]
fn sparse_time_regions_are_independent_of_source_delivery_order() {
    let trades = [
        trade(1, 99, 450, Side::Buy),
        trade(2, 100, 450, Side::Sell),
        trade(FLOW_REGION_WINDOW_MS + 1, 99, 600, Side::Sell),
        trade(2 * FLOW_REGION_WINDOW_MS + 1, 100, 4500, Side::Buy),
    ];
    let ordered = project(&trades, 12.0, false, false);
    let reversed = project_flow_tape(
        trades
            .iter()
            .enumerate()
            .rev()
            .map(|(ordinal, trade)| FlowExecution {
                ordinal,
                slot: 0,
                accepted_ordinal: ordinal,
                ticks_per_bar: Decimal::from(trades.len()),
                trade,
                opening: false,
            }),
        1,
        trades.len(),
        trades.len(),
        ordered.view,
    );
    assert_eq!(reversed, ordered);
    assert_partition(&ordered, 4, 4950, 1050);
}

#[test]
fn many_temporally_separate_regions_share_pixels_without_forming_one_false_event() {
    const COUNT: usize = 20_000;
    let trades = (0..COUNT)
        .map(|index| trade(index as i64 * FLOW_REGION_WINDOW_MS, 100, 1, Side::Buy))
        .collect::<Vec<_>>();
    let frame = project(&trades, 1.0, false, false);
    assert_eq!(frame.dots.len(), COUNT);
    assert!(frame.dots.iter().all(|dot| dot.members.len() == 1));
    assert_partition(&frame, COUNT, COUNT as i64, 0);
}
