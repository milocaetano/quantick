use super::*;
use quantick_engine::Side;
#[path = "tests/hull.rs"]
mod hull;
#[path = "tests/reading.rs"]
mod reading;
fn trade(time: i64, price: i64, quantity: i64, side: Side) -> Trade {
    Trade {
        agg_id: 1,
        timestamp_ms: time,
        price: price.into(),
        quantity: quantity.into(),
        side,
    }
}
fn view(width: f32) -> FlowTapeView {
    FlowTapeView {
        first_slot: 0,
        end_slot: 2,
        clip_left: 0.into(),
        clip_right: 2.into(),
        width_px: width,
        height_px: 200.0,
        prices: PriceWindow::new(90.into(), 110.into()).unwrap(),
        reference: FlowReference::Typed(900.into()),
        radius_limit: 7.0,
        merge_support_radius: 12.0,
        exclude_opening: true,
    }
}
fn frame(trades: &[Trade], width: f32) -> FlowTapeFrame {
    project_flow_tape(
        trades
            .iter()
            .enumerate()
            .map(|(ordinal, trade)| FlowExecution {
                ordinal,
                slot: ordinal / 2,
                accepted_ordinal: ordinal % 2,
                ticks_per_bar: 2.into(),
                trade,
                opening: false,
            }),
        1,
        trades.len(),
        trades.len(),
        view(width),
    )
}
#[test]
fn low_900_and_high_90_have_ten_to_one_area() {
    let f = frame(
        &[
            trade(1000, 95, 900, Side::Sell),
            trade(1100, 105, 90, Side::Buy),
        ],
        200.0,
    );
    assert_eq!(f.dots.len(), 2);
    let area = |quantity| {
        let r = f
            .dots
            .iter()
            .find(|d| d.mark.quantity == Decimal::from(quantity))
            .unwrap()
            .radius;
        r * r
    };
    assert!((area(900) / area(90) - 10.0).abs() < 0.0001);
}
#[test]
fn neighboring_450s_merge_as_900_and_zoom_restores_constituents() {
    let trades = [
        trade(1000, 100, 450, Side::Buy),
        trade(1100, 100, 450, Side::Sell),
    ];
    let close = frame(&trades, 200.0);
    let coarse = frame(&trades, 10.0);
    assert_eq!(close.dots.len(), 2);
    assert_eq!(coarse.dots.len(), 1);
    let merged = &coarse.dots[0];
    assert_eq!(merged.mark.quantity, Decimal::from(900));
    assert_eq!(merged.mark.buy_quantity, Decimal::from(450));
    assert_eq!(merged.mark.trade_count, 2);
    assert_eq!(merged.radius, 7.0);
    assert_eq!(
        merged.members.iter().map(|m| m.ordinal).collect::<Vec<_>>(),
        [0, 1]
    );
    assert_eq!(frame(&trades, 200.0), close);
}
#[test]
fn forming_to_closed_keeps_the_accepted_position() {
    let first = trade(1000, 100, 90, Side::Buy);
    let before = frame(std::slice::from_ref(&first), 200.0);
    let after = frame(&[first, trade(1100, 105, 10, Side::Sell)], 200.0);
    assert_eq!(before.dots[0].candle_position, Decimal::new(25, 2));
    assert_eq!(
        before.dots[0].candle_position,
        after.dots[0].candle_position
    );
}
#[test]
fn opening_cap_changes_no_source_facts() {
    let trade = trade(1000, 100, 9000, Side::Buy);
    let f = project_flow_tape(
        [FlowExecution {
            ordinal: 0,
            slot: 0,
            accepted_ordinal: 0,
            ticks_per_bar: 2.into(),
            trade: &trade,
            opening: true,
        }],
        3,
        1,
        1,
        view(200.0),
    );
    assert_eq!(f.effective_reference, Some(Decimal::from(900)));
    assert!(f.dots[0].opening_capped);
    assert_eq!(f.dots[0].mark.quantity, Decimal::from(9000));
    assert_eq!(f.dots[0].opening_quantity, Decimal::from(9000));
}
#[test]
fn ordinary_overflow_shrinks_the_common_frame_without_flattening_ratios() {
    let f = frame(
        &[
            trade(1000, 95, 1800, Side::Buy),
            trade(1100, 105, 900, Side::Sell),
        ],
        200.0,
    );
    assert_eq!(f.effective_reference, Some(Decimal::from(1800)));
    let a = f.dots[0].radius;
    let b = f.dots[1].radius;
    assert!((a * a / (b * b) - 2.0).abs() < 0.0001);
}

