use super::*;
use quantick_engine::Side;

fn print(time: i64, quantity: i64, side: Side) -> Trade {
    Trade {
        agg_id: 1,
        timestamp_ms: time,
        price: 200_000.into(),
        quantity: quantity.into(),
        side,
    }
}
fn view() -> FlowTapeView {
    FlowTapeView {
        tape_bursts: Some(FlowBurstView {
            calibration: crate::TickBurstGrouping {
                window_ms: 72_000,
                width_px: 420.0,
                price_units_per_pixel: 0.8,
                max_radius: 18.0,
            },
            from_ms: 0,
            until_ms: 144_000,
            settled_through_ms: 144_000,
        }),
        region_window_ms: 100,
        first_slot: 0,
        end_slot: 10,
        clip_left: 0.into(),
        clip_right: 10.into(),
        width_px: 800.0,
        height_px: 600.0,
        prices: PriceWindow::new(199_900.into(), 200_100.into()).unwrap(),
        reference: FlowReference::Typed(20_000.into()),
        radius_limit: 32.0,
        merge_support_radius: 6.0,
        exclude_opening: false,
    }
}
fn append(source: &mut FlowTapeSource, prints: &[Trade], order: impl Iterator<Item = usize>) {
    source.append(order.map(|ordinal| FlowExecution {
        ordinal,
        slot: ordinal,
        accepted_ordinal: 0,
        ticks_per_bar: Decimal::ONE,
        trade: &prints[ordinal],
        opening: false,
    }));
}
fn project(source: &mut FlowTapeSource, view: FlowTapeView) -> FlowTapeFrame {
    source.project(
        1,
        0,
        source.len(),
        0..source.len(),
        view,
        FlowOpeningSelection::default(),
    )
}
fn identity(frame: &FlowTapeFrame) -> Vec<(Vec<usize>, Decimal, Decimal, Decimal, u32)> {
    let mut groups: Vec<_> = frame
        .dots
        .iter()
        .map(|dot| {
            let mut members: Vec<_> = dot.members.iter().map(|member| member.ordinal).collect();
            members.sort_unstable();
            (
                members,
                dot.mark.quantity,
                dot.mark.buy_quantity,
                dot.candle_position,
                dot.radius.to_bits(),
            )
        })
        .collect();
    groups.sort();
    groups
}

#[test]
fn a_multi_window_burst_keeps_its_exact_members_and_dominates_small_prints() {
    let prints = [
        print(1_000, 1309, Side::Sell),
        print(1_100, 1291, Side::Sell),
        print(1_400, 851, Side::Buy),
        print(20_000, 1, Side::Buy),
        print(40_000, 200, Side::Buy),
    ];
    let mut source = FlowTapeSource::default();
    append(&mut source, &prints, 0..prints.len());
    let frame = project(&mut source, view());
    assert_eq!(frame.dots.len(), 3);
    let burst = frame
        .dots
        .iter()
        .find(|dot| dot.members.len() == 3)
        .unwrap();
    assert_eq!(burst.mark.quantity, Decimal::from(3451));
    assert_eq!(burst.mark.buy_quantity, Decimal::from(851));
    assert_eq!(
        burst.candle_position,
        (Decimal::from(1309) / Decimal::from(2)
            + Decimal::from(1291) * Decimal::new(15, 1)
            + Decimal::from(851) * Decimal::new(25, 1))
            / Decimal::from(3451)
    );
    let small = frame
        .dots
        .iter()
        .find(|dot| dot.mark.quantity == Decimal::ONE)
        .unwrap();
    assert!((burst.radius.powi(2) / small.radius.powi(2) - 3451.0).abs() < 0.01);
    assert!(small.radius < 0.3 && burst.radius > 13.0);
}

#[test]
fn a_native_key_crossing_candles_never_replaces_an_execution() {
    let prints = [print(1_000, 10, Side::Buy), print(1_010, 30, Side::Sell)];
    let mut source = FlowTapeSource::default();
    append(&mut source, &prints, 0..prints.len());
    let frame = project(&mut source, view());
    assert_eq!(frame.dots.len(), 1);
    let dot = &frame.dots[0];
    assert_eq!(dot.members.len(), 2);
    assert_eq!(dot.mark.quantity, Decimal::from(40));
    assert_eq!(dot.mark.buy_quantity, Decimal::from(10));
    assert_eq!(dot.candle_position, Decimal::new(125, 2));
}

