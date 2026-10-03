//! Original execution extents, rather than moving centroids, bound regional merges.
use super::*;

fn at_scale(trades: &[Trade], width: f32, height: f32, prices: PriceWindow) -> FlowTapeFrame {
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
                opening: false,
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
            height_px: height,
            prices,
            reference: FlowReference::VisibleRegions,
            radius_limit: 7.0,
            merge_support_radius: 6.0,
            exclude_opening: false,
        },
    )
}

fn assert_partition(frame: &FlowTapeFrame, ordinals: &[usize], buy: i64, sell: i64) {
    let mut actual: Vec<_> = frame
        .dots
        .iter()
        .flat_map(|dot| dot.members.iter().map(|m| m.ordinal))
        .collect();
    actual.sort_unstable();
    assert_eq!(
        actual, ordinals,
        "each eligible source appears exactly once"
    );
    assert_eq!(
        frame
            .dots
            .iter()
            .map(|d| d.mark.buy_quantity)
            .sum::<Decimal>(),
        buy.into()
    );
    assert_eq!(
        frame
            .dots
            .iter()
            .map(|d| d.mark.quantity - d.mark.buy_quantity)
            .sum::<Decimal>(),
        sell.into()
    );
    assert_eq!(
        frame.dots.iter().map(|d| d.mark.trade_count).sum::<usize>(),
        ordinals.len()
    );
    assert_eq!(frame.omitted_executions, 0);
}

#[test]
fn a_walking_price_centroid_cannot_swallow_an_entire_swing() {
    let trades = [1, 10, 100, 1000, 10000]
        .into_iter()
        .enumerate()
        .map(|(i, q)| {
            trade(
                1000 + i as i64 * 100,
                100 + i as i64 * 10,
                q,
                if i % 2 == 0 { Side::Buy } else { Side::Sell },
            )
        })
        .collect::<Vec<_>>();
    let prices = PriceWindow::new(100.into(), 200.into()).unwrap();
    let close = at_scale(&trades, 1.0, 100.0, prices);
    assert!(
        close.dots.len() >= 2,
        "nearby centroids must not pool a 40px source swing"
    );
    assert!(
        close
            .dots
            .iter()
            .all(|dot| dot.mark.price_span <= Decimal::from(32))
    );
    assert_partition(&close, &[0, 1, 2, 3, 4], 10101, 1010);
    let coarse = at_scale(&trades, 1.0, 50.0, prices);
    assert_eq!(
        coarse.dots.len(),
        1,
        "the complete 20px source span fits one local region after zooming out"
    );
    assert_partition(&coarse, &[0, 1, 2, 3, 4], 10101, 1010);
    assert_eq!(at_scale(&trades, 1.0, 100.0, prices), close);
}