#[test]
fn mixed_opening_group_keeps_ordinary_quantity_in_the_common_reference() {
    let trades = [
        trade(1000, 100, 100, Side::Buy),
        trade(1100, 100, 1800, Side::Sell),
    ];
    let f = project_flow_tape(
        trades
            .iter()
            .enumerate()
            .map(|(ordinal, trade)| FlowExecution {
                ordinal,
                slot: 0,
                accepted_ordinal: ordinal,
                ticks_per_bar: 2.into(),
                trade,
                opening: ordinal == 0,
            }),
        1,
        2,
        2,
        view(10.0),
    );
    assert_eq!(f.effective_reference, Some(Decimal::from(1800)));
    assert_eq!(f.dots[0].mark.quantity, Decimal::from(1900));
    assert_eq!(f.dots[0].opening_quantity, Decimal::from(100));
    assert!(f.dots[0].opening_capped);
}
#[test]
fn off_axis_whale_does_not_rescale_visible_regions() {
    let f = frame(
        &[
            trade(1000, 100, 90, Side::Buy),
            trade(1100, 200, 90000, Side::Sell),
        ],
        200.0,
    );
    assert_eq!(f.effective_reference, Some(Decimal::from(900)));
    assert_eq!(f.dots.len(), 1);
    assert_eq!(f.off_axis_executions, 1);
}
#[test]
fn dense_bounded_source_cells_are_bounded_and_conserve_every_member() {
    let trades = (0..80_000)
        .map(|n| trade(n as i64 * 100, 100, 1, Side::Buy))
        .collect::<Vec<_>>();
    let f = project_flow_tape(
        trades
            .iter()
            .enumerate()
            .map(|(ordinal, trade)| FlowExecution {
                ordinal,
                slot: 0,
                accepted_ordinal: ordinal,
                ticks_per_bar: Decimal::from(80_000),
                trade,
                opening: false,
            }),
        1,
        trades.len(),
        trades.len(),
        view(1.0),
    );
    assert_eq!(f.dots.len(), 1);
    assert_eq!(f.dots[0].members.len(), 80_000);
    assert_eq!(f.dots[0].mark.quantity, Decimal::from(80_000));
}

#[test]
fn regional_support_combines_tiny_prints_without_a_painted_radius_floor() {
    let trades = [
        trade(1000, 100, 1, Side::Buy),
        trade(1100, 100, 1, Side::Sell),
    ];
    // Centres are ten pixels apart: far beyond their subpixel painted circles,
    // but inside the same region at this zoom.
    let regional = frame(&trades, 40.0);
    assert_eq!(regional.dots.len(), 1);
    let dot = &regional.dots[0];
    assert_eq!(dot.mark.quantity, Decimal::from(2));
    assert_eq!(dot.mark.buy_quantity, Decimal::ONE);
    assert_eq!(dot.mark.trade_count, 2);
    assert_eq!(dot.native_cells, 2);
    assert!(
        dot.radius < 0.34,
        "group support must never become a painted radius floor"
    );
    // Fifty pixels of separation recover the two native execution cells.
    let detail = frame(&trades, 200.0);
    assert_eq!(detail.dots.len(), 2);
    assert!(detail.dots.iter().all(|dot| dot.radius < 0.24));
    assert_eq!(
        detail
            .dots
            .iter()
            .map(|dot| dot.mark.quantity)
            .sum::<Decimal>(),
        Decimal::from(2)
    );
}

