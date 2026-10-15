use super::*;

fn projected(trades: &[Trade], reference: FlowReference, opening: bool) -> FlowTapeFrame {
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
                opening: opening && ordinal == 0,
            }),
        1,
        trades.len(),
        trades.len(),
        FlowTapeView {
            reference,
            ..view(200.0)
        },
    )
}

#[test]
fn visible_maximum_with_floor_restores_the_previous_typed_area_formula() {
    let trades = [
        trade(1_000, 95, 1_800, Side::Buy),
        trade(2_000, 105, 900, Side::Sell),
        trade(3_000, 100, 90, Side::Buy),
    ];
    let fixed = projected(&trades, FlowReference::Typed(900.into()), false);
    let visible = projected(
        &trades,
        FlowReference::VisibleRegionsAtLeast(900.into()),
        false,
    );
    assert_eq!(visible.effective_reference, Some(1_800.into()));
    assert_eq!(
        visible.scale_basis,
        FlowScaleBasis::VisibleRegionMaxWithFloor
    );
    assert_eq!(fixed.effective_reference, Some(900.into()));
    assert_eq!(fixed.dots.iter().filter(|dot| dot.radius == 7.0).count(), 2);
    for (fixed, visible) in fixed.dots.iter().zip(&visible.dots) {
        use rust_decimal::prelude::ToPrimitive as _;
        let expected = 7.0 * (visible.mark.quantity.to_f64().unwrap() / 1_800.0).sqrt() as f32;
        assert_eq!(visible.radius, expected);
        let mut resized = fixed.clone();
        resized.radius = visible.radius;
        resized.mark.size = visible.mark.size;
        assert_eq!(
            &resized, visible,
            "only area changes, all execution facts remain exact"
        );
    }
    let small = projected(
        &trades[2..],
        FlowReference::VisibleRegionsAtLeast(900.into()),
        false,
    );
    assert_eq!(
        small.effective_reference,
        Some(900.into()),
        "the configured floor holds"
    );
    assert_eq!(small.dots[0].radius, 7.0 * 0.1_f32.sqrt());
}

#[test]
fn visible_floor_keeps_the_historical_excluded_opening_area() {
    let trades = [
        trade(1_000, 95, 3_600, Side::Buy),
        trade(2_000, 105, 900, Side::Sell),
    ];
    let visible = projected(
        &trades,
        FlowReference::VisibleRegionsAtLeast(900.into()),
        true,
    );
    assert_eq!(visible.effective_reference, Some(900.into()));
    assert!(visible.opening_exclusion_effective);
    let opening = visible.dots.iter().find(|dot| dot.opening_anchor).unwrap();
    assert!(opening.opening_oversized);
    assert_eq!(opening.radius, 14.0);
    let fixed = projected(&trades, FlowReference::Typed(900.into()), true);
    assert_eq!(
        fixed
            .dots
            .iter()
            .find(|dot| dot.opening_anchor)
            .unwrap()
            .radius,
        7.0
    );
}

#[test]
fn cached_visible_maximum_rescales_unchanged_windows_without_changing_their_facts() {
    let trades = [
        trade(1_000, 95, 900, Side::Buy),
        trade(2_000, 100, 90, Side::Sell),
        trade(3_000, 105, 3_600, Side::Buy),
    ];
    let mut source = FlowTapeSource::default();
    let execution = |ordinal: usize| FlowExecution {
        ordinal,
        slot: ordinal / 2,
        accepted_ordinal: ordinal % 2,
        ticks_per_bar: 2.into(),
        trade: &trades[ordinal],
        opening: false,
    };
    let mut view = FlowTapeView {
        reference: FlowReference::VisibleRegionsAtLeast(900.into()),
        ..view(200.0)
    };
    let openings = FlowOpeningSelection {
        windows: &[],
        ordinals: &[],
    };
    source.append((0..2).map(execution));
    let before = source.project_cached(1, 0, 3, 0..3, view, openings);
    assert_eq!(before.effective_reference, Some(900.into()));
    source.append(std::iter::once(execution(2)));
    let after = source.project_cached(1, 0, 3, 0..3, view, openings);
    assert_eq!(after, source.project(1, 0, 3, 0..3, view, openings));
    assert_eq!(after.effective_reference, Some(3_600.into()));
    for prior in before.dots {
        let current = after
            .dots
            .iter()
            .find(|dot| dot.members == prior.members)
            .unwrap();
        let mut resized = prior;
        assert_eq!(current.radius, resized.radius / 2.0);
        resized.radius = current.radius;
        resized.mark.size = current.mark.size;
        assert_eq!(&resized, current);
    }
    for reference in [
        FlowReference::Typed(900.into()),
        FlowReference::VisibleRegionsAtLeast(900.into()),
    ] {
        view.reference = reference;
        assert_eq!(
            source.project_cached(1, 0, 3, 0..3, view, openings),
            source.project(1, 0, 3, 0..3, view, openings)
        );
    }
    view.clip_right = 1.into();
    let panned = source.project_cached(1, 0, 3, 0..3, view, openings);
    assert_eq!(panned, source.project(1, 0, 3, 0..3, view, openings));
    assert_eq!(panned.effective_reference, Some(900.into()));
    source.retain(&FlowKeep {
        slots: 0..1,
        ordinals: 0..2,
    });
    view.clip_right = 2.into();
    assert_eq!(
        source.project_cached(1, 0, 3, 0..3, view, openings),
        source.project(1, 0, 3, 0..3, view, openings)
    );
}