#[test]
fn a_late_update_keeps_an_unrelated_settled_burst_whole() {
    let mut prints = vec![
        print(1_000, 1, Side::Buy),
        print(2_000, 1, Side::Buy),
        print(2_100, 1, Side::Sell),
    ];
    prints[0].price = 199_000.into();
    prints[2].price = 200_005.into();
    let mut source = FlowTapeSource::default();
    append(&mut source, &prints, 0..3);
    let before = project(&mut source, view());
    let group = before
        .dots
        .iter()
        .find(|dot| dot.members.len() == 2)
        .unwrap();
    let held = (
        group.mark.quantity,
        group.mark.buy_quantity,
        group.candle_position,
        group.radius,
    );
    let mut late = print(1_010, 10_000, Side::Buy);
    late.price = 199_000.into();
    prints.push(late);
    append(&mut source, &prints, 3..4);
    let after = project(&mut source, view());
    let group = after
        .dots
        .iter()
        .find(|dot| {
            let ids: Vec<_> = dot.members.iter().map(|member| member.ordinal).collect();
            ids == [1, 2]
        })
        .expect("late quantity must not split an unrelated settled burst");
    assert_eq!(
        (
            group.mark.quantity,
            group.mark.buy_quantity,
            group.candle_position,
            group.radius
        ),
        held
    );
}

#[test]
fn packet_boundaries_inside_a_native_window_never_freeze_the_preview() {
    let mut prints = [
        print(1_000, 1, Side::Buy),
        print(1_010, 1, Side::Buy),
        print(1_020, 1, Side::Sell),
        print(1_030, 10_000, Side::Buy),
        print(1_100, 2, Side::Sell),
        print(5_000, 50, Side::Buy),
        print(20_000, 100, Side::Sell),
    ];
    prints[0].price = 199_000.into();
    prints[2].price = 200_005.into();
    prints[3].price = 199_000.into();
    let mut cold = FlowTapeSource::default();
    append(&mut cold, &prints, 0..prints.len());
    let mut final_view = view();
    final_view.tape_bursts.as_mut().unwrap().settled_through_ms = 20_000;
    let expected = identity(&project(&mut cold, final_view));
    for packet in [1, 2, 3, 4] {
        let mut streamed = FlowTapeSource::default();
        for start in (0..prints.len()).step_by(packet) {
            let end = (start + packet).min(prints.len());
            append(&mut streamed, &prints, start..end);
            let mut camera = view();
            camera.tape_bursts.as_mut().unwrap().settled_through_ms =
                prints[end - 1].timestamp_ms.div_euclid(100) * 100;
            let frame = project(&mut streamed, camera);
            assert_eq!(
                frame
                    .dots
                    .iter()
                    .map(|dot| dot.members.len())
                    .sum::<usize>()
                    + frame.off_axis_executions,
                end
            );
        }
        assert_eq!(
            identity(&project(&mut streamed, final_view)),
            expected,
            "packet {packet}"
        );
    }
}

#[test]
fn incomplete_admission_cannot_seal_a_newer_packet_ahead_of_the_gap() {
    let prints = [
        print(1_000, 1, Side::Buy),
        print(1_010, 100, Side::Sell),
        print(1_100, 2, Side::Buy),
        print(2_000, 200, Side::Sell),
    ];
    let mut cold = FlowTapeSource::default();
    append(&mut cold, &prints, 0..prints.len());
    let expected = identity(&project(&mut cold, view()));
    let mut paged = FlowTapeSource::default();
    append(&mut paged, &prints, 2..4);
    let partial = paged.project(
        1,
        0,
        prints.len(),
        0..prints.len(),
        view(),
        FlowOpeningSelection::default(),
    );
    assert_eq!(partial.omitted_executions, 2);
    assert!(
        paged.bursts.blocks.is_empty(),
        "partial admission is preview-only"
    );
    append(&mut paged, &prints, 0..2);
    assert_eq!(identity(&project(&mut paged, view())), expected);
}

