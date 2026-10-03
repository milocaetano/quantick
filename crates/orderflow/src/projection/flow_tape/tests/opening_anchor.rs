use super::*;
use rust_decimal::prelude::ToPrimitive as _;

fn source(trades: &[Trade]) -> FlowTapeSource {
    let mut source = FlowTapeSource::default();
    source.append(
        trades
            .iter()
            .enumerate()
            .map(|(ordinal, trade)| FlowExecution {
                ordinal,
                slot: ordinal,
                accepted_ordinal: 0,
                ticks_per_bar: Decimal::ONE,
                trade,
                opening: false,
            }),
    );
    source
}

fn wide_view(count: usize) -> FlowTapeView {
    FlowTapeView {
        end_slot: count,
        clip_right: Decimal::from(count),
        reference: FlowReference::VisibleRegions,
        ..view(count as f32 * 100.0)
    }
}

#[test]
fn only_first_daily_region_is_uncapped_and_other_opening_cells_bound_the_reference() {
    let trades = [
        trade(1000, 100, 90_000, Side::Buy),
        trade(1001, 105, 500, Side::Sell),
        trade(1100, 95, 100, Side::Buy),
        trade(1200, 100, 50, Side::Sell),
    ];
    let source = source(&trades);
    let mut view = wide_view(4);
    let excluded = source.project(
        1,
        0,
        4,
        0..4,
        view,
        FlowOpeningSelection {
            windows: &[1000],
            ordinals: &[0],
        },
    );
    assert_eq!(excluded.effective_reference, Some(500.into()));
    assert_eq!(
        excluded
            .dots
            .iter()
            .filter(|dot| dot.opening_oversized)
            .count(),
        1
    );
    assert!(excluded.dots[0].opening_anchor);
    assert_eq!(excluded.dots[1].opening_quantity, 500.into());
    assert!(!excluded.dots[1].opening_anchor);
    assert!(
        (excluded.dots[0].radius.powi(2) / excluded.dots[1].radius.powi(2) - 180.0).abs() < 0.0001
    );
    for dot in &excluded.dots[1..] {
        assert!(dot.radius <= view.radius_limit);
        assert!(
            (f64::from(dot.radius).powi(2) / 49.0 - dot.mark.quantity.to_f64().unwrap() / 500.0)
                .abs()
                < 0.00001
        );
    }
    view.exclude_opening = false;
    let included = source.project(
        1,
        0,
        4,
        0..4,
        view,
        FlowOpeningSelection {
            windows: &[1000],
            ordinals: &[0],
        },
    );
    assert_eq!(included.effective_reference, Some(90_000.into()));
    for (a, b) in excluded.dots.iter().zip(&included.dots) {
        assert_eq!(a.members, b.members);
        assert_eq!(a.mark.quantity, b.mark.quantity);
        assert_eq!(a.candle_position, b.candle_position);
        assert_eq!(a.opening_anchor, b.opening_anchor);
        assert!(!b.opening_oversized);
    }
}

#[test]
fn offscreen_or_evicted_first_anchor_never_promotes_another_opening_cell() {
    let trades = [
        trade(1000, 100, 90_000, Side::Buy),
        trade(1001, 100, 500, Side::Buy),
        trade(1100, 100, 100, Side::Buy),
    ];
    let mut source = source(&trades);
    let mut view = wide_view(3);
    view.clip_left = Decimal::ONE;
    let clipped = source.project(
        1,
        0,
        3,
        0..3,
        view,
        FlowOpeningSelection {
            windows: &[1000],
            ordinals: &[0],
        },
    );
    assert_eq!(clipped.offscreen_executions, 1);
    assert_eq!(clipped.effective_reference, Some(500.into()));
    assert!(!clipped.opening_exclusion_effective);
    assert!(clipped.dots.iter().all(|dot| !dot.opening_anchor));
    source.retain(&FlowKeep {
        slots: 1..3,
        ordinals: 1..3,
    });
    let retained = source.project(
        1,
        0,
        3,
        1..3,
        view,
        FlowOpeningSelection {
            windows: &[1000],
            ordinals: &[0],
        },
    );
    assert_eq!(retained.dots, clipped.dots);
    assert_eq!(retained.omitted_executions, 0);
    assert_eq!(retained.loaded_executions, 2);
}