#[test]
fn visible_floor_reconciles_one_scale_when_history_frames_are_carried() {
    let trades = [
        trade(1_000, 95, 900, Side::Buy),
        trade(2_000, 105, 3_600, Side::Sell),
    ];
    let reference = FlowReference::VisibleRegionsAtLeast(900.into());
    let complete = projected(&trades, reference, false);
    let mut carried = projected(&trades[..1], reference, false);
    let mut newest = complete.clone();
    newest
        .dots
        .retain(|dot| dot.members.iter().all(|member| member.ordinal >= 1));
    newest.covered_from_ordinal = 1;
    let union = newest.with_carried(&carried);
    assert_eq!(union.effective_reference, Some(3_600.into()));
    for dot in &union.dots {
        assert_eq!(
            dot,
            complete
                .dots
                .iter()
                .find(|full| full.members == dot.members)
                .unwrap()
        );
    }
    // A carried maximum must also raise a smaller new publication's reference.
    carried = complete.clone();
    carried
        .dots
        .retain(|dot| dot.members.iter().all(|member| member.ordinal == 1));
    for part in &mut carried.dots[0].members.parts {
        Arc::make_mut(part)[0].ordinal = 0;
    }
    newest = projected(&trades[..1], reference, false);
    for part in &mut newest.dots[0].members.parts {
        Arc::make_mut(part)[0].ordinal = 1;
    }
    newest.covered_from_ordinal = 1;
    let union = newest.with_carried(&carried);
    assert_eq!(union.effective_reference, Some(3_600.into()));
    assert_eq!(union.dots[0].radius, 3.5);
    assert_eq!(union.dots[1].radius, 7.0);
    // Carried facts may lie outside a new viewport during a history refill.
    // They remain carried, but cannot define its visible quantity reference.
    newest.view.clip_right = Decimal::new(5, 1);
    let panned = newest.with_carried(&carried);
    assert_eq!(panned.dots.len(), 2);
    assert_eq!(panned.effective_reference, Some(900.into()));
    assert_eq!(panned.dots[0].radius, 7.0);
    newest.view.clip_right = 2.into();
    newest.view.prices = PriceWindow::new(90.into(), 100.into()).unwrap();
    let off_axis = newest.with_carried(&carried);
    assert_eq!(off_axis.dots.len(), 2);
    assert_eq!(off_axis.effective_reference, Some(900.into()));
}

#[test]
fn viewport_presentation_keeps_opening_exclusion_and_never_changes_factual_groups() {
    let trades = [
        trade(1_000, 95, 3_600, Side::Buy),
        trade(2_000, 105, 900, Side::Sell),
    ];
    let source = projected(
        &trades,
        FlowReference::VisibleRegionsAtLeast(9_000.into()),
        true,
    );
    let before = source.clone();
    let displayed = source.viewport_relative(5.0, |_| true);
    assert_eq!(source, before);
    assert_eq!(displayed.effective_reference, Some(900.into()));
    assert!(displayed.opening_exclusion_effective);
    for (raw, drawn) in source.dots.iter().zip(&displayed.dots) {
        let mut expected = raw.clone();
        expected.mark.size = drawn.mark.size;
        expected.radius = drawn.radius;
        expected.opening_oversized = drawn.opening_oversized;
        assert_eq!(&expected, drawn);
        assert_eq!(drawn.radius, if drawn.opening_anchor { 10.0 } else { 5.0 });
    }
    let only_opening = source.viewport_relative(5.0, |dot| dot.opening_anchor);
    assert_eq!(only_opening.effective_reference, Some(3_600.into()));
    assert!(!only_opening.opening_exclusion_effective);
    assert_eq!(only_opening.dots[0].radius, 5.0);
}