#[test]
fn a_gap_in_another_block_does_not_cold_rebuild_retained_late_history() {
    let mut prints = [
        print(1_000, 1, Side::Buy),
        print(2_000, 1, Side::Buy),
        print(2_100, 1, Side::Sell),
        print(1_010, 10_000, Side::Buy),
        print(73_000, 10, Side::Buy),
        print(73_010, 20, Side::Sell),
    ];
    prints[0].price = 199_000.into();
    prints[2].price = 200_005.into();
    prints[3].price = 199_000.into();
    let mut source = FlowTapeSource::default();
    let mut camera = view();
    camera.tape_bursts.as_mut().unwrap().settled_through_ms = 3_000;
    append(&mut source, &prints, 0..3);
    let _ = project(&mut source, camera);
    append(&mut source, &prints, 3..4);
    let before = identity(&project(&mut source, camera));
    assert!(before.iter().any(|group| group.0 == [1, 2]));
    let held_count = source.bursts.blocks[&0].memory.retained_group_count();
    append(&mut source, &prints, 5..6);
    let partial = source.project(
        1,
        0,
        prints.len(),
        0..prints.len(),
        view(),
        FlowOpeningSelection::default(),
    );
    assert_eq!(partial.omitted_executions, 1);
    assert!(
        before
            .iter()
            .all(|group| identity(&partial).contains(group))
    );
    let held = &source.bursts.blocks[&0];
    assert_eq!(
        held.through_ms, 3_000,
        "preview must not advance retained memory"
    );
    assert_eq!(held.memory.retained_group_count(), held_count);
    append(&mut source, &prints, 4..5);
    let complete = identity(&project(&mut source, view()));
    assert!(before.iter().all(|group| complete.contains(group)));
}

#[test]
fn an_unaligned_watermark_keeps_the_forming_native_cell_in_preview() {
    let prints = [print(1_000, 10, Side::Buy)];
    let mut source = FlowTapeSource::default();
    append(&mut source, &prints, 0..1);
    let mut camera = view();
    camera.tape_bursts.as_mut().unwrap().settled_through_ms = 1_050;
    let frame = project(&mut source, camera);
    assert_eq!(frame.dots.len(), 1);
    assert_eq!(frame.dots[0].mark.quantity, Decimal::from(10));
    assert_eq!(source.bursts.blocks[&0].memory.retained_group_count(), 0);
}

#[test]
fn camera_changes_and_remote_whales_keep_cached_group_members_and_areas() {
    let prints = [
        print(1_000, 100, Side::Sell),
        print(1_100, 200, Side::Buy),
        print(73_000, 50_000, Side::Buy),
    ];
    let mut source = FlowTapeSource::default();
    append(&mut source, &prints, 0..2);
    let before = project(&mut source, view());
    let passes = source.bursts.passes;
    let mut zoomed = view();
    zoomed.width_px = 250.0;
    zoomed.height_px = 300.0;
    assert_eq!(identity(&before), identity(&project(&mut source, zoomed)));
    assert_eq!(passes, source.bursts.passes);
    append(&mut source, &prints, 2..3);
    let after = project(&mut source, view());
    assert_eq!(after.effective_reference, before.effective_reference);
    assert!(identity(&after).contains(&identity(&before)[0]));
    assert_eq!(source.bursts.passes, passes + 1);
    let mut clipped = view();
    clipped.clip_right = Decimal::from(2);
    assert_eq!(identity(&before), identity(&project(&mut source, clipped)));
}

#[test]
fn retries_packet_order_eviction_and_block_boundaries_are_explicit() {
    let prints = [print(71_950, 10, Side::Buy), print(72_010, 30, Side::Sell)];
    let mut cold = FlowTapeSource::default();
    append(&mut cold, &prints, 0..2);
    let expected = project(&mut cold, view());
    assert_eq!(
        expected.dots.len(),
        2,
        "independent UTC blocks do not claim cross-boundary merging"
    );
    let mut chunks = FlowTapeSource::default();
    append(&mut chunks, &prints, 1..2);
    let _ = project(&mut chunks, view());
    append(&mut chunks, &prints, (0..2).rev());
    assert_eq!(identity(&expected), identity(&project(&mut chunks, view())));
    chunks.retain(&FlowKeep {
        slots: 1..2,
        ordinals: 1..2,
    });
    append(&mut chunks, &prints, 0..2);
    assert_eq!(identity(&expected), identity(&project(&mut chunks, view())));
}