fn owned_chunk(epoch: u64, ordinals: std::ops::Range<usize>) -> FlowChunk {
    FlowChunk {
        epoch,
        ticks_per_bar: 2.into(),
        executions: ordinals
            .map(|ordinal| OwnedFlowExecution {
                ordinal,
                slot: ordinal / 2,
                accepted_ordinal: ordinal % 2,
                trade: trade(1000, 100, 1, Side::Buy),
            })
            .collect(),
    }
}
fn request(epoch: u64, end: usize) -> FlowRequest {
    FlowRequest {
        epoch,
        layout_revision: 0,
        source_count: end,
        requested: 0..end,
        keep: FlowKeep {
            slots: 0..end.div_ceil(2),
            ordinals: 0..end,
        },
        view: view(200.0),
        opening_windows: Vec::new(),
    }
}
#[test]
fn cached_incremental_cells_preserve_old_snapshots_and_exact_same_ms_membership() {
    let mut cache = FlowWorkerCache::default();
    cache.select_epoch(3);
    cache.append(&owned_chunk(3, 0..1));
    let partial = cache.project(&request(3, 1));
    cache.append(&owned_chunk(3, 1..3));
    cache.append(&owned_chunk(3, 1..3)); // retry is not a second execution
    let closed = cache.project(&request(3, 3));
    assert_eq!(cache.loaded(), 3);
    assert_eq!(partial.dots[0].mark.quantity, Decimal::ONE);
    assert_eq!(partial.dots[0].members.len(), 1);
    assert_eq!(
        closed
            .dots
            .iter()
            .map(|dot| dot.mark.quantity)
            .sum::<Decimal>(),
        Decimal::from(3)
    );
    let member = closed
        .dots
        .iter()
        .flat_map(|dot| dot.members.iter())
        .find(|member| member.ordinal == 2)
        .unwrap();
    assert_eq!((member.candle_slot, member.accepted_ordinal), (1, 0));
    let again = cache.project(&request(3, 3));
    assert!(Arc::ptr_eq(
        &closed.dots[0].members.parts[0],
        &again.dots[0].members.parts[0]
    ));
}
#[test]
fn sparse_coverage_cannot_claim_completion_from_the_newest_ordinal() {
    let mut cache = FlowWorkerCache::default();
    cache.select_epoch(1);
    cache.append(&owned_chunk(1, 2..4));
    let frame = cache.project(&request(1, 4));
    assert_eq!(frame.loaded_executions, 2);
    assert_eq!(frame.omitted_executions, 2);
    assert_eq!(frame.computed_through_ordinal, 0);
    cache.append(&owned_chunk(1, 0..2));
    let frame = cache.project(&request(1, 4));
    assert_eq!(frame.omitted_executions, 0);
    assert_eq!(frame.computed_through_ordinal, 4);
}
#[test]
fn old_source_epoch_is_rejected_and_openings_are_reclassified_from_current_metadata() {
    let mut cache = FlowWorkerCache::default();
    cache.select_epoch(1);
    cache.append(&owned_chunk(1, 0..1));
    cache.select_epoch(2);
    cache.append(&owned_chunk(1, 1..2));
    assert_eq!(cache.loaded(), 0);
    cache.append(&owned_chunk(2, 0..2));
    let mut request = request(2, 2);
    assert_eq!(
        cache.project(&request).dots[0].opening_quantity,
        Decimal::ZERO
    );
    request.opening_windows = vec![1000];
    assert_eq!(
        cache.project(&request).dots[0].opening_quantity,
        Decimal::from(2)
    );
    request.opening_windows = vec![900];
    assert_eq!(
        cache.project(&request).dots[0].opening_quantity,
        Decimal::ZERO
    );
}

#[test]
fn shuffled_source_chunks_have_the_same_completed_regional_projection() {
    let mut ordered = FlowWorkerCache::default();
    ordered.select_epoch(1);
    ordered.append(&owned_chunk(1, 0..4));
    let mut shuffled = FlowWorkerCache::default();
    shuffled.select_epoch(1);
    for ordinal in [3, 1, 2, 0] {
        shuffled.append(&owned_chunk(1, ordinal..ordinal + 1));
    }
    assert_eq!(
        ordered.project(&request(1, 4)),
        shuffled.project(&request(1, 4))
    );
}