#[test]
fn off_axis_anchor_does_not_influence_the_visible_scale() {
    let trades = [
        trade(1000, 200, 90_000, Side::Buy),
        trade(1001, 100, 500, Side::Buy),
    ];
    let frame = source(&trades).project(
        1,
        0,
        2,
        0..2,
        wide_view(2),
        FlowOpeningSelection {
            windows: &[1000],
            ordinals: &[0],
        },
    );
    assert_eq!(frame.off_axis_executions, 1);
    assert_eq!(frame.effective_reference, Some(500.into()));
    assert!(!frame.dots[0].opening_anchor);
    assert_eq!(frame.dots[0].radius, 7.0);
}

#[test]
fn one_region_per_date_may_exceed_the_later_ordinary_whale() {
    let trades = [
        trade(1000, 100, 90_000, Side::Buy),
        trade(1001, 105, 500, Side::Buy),
        trade(1100, 95, 5000, Side::Sell),
        trade(86_401_000, 100, 20_000, Side::Buy),
        trade(86_401_001, 105, 200, Side::Buy),
    ];
    let frame = source(&trades).project(
        1,
        0,
        5,
        0..5,
        wide_view(5),
        FlowOpeningSelection {
            windows: &[1000, 86_401_000],
            ordinals: &[0, 3],
        },
    );
    assert_eq!(frame.effective_reference, Some(5000.into()));
    let anchors = frame
        .dots
        .iter()
        .filter(|dot| dot.opening_oversized)
        .map(|dot| dot.members.iter().next().unwrap().ordinal)
        .collect::<Vec<_>>();
    assert_eq!(anchors, [0, 3]);
    assert_eq!(frame.dots[2].radius, 7.0);
    assert_eq!(
        frame
            .dots
            .iter()
            .map(|dot| dot.mark.quantity)
            .sum::<Decimal>(),
        115_700.into()
    );
}

#[test]
fn zoom_regroups_source_members_without_moving_the_canonical_anchor() {
    let trades = [
        trade(1000, 100, 9000, Side::Buy),
        trade(1100, 100, 90, Side::Buy),
        trade(1200, 100, 90, Side::Buy),
    ];
    let source = source(&trades);
    for width in [300.0, 1.0, 300.0] {
        let mut view = wide_view(3);
        view.width_px = width;
        let frame = source.project(
            1,
            0,
            3,
            0..3,
            view,
            FlowOpeningSelection {
                windows: &[1000],
                ordinals: &[0],
            },
        );
        let nominated = frame
            .dots
            .iter()
            .filter(|dot| dot.opening_anchor)
            .collect::<Vec<_>>();
        assert_eq!(nominated.len(), 1);
        assert!(
            nominated[0]
                .members
                .iter()
                .any(|member| member.ordinal == 0)
        );
        let mut members = frame
            .dots
            .iter()
            .flat_map(|dot| dot.members.iter().map(|member| member.ordinal))
            .collect::<Vec<_>>();
        members.sort_unstable();
        assert_eq!(members, [0, 1, 2]);
        assert_eq!(
            frame
                .dots
                .iter()
                .map(|dot| dot.mark.quantity)
                .sum::<Decimal>(),
            9180.into()
        );
    }
}

#[test]
fn reordered_native_cell_finds_anchor_even_when_it_is_not_the_first_source_ordinal() {
    let trades = [
        trade(1050, 100, 9000, Side::Buy),
        trade(1010, 100, 900, Side::Buy),
    ];
    let mut source = FlowTapeSource::default();
    source.append([1, 0].map(|ordinal| FlowExecution {
        ordinal,
        slot: 0,
        accepted_ordinal: ordinal,
        ticks_per_bar: 2.into(),
        trade: &trades[ordinal],
        opening: false,
    }));
    let frame = source.project(
        1,
        0,
        2,
        0..2,
        view(200.0),
        FlowOpeningSelection {
            windows: &[1000],
            ordinals: &[1],
        },
    );
    assert_eq!(frame.dots.len(), 1);
    assert!(frame.dots[0].opening_anchor);
    assert!(frame.dots[0].opening_oversized);
    assert_eq!(
        frame.dots[0]
            .members
            .iter()
            .map(|member| member.ordinal)
            .collect::<Vec<_>>(),
        [0, 1]
    );
}