#[test]
fn a_walking_time_centroid_keeps_original_x_extent_bounded() {
    let trades = [1, 10, 100, 1000, 10000, 100000, 1000000]
        .into_iter()
        .enumerate()
        .map(|(i, q)| {
            trade(
                1000 + i as i64 * 100,
                100,
                q,
                if i % 2 == 0 { Side::Buy } else { Side::Sell },
            )
        })
        .collect::<Vec<_>>();
    let prices = PriceWindow::new(90.into(), 110.into()).unwrap();
    let close = at_scale(&trades, 70.0, 100.0, prices);
    assert!(
        close.dots.len() >= 2,
        "nearby centroids must not pool 60px of source positions"
    );
    for dot in &close.dots {
        let min = dot
            .members
            .iter()
            .map(|m| m.accepted_ordinal)
            .min()
            .unwrap();
        let max = dot
            .members
            .iter()
            .map(|m| m.accepted_ordinal)
            .max()
            .unwrap();
        assert!((max - min) as f32 * 10.0 <= 48.0);
    }
    assert_partition(&close, &[0, 1, 2, 3, 4, 5, 6], 1010101, 101010);
    let coarse = at_scale(&trades, 35.0, 100.0, prices);
    assert_eq!(
        coarse.dots.len(),
        1,
        "zooming out puts the full 30px source span inside one local region"
    );
    assert_partition(&coarse, &[0, 1, 2, 3, 4, 5, 6], 1010101, 101010);
    assert_eq!(at_scale(&trades, 70.0, 100.0, prices), close);

    // The real caller grows width with admission overscan. Holding width fixed
    // would describe a different zoom, not the same physical merge contract.
    let mut source = FlowTapeSource::default();
    source.append(
        trades
            .iter()
            .enumerate()
            .map(|(ordinal, trade)| FlowExecution {
                ordinal,
                slot: 0,
                accepted_ordinal: ordinal,
                ticks_per_bar: Decimal::from(trades.len()),
                trade,
                opening: false,
            }),
    );
    let mut padded = close.view;
    padded.end_slot = 3;
    padded.width_px = 210.0;
    let padded = source.project(
        1,
        0,
        trades.len(),
        0..trades.len(),
        padded,
        FlowOpeningSelection::default(),
    );
    let facts = |frame: &FlowTapeFrame| {
        frame
            .dots
            .iter()
            .map(|dot| {
                (
                    dot.members.clone(),
                    dot.mark.quantity,
                    dot.mark.buy_quantity,
                    dot.candle_position,
                    dot.mark.price,
                    dot.radius,
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(facts(&close), facts(&padded));
}

#[test]
fn original_native_cell_extent_matters_even_when_its_centroid_is_nearby() {
    for (last_in_cell, neighbour, cell_width) in [(44, 55, 44.0), (60, 70, 60.0)] {
        let mut trades = vec![trade(1000, 200, 1, Side::Buy); neighbour + 1];
        trades[0] = trade(1000, 100, 1, Side::Buy);
        trades[last_in_cell] = trade(1000, 100, 1000, Side::Sell);
        trades[neighbour] = trade(1100, 100, 100, Side::Buy);
        let frame = at_scale(
            &trades,
            trades.len() as f32,
            100.0,
            PriceWindow::new(90.into(), 110.into()).unwrap(),
        );
        assert_eq!(
            frame.dots.len(),
            2,
            "near centroids must not hide the original native extent"
        );
        let native = frame
            .dots
            .iter()
            .find(|dot| dot.mark.trade_count == 2)
            .unwrap();
        assert_eq!(native.native_cells, 1);
        assert_eq!(
            native.members.iter().map(|m| m.ordinal).collect::<Vec<_>>(),
            [0, last_in_cell]
        );
        assert_eq!(last_in_cell as f32, cell_width);
        // An atomic cell already wider than48px remains intact and cannot absorb a neighbour.
        assert_partition(&frame, &[0, last_in_cell, neighbour], 101, 1000);
        assert_eq!(frame.off_axis_executions, trades.len() - 3);
    }
}

#[test]
fn many_oversized_atomic_cells_stay_intact_without_entering_the_collision_frontier() {
    const CELLS: usize = 10_000;
    // Every cell revisits its exact price/window half a candle later. Its64px
    // original span cannot merge, although thousands of centroids are nearby.
    let trades = (0..CELLS * 2)
        .map(|i| {
            trade(
                1000 + (i % CELLS) as i64 * 100,
                100,
                if i < CELLS { 1 } else { 2 },
                if i < CELLS { Side::Buy } else { Side::Sell },
            )
        })
        .collect::<Vec<_>>();
    let frame = at_scale(
        &trades,
        128.0,
        100.0,
        PriceWindow::new(90.into(), 110.into()).unwrap(),
    );
    assert_eq!(frame.dots.len(), CELLS);
    assert!(
        frame
            .dots
            .iter()
            .all(|dot| dot.native_cells == 1 && dot.mark.trade_count == 2)
    );
    assert_partition(
        &frame,
        &(0..CELLS * 2).collect::<Vec<_>>(),
        CELLS as i64,
        CELLS as i64 * 2,
    );
}

#[test]
fn geometric_regions_pool_neighbours_without_a_painted_size_floor() {
    let trades = [
        trade(1000, 100, 1, Side::Buy),
        trade(1100, 100, 1, Side::Sell),
        trade(1200, 100, 1000, Side::Buy),
    ];
    let prices = PriceWindow::new(90.into(), 110.into()).unwrap();
    // Eight pixels between cells: the two small neighbours form one region.
    // Its centroid remains too far from the whale to merge at this zoom.
    let regional = at_scale(&trades, 24.0, 100.0, prices);
    assert_eq!(regional.dots.len(), 2);
    let dominant = regional
        .dots
        .iter()
        .find(|dot| dot.mark.quantity == Decimal::from(1000))
        .unwrap();
    assert_eq!(
        dominant
            .members
            .iter()
            .map(|m| m.ordinal)
            .collect::<Vec<_>>(),
        [2]
    );
    let tiny = regional
        .dots
        .iter()
        .find(|dot| dot.mark.quantity == Decimal::from(2))
        .unwrap();
    assert!((dominant.radius.powi(2) / tiny.radius.powi(2) - 500.0).abs() < 0.001);
    assert!(
        tiny.radius < 1.0,
        "spatial support is not a painted radius floor"
    );
    assert_partition(&regional, &[0, 1, 2], 1001, 1);
    let micro = at_scale(&trades, 120.0, 100.0, prices);
    assert_eq!(micro.dots.len(), 3);
    assert_partition(&micro, &[0, 1, 2], 1001, 1);
    let distant = at_scale(&trades, 1.0, 100.0, prices);
    assert_eq!(distant.dots.len(), 1);
    assert_partition(&distant, &[0, 1, 2], 1001, 1);
    assert_eq!(at_scale(&trades, 24.0, 100.0, prices), regional);
}

#[test]
fn remote_volume_never_changes_local_regions_or_toggle_membership() {
    let prices = PriceWindow::new(90.into(), 110.into()).unwrap();
    let trades = [
        trade(1000, 109, 1_000_000, Side::Buy),
        trade(1200, 100, 1, Side::Buy),
        trade(1300, 100, 1, Side::Sell),
        trade(1400, 100, 1000, Side::Buy),
    ];
    let project = |opening: bool, exclude: bool, remote_quantity: i64| {
        let mut trades = trades.clone();
        trades[0].quantity = remote_quantity.into();
        let mut view = at_scale(&trades, 32.0, 200.0, prices).view;
        view.exclude_opening = exclude;
        project_flow_tape(
            trades
                .iter()
                .enumerate()
                .map(|(ordinal, trade)| FlowExecution {
                    ordinal,
                    slot: 0,
                    accepted_ordinal: ordinal,
                    ticks_per_bar: 4.into(),
                    trade,
                    opening: opening && ordinal == 0,
                }),
            1,
            4,
            4,
            view,
        )
    };
    let opening = project(true, false, 1_000_000);
    let excluded = project(true, true, 1_000_000);
    let small_remote = project(false, false, 1);
    let members = |frame: &FlowTapeFrame| {
        frame
            .dots
            .iter()
            .map(|dot| dot.members.iter().map(|m| m.ordinal).collect::<Vec<_>>())
            .collect::<Vec<_>>()
    };
    assert_eq!(members(&opening), members(&excluded));
    assert_eq!(members(&opening), members(&small_remote));
    for frame in [&opening, &small_remote] {
        let radius = |quantity| {
            frame
                .dots
                .iter()
                .find(|dot| dot.mark.quantity == Decimal::from(quantity))
                .unwrap()
                .radius
        };
        assert!((radius(1000).powi(2) / radius(2).powi(2) - 500.0).abs() < 0.001);
    }
    assert!(
        opening
            .dots
            .iter()
            .any(|dot| dot.mark.quantity == Decimal::from(2))
    );
    assert_eq!(
        members(&project(false, false, 1_000_000)),
        members(&opening),
        "remote opening classification changes calibration, never local membership"
    );
    assert_partition(&opening, &[0, 1, 2, 3], 1_001_001, 1);
    assert_partition(&excluded, &[0, 1, 2, 3], 1_001_001, 1);
}

#[test]
fn opening_collision_groups_survive_first_region_scale_exclusion() {
    let trades = [0; 3].map(|_| trade(1000, 100, 100, Side::Buy));
    let project = |opening| {
        let mut view = at_scale(
            &trades,
            30.0,
            100.0,
            PriceWindow::new(90.into(), 110.into()).unwrap(),
        )
        .view;
        view.end_slot = 5;
        view.clip_right = 5.into();
        view.exclude_opening = true;
        project_flow_tape(
            trades
                .iter()
                .zip([0, 1, 4])
                .enumerate()
                .map(|(ordinal, (trade, slot))| FlowExecution {
                    ordinal,
                    slot,
                    accepted_ordinal: 0,
                    ticks_per_bar: 1.into(),
                    trade,
                    opening,
                }),
            1,
            3,
            3,
            view,
        )
    };
    let ordinary = project(false);
    let opening = project(true);
    let facts = |frame: &FlowTapeFrame| {
        frame
            .dots
            .iter()
            .map(|dot| (dot.members.clone(), dot.mark.quantity))
            .collect::<Vec<_>>()
    };
    assert_eq!(ordinary.dots.len(), 2);
    assert_eq!(
        facts(&opening),
        facts(&ordinary),
        "first-region scaling must preserve the collision groups"
    );
    assert_eq!(opening.effective_reference, Some(100.into()));
    assert_eq!(
        opening.scale_basis,
        FlowScaleBasis::VisibleOrdinaryRegionMax
    );
    assert_partition(&opening, &[0, 1, 2], 300, 0);
}