#[test]
fn nonpositive_admitted_quantities_finish_loading_and_report_ineligible_coverage() {
    let mut cache = FlowWorkerCache::default();
    cache.select_epoch(1);
    let mut packet = owned_chunk(1, 0..3);
    packet.executions[0].trade.quantity = Decimal::ZERO;
    packet.executions[1].trade.quantity = Decimal::from(-2);
    packet.executions[2].trade.quantity = Decimal::from(5);
    cache.append(&packet);
    cache.append(&packet);
    let frame = cache.project(&request(1, 3));
    assert_eq!(frame.loaded_executions, 3);
    assert_eq!(frame.ineligible_executions, 2);
    assert_eq!(frame.omitted_executions, 0);
    assert_eq!(frame.computed_through_ordinal, 3);
    assert_eq!(
        frame
            .dots
            .iter()
            .map(|dot| dot.mark.quantity)
            .sum::<Decimal>(),
        Decimal::from(5)
    );
    assert_eq!(frame.dots[0].members.iter().next().unwrap().ordinal, 2);
    let mut invalid = owned_chunk(1, 3..4);
    invalid.ticks_per_bar = Decimal::ZERO;
    cache.append(&invalid);
    let frame = cache.project(&request(1, 4));
    assert_eq!(frame.ineligible_executions, 3);
    assert_eq!(frame.omitted_executions, 0);
}

fn automatic_frame(
    trades: &[Trade],
    opening: &[usize],
    width: f32,
    exclude: bool,
) -> FlowTapeFrame {
    let mut auto = view(width);
    auto.reference = FlowReference::VisibleRegions;
    auto.exclude_opening = exclude;
    project_flow_tape(
        trades
            .iter()
            .enumerate()
            .map(|(ordinal, trade)| FlowExecution {
                ordinal,
                slot: ordinal / 2,
                accepted_ordinal: ordinal % 2,
                ticks_per_bar: 2.into(),
                trade,
                opening: opening.contains(&ordinal),
            }),
        1,
        trades.len(),
        trades.len(),
        auto,
    )
}
#[test]
fn automatic_region_scale_is_unit_invariant_below_one_contract() {
    let mut trades = [
        trade(1000, 95, 9, Side::Buy),
        trade(1100, 105, 9, Side::Sell),
    ];
    trades[0].quantity = Decimal::new(9, 4);
    trades[1].quantity = Decimal::new(9, 5);
    let fractional = automatic_frame(&trades, &[], 200.0, true);
    assert_eq!(fractional.effective_reference, Some(Decimal::new(9, 4)));
    assert_eq!(fractional.scale_basis, FlowScaleBasis::VisibleRegionMax);
    for trade in &mut trades {
        trade.quantity *= Decimal::from(10_000);
    }
    let integral = automatic_frame(&trades, &[], 200.0, true);
    assert_eq!(
        fractional
            .dots
            .iter()
            .map(|dot| dot.radius)
            .collect::<Vec<_>>(),
        integral
            .dots
            .iter()
            .map(|dot| dot.radius)
            .collect::<Vec<_>>()
    );
    assert!(
        (fractional.dots[0].radius.powi(2) / fractional.dots[1].radius.powi(2) - 10.0).abs()
            < 0.0001
    );
}
#[test]
fn automatic_opening_only_fallback_keeps_area_ratios_and_empty_has_no_reference() {
    let trades = [
        trade(1000, 95, 450, Side::Buy),
        trade(1100, 105, 90, Side::Sell),
    ];
    let frame = automatic_frame(&trades, &[0, 1], 200.0, true);
    assert_eq!(frame.effective_reference, Some(Decimal::from(450)));
    assert_eq!(frame.scale_basis, FlowScaleBasis::OpeningOnlyFallback);
    assert!(!frame.opening_exclusion_effective);
    assert!(frame.dots.iter().all(|dot| !dot.opening_capped));
    assert!((frame.dots[0].radius.powi(2) / frame.dots[1].radius.powi(2) - 5.0).abs() < 0.0001);
    let empty = automatic_frame(&[], &[], 200.0, true);
    assert_eq!(empty.effective_reference, None);
    assert_eq!(empty.scale_basis, FlowScaleBasis::Empty);
}
#[test]
fn automatic_mixed_opening_cap_keeps_gross_facts_and_toggle_keeps_membership() {
    let trades = [
        trade(1000, 100, 100, Side::Buy),
        trade(1100, 100, 1800, Side::Sell),
        trade(1200, 105, 900, Side::Buy),
    ];
    let excluded = automatic_frame(&trades, &[0], 10.0, true);
    let included = automatic_frame(&trades, &[0], 10.0, false);
    assert_eq!(excluded.effective_reference, Some(Decimal::from(1800)));
    assert_eq!(included.effective_reference, Some(Decimal::from(1900)));
    assert!(excluded.opening_exclusion_effective);
    assert_eq!(
        excluded.scale_basis,
        FlowScaleBasis::VisibleOrdinaryRegionMax
    );
    assert!(!included.opening_exclusion_effective);
    let mixed = excluded
        .dots
        .iter()
        .find(|dot| dot.opening_quantity > Decimal::ZERO)
        .unwrap();
    assert_eq!(mixed.mark.quantity, Decimal::from(1900));
    assert!(mixed.opening_capped);
    assert_eq!(mixed.radius, 7.0);
    let (buy_radius, sell_radius) = mixed.side_radii();
    assert!((buy_radius.powi(2) - 49.0 * 100.0 / 1900.0).abs() < 0.00001);
    assert!((sell_radius.powi(2) - 49.0 * 1800.0 / 1900.0).abs() < 0.00001);
    let ordinary = excluded
        .dots
        .iter()
        .find(|dot| dot.opening_quantity == Decimal::ZERO)
        .unwrap();
    assert!((ordinary.radius - 7.0 / 2.0_f32.sqrt()).abs() < 0.00001);
    for (a, b) in excluded.dots.iter().zip(&included.dots) {
        assert_eq!(a.members, b.members);
        assert_eq!(a.mark.quantity, b.mark.quantity);
        assert_eq!(a.mark.buy_quantity, b.mark.buy_quantity);
        assert!(!b.opening_capped);
    }
}

#[test]
fn padded_source_outside_actual_fractional_clip_does_not_set_visible_reference() {
    let trades = [
        trade(1000, 100, 939, Side::Buy),
        trade(1100, 105, 769, Side::Sell),
        trade(1200, 100, 9000, Side::Buy),
    ];
    let mut source = FlowTapeSource::default();
    source.append(
        trades
            .iter()
            .enumerate()
            .map(|(ordinal, trade)| FlowExecution {
                ordinal,
                slot: ordinal,
                accepted_ordinal: 0,
                ticks_per_bar: 2.into(),
                trade,
                opening: false,
            }),
    );
    let mut view = view(300.0);
    view.end_slot = 3;
    view.clip_left = Decimal::new(5, 1);
    view.clip_right = Decimal::new(20, 1);
    view.reference = FlowReference::VisibleRegions;
    let frame = source.project(1, 0, 3, 0..3, view, &[]);
    assert_eq!(frame.effective_reference, Some(769.into()));
    assert_eq!(frame.offscreen_executions, 2);
    assert_eq!(frame.off_axis_executions, 0);
    assert_eq!(frame.omitted_executions, 0);
    assert_eq!(frame.dots.len(), 1);
    assert_eq!(frame.dots[0].members.iter().next().unwrap().ordinal, 1);
}

#[test]
fn side_radii_keep_a_positive_decimal_minority_when_the_cached_share_is_zero() {
    let mut projected = frame(&[trade(1000, 100, 100, Side::Buy)], 200.0);
    let dot = &mut projected.dots[0];
    dot.mark.quantity = Decimal::from(100_000_000_000_000_000_000_u128);
    dot.mark.buy_quantity = Decimal::new(1, 20);
    dot.mark.buy_share = 0.0;
    let (buy_share, sell_share) = dot.side_shares();
    assert!((buy_share / 1e-40 - 1.0).abs() < 0.000001);
    assert_eq!(sell_share, 1.0);
    let (buy, sell) = dot.side_radii();
    assert!(buy > 0.0 && sell > 0.0);
    assert!((f64::from(buy) / (f64::from(dot.radius) * 1e-20) - 1.0).abs() < 0.000001);
    assert_eq!(sell, dot.radius);
}
