use super::*;
use quantick_engine::{Side, Trade};
use quantick_orderflow::projection::{
    PriceWindow,
    flow_tape::{FlowExecution, FlowReference, FlowTapeView, project_flow_tape},
};
use rust_decimal::Decimal;

const HISTORY: FlowBounds = [[0.0, 0.0], [400.0, 300.0]];

fn frame(quantities: &[i64]) -> FlowTapeFrame {
    let trades: Vec<_> = quantities
        .iter()
        .enumerate()
        .map(|(index, &quantity)| Trade {
            agg_id: index as u64 + 1,
            timestamp_ms: (index as i64 + 1) * 1000,
            price: 100.into(),
            quantity: quantity.into(),
            side: if index.is_multiple_of(2) {
                Side::Buy
            } else {
                Side::Sell
            },
        })
        .collect();
    project_flow_tape(
        trades
            .iter()
            .enumerate()
            .map(|(ordinal, trade)| FlowExecution {
                ordinal,
                slot: ordinal,
                accepted_ordinal: 0,
                ticks_per_bar: 1.into(),
                trade,
                opening: false,
            }),
        1,
        trades.len(),
        trades.len(),
        FlowTapeView {
            first_slot: 0,
            end_slot: trades.len(),
            clip_left: 0.into(),
            clip_right: trades.len().into(),
            width_px: 400.0,
            height_px: 300.0,
            prices: PriceWindow::new(99.into(), 101.into()).unwrap(),
            reference: FlowReference::VisibleRegions,
            radius_limit: 12.0,
            merge_support_radius: 6.0,
            exclude_opening: false,
        },
    )
}

fn plan(frame: &FlowTapeFrame, centers: &[[f32; 2]]) -> FlowPresentation {
    FlowPresentation::new(frame, HISTORY, |dot| {
        Some(centers[dot.mark.agg_id as usize - 1].map(|value| value + 18.0))
    })
}

#[test]
fn priority_preserves_every_member_quantity_center_and_true_area() {
    let frame = frame(&[4500, 600, 90]);
    let before = frame.clone();
    let centers = [[50.0, 100.0], [55.0, 100.0], [90.0, 100.0]];
    let presentation = plan(&frame, &centers);
    assert_eq!(frame, before);
    assert_eq!(presentation.regions.len(), 3);
    assert_eq!(
        presentation
            .regions
            .iter()
            .map(|r| (r.dot_index, r.role))
            .collect::<Vec<_>>(),
        [
            (2, FlowRegionRole::Context),
            (1, FlowRegionRole::Context),
            (0, FlowRegionRole::Peak)
        ]
    );
    let mut members = Vec::new();
    let mut totals = [Decimal::ZERO; 2];
    for region in &presentation.regions {
        let dot = &frame.dots[region.dot_index];
        assert_eq!(region.disc.center, centers[region.dot_index]);
        assert_eq!(region.disc.radius, dot.radius);
        assert_eq!(region.source_disc, region.disc);
        totals[0] += dot.mark.buy_quantity;
        totals[1] += dot.mark.quantity - dot.mark.buy_quantity;
        members.extend(dot.members.iter().map(|member| member.ordinal));
    }
    members.sort_unstable();
    assert_eq!(members, [0, 1, 2]);
    assert_eq!(totals, [4590.into(), 600.into()]);
    assert!((frame.dots[0].radius.powi(2) / frame.dots[1].radius.powi(2) - 7.5).abs() < 0.0001);
}

#[test]
fn hit_selection_uses_actual_role_order_before_quantity_or_source_position() {
    // A presentation may later displace a peak while retaining its source disc.
    // Hit testing follows the displayed circle and roles, not the source order.
    let context = FlowDisc {
        center: [116.0, 100.0],
        radius: 8.5,
    };
    let peak = FlowDisc {
        center: [127.0, 100.0],
        radius: 4.0,
    };
    let presentation = FlowPresentation {
        regions: vec![
            FlowRegionVisual {
                dot_index: 1,
                source_disc: context,
                disc: context,
                role: FlowRegionRole::Context,
                unresolved_overlap: false,
            },
            FlowRegionVisual {
                dot_index: 2,
                source_disc: FlowDisc {
                    center: [160.0, 100.0],
                    ..peak
                },
                disc: peak,
                role: FlowRegionRole::Peak,
                unresolved_overlap: false,
            },
        ],
    };
    assert_eq!(presentation.hit(HISTORY, [124.0, 100.0]), Some(2));
    assert_eq!(presentation.hit(HISTORY, [115.0, 100.0]), Some(1));
    assert_eq!(presentation.hit(HISTORY, [160.0, 100.0]), None);
}

#[test]
fn comparable_large_regions_are_not_suppressed_by_their_larger_neighbor() {
    let frame = frame(&[12780, 8771, 7209, 6617, 6455, 200]);
    let presentation = plan(&frame, &[[100.0, 100.0]; 6]);
    assert_eq!(presentation.regions[0].role, FlowRegionRole::Context);
    assert_eq!(presentation.regions[0].dot_index, 5);
    assert!(
        presentation.regions[1..]
            .iter()
            .all(|region| region.role == FlowRegionRole::Peak)
    );
    assert_eq!(
        presentation.regions[1..]
            .iter()
            .map(|r| r.dot_index)
            .collect::<Vec<_>>(),
        [4, 3, 2, 1, 0]
    );
    assert_eq!(presentation.hit(HISTORY, [100.0, 100.0]), Some(0));
}

#[test]
fn oversized_opening_neither_blocks_peaks_nor_changes_its_earned_disc() {
    let mut frame = frame(&[76016, 4500, 600]);
    frame.dots[0].opening_oversized = true;
    frame.dots[0].radius = 120.0;
    frame.effective_reference = Some(4500.into());
    let presentation = plan(&frame, &[[100.0, 100.0]; 3]);
    assert_eq!(
        presentation
            .regions
            .iter()
            .map(|r| (r.dot_index, r.role))
            .collect::<Vec<_>>(),
        [
            (0, FlowRegionRole::Opening),
            (2, FlowRegionRole::Context),
            (1, FlowRegionRole::Peak)
        ]
    );
    assert_eq!(presentation.regions[0].disc.radius, 120.0);
    assert_eq!(presentation.hit(HISTORY, [100.0, 100.0]), Some(1));
    assert_eq!(presentation.hit(HISTORY, [180.0, 100.0]), Some(0));
}

#[test]
fn camera_spacing_keeps_small_volume_dim_without_losing_its_path_area_or_inspection() {
    let frame = frame(&[4500, 600, 90, 1]);
    let before = frame.clone();
    let offsets = [[12.0, 10.0], [0.0, 0.0], [2.0, 3.0], [7.0, 6.0]];
    for x_scale in [0.5, 1.0, 2.0, 8.0, 16.0] {
        for y_scale in [0.5, 1.0, 2.0, 8.0, 16.0] {
            let centers = offsets.map(|[x, y]| [50.0 + x * x_scale, 50.0 + y * y_scale]);
            let presentation = plan(&frame, &centers);
            assert_eq!(presentation.regions.len(), 4);
            let mut members = Vec::new();
            let mut totals = [Decimal::ZERO; 2];
            for region in &presentation.regions {
                let dot = &frame.dots[region.dot_index];
                assert_eq!(
                    region.role,
                    if region.dot_index == 0 {
                        FlowRegionRole::Peak
                    } else {
                        FlowRegionRole::Context
                    },
                    "unchanged volume keeps its priority at camera scales {x_scale}, {y_scale}"
                );
                assert_eq!(region.disc.center, centers[region.dot_index]);
                assert_eq!(region.disc, region.source_disc);
                assert_eq!(region.disc.radius, dot.radius);
                members.extend(dot.members.iter().map(|member| member.ordinal));
                totals[0] += dot.mark.buy_quantity;
                totals[1] += dot.mark.quantity - dot.mark.buy_quantity;
            }
            members.sort_unstable();
            assert_eq!(members, [0, 1, 2, 3]);
            assert_eq!(totals, [4590.into(), 601.into()]);
            if x_scale >= 8.0 && y_scale >= 8.0 {
                let [x, y] = centers[3];
                assert_eq!(presentation.hit(HISTORY, [x + 3.0, y]), Some(3));
                assert_eq!(presentation.hit(HISTORY, [x + 7.0, y]), None);
            }
        }
    }
    assert_eq!(frame, before);
    assert_eq!(frame.effective_reference, Some(4500.into()));
    assert!(frame.dots[3].radius < 0.2);
}

#[test]
fn quarter_reference_priority_is_exact_in_crowded_and_separated_geometry() {
    let frame = frame(&[400, 100, 99]);
    for centers in [
        [[100.0, 100.0]; 3],
        [[50.0, 50.0], [150.0, 150.0], [250.0, 250.0]],
    ] {
        let presentation = plan(&frame, &centers);
        assert_eq!(
            presentation
                .regions
                .iter()
                .map(|region| (region.dot_index, region.role))
                .collect::<Vec<_>>(),
            [
                (2, FlowRegionRole::Context),
                (1, FlowRegionRole::Peak),
                (0, FlowRegionRole::Peak)
            ]
        );
    }
}

#[test]
fn peak_selection_is_deterministic_and_translation_invariant() {
    let frame = frame(&[600, 600, 600, 4500]);
    let centers = [
        [100.0, 100.0],
        [101.0, 100.0],
        [200.0, 100.0],
        [300.0, 100.0],
    ];
    let initial = plan(&frame, &centers);
    assert_eq!(initial, plan(&frame, &centers));
    let translated = plan(
        &frame,
        &centers.map(|point| [point[0] + 17.0, point[1] + 13.0]),
    );
    for (before, after) in initial.regions.iter().zip(&translated.regions) {
        assert_eq!(
            (before.dot_index, before.role),
            (after.dot_index, after.role)
        );
        assert_eq!(
            after.disc.center,
            [before.disc.center[0] + 17.0, before.disc.center[1] + 13.0]
        );
        assert_eq!(before.disc.radius, after.disc.radius);
    }
    assert_eq!(
        initial
            .regions
            .iter()
            .find(|r| r.dot_index == 0)
            .unwrap()
            .role,
        FlowRegionRole::Context
    );
    assert_eq!(
        initial
            .regions
            .iter()
            .find(|r| r.dot_index == 1)
            .unwrap()
            .role,
        FlowRegionRole::Context
    );
    assert_eq!(initial.regions[0].dot_index, 0);
    assert_eq!(initial.regions[1].dot_index, 1);
}

#[test]
fn invalid_or_clipped_geometry_never_becomes_a_peak_or_hit_target() {
    let empty = frame(&[]);
    let mut frame = frame(&[1; 7]);
    frame.dots[1].radius = 0.0;
    frame.dots[2].radius = f32::NAN;
    frame.dots[3].mark.quantity = Decimal::ZERO;
    let presentation = FlowPresentation::new(&frame, HISTORY, |dot| match dot.mark.agg_id {
        1 => Some([13.0, 118.0]), // Earned circle centred five pixels off the left edge.
        5 => None,
        6 => Some([f32::NAN, 0.0]),
        7 => Some([1000.0, 1000.0]),
        _ => Some([100.0, 100.0]),
    });
    assert_eq!(presentation.regions.len(), 1);
    assert_eq!(presentation.regions[0].disc.center, [-5.0, 100.0]);
    assert_eq!(presentation.hit(HISTORY, [1.0, 100.0]), Some(0));
    assert_eq!(presentation.hit(HISTORY, [-1.0, 100.0]), None);
    for bounds in [
        [[0.0, 0.0], [0.0, 100.0]],
        [[100.0, 0.0], [0.0, 100.0]],
        [[0.0, 0.0], [f32::NAN, 100.0]],
    ] {
        assert!(
            FlowPresentation::new(&frame, bounds, |_| Some([100.0, 100.0]))
                .regions
                .is_empty()
        );
    }
    assert!(
        FlowPresentation::new(&empty, HISTORY, |_| None)
            .regions
            .is_empty()
    );
}

#[test]
fn dense_coincident_regions_keep_all_volume_with_one_stable_peak() {
    let mut quantities = vec![1; 20_000];
    quantities.push(10_000);
    let frame = frame(&quantities);
    let presentation = FlowPresentation::new(&frame, HISTORY, |dot| {
        Some(if dot.mark.quantity == 1.into() {
            [118.0, 118.0]
        } else {
            [318.0, 218.0]
        })
    });
    assert_eq!(presentation.regions.len(), 20_001);
    assert_eq!(
        presentation
            .regions
            .iter()
            .filter(|r| r.role == FlowRegionRole::Peak)
            .count(),
        1
    );
    assert_eq!(presentation.regions.last().unwrap().dot_index, 20_000);
    assert_eq!(
        presentation
            .regions
            .iter()
            .map(|r| frame.dots[r.dot_index].mark.quantity)
            .sum::<Decimal>(),
        30_000.into()
    );
}

#[test]
fn many_isolated_tiny_regions_keep_their_volume_without_competing_with_a_large_peak() {
    let mut quantities = vec![1; 20_000];
    quantities.push(100_000_000);
    let frame = frame(&quantities);
    let presentation = FlowPresentation::new(&frame, HISTORY, |dot| {
        let index = dot.mark.agg_id as usize - 1;
        Some(if index == 20_000 {
            [318.0, 218.0]
        } else {
            [
                118.0 + (index % 200) as f32 * 0.005,
                118.0 + (index / 200) as f32 * 0.005,
            ]
        })
    });
    assert_eq!(presentation.regions.len(), 20_001);
    assert_eq!(
        presentation
            .regions
            .iter()
            .filter(|region| region.role == FlowRegionRole::Peak)
            .count(),
        1
    );
    assert_eq!(presentation.regions.last().unwrap().dot_index, 20_000);
    assert!(
        presentation.regions[..20_000]
            .iter()
            .all(|region| region.role == FlowRegionRole::Context)
    );
    assert!(presentation.regions[0].disc.radius < 0.002);
    assert_eq!(
        presentation
            .regions
            .iter()
            .map(|region| frame.dots[region.dot_index].mark.quantity)
            .sum::<Decimal>(),
        100_020_000.into()
    );
}

#[test]
fn absent_reference_falls_back_to_the_visible_ordinary_maximum() {
    let mut frame = frame(&[400, 100, 99]);
    frame.effective_reference = None;
    let presentation = plan(&frame, &[[100.0, 100.0]; 3]);
    assert_eq!(
        presentation
            .regions
            .iter()
            .map(|r| (r.dot_index, r.role))
            .collect::<Vec<_>>(),
        [
            (2, FlowRegionRole::Context),
            (1, FlowRegionRole::Peak),
            (0, FlowRegionRole::Peak)
        ]
    );
}

#[test]
fn displacement_keeps_source_truth_area_and_hit_testing_within_the_bound() {
    let frame = frame(&[400, 200, 1]);
    let centers = [[100.0, 100.0]; 3];
    let presentation = plan(&frame, &centers);
    assert_eq!(presentation, plan(&frame, &centers));
    let moved = presentation
        .regions
        .iter()
        .find(|region| region.dot_index == 1)
        .unwrap();
    let context = presentation
        .regions
        .iter()
        .find(|region| region.dot_index == 2)
        .unwrap();
    assert_eq!(moved.role, FlowRegionRole::Peak);
    assert_eq!(moved.source_disc.center, centers[1]);
    assert_eq!(moved.source_disc.radius, frame.dots[1].radius);
    assert_eq!(moved.disc.radius, moved.source_disc.radius);
    let dx = moved.disc.center[0] - moved.source_disc.center[0];
    let dy = moved.disc.center[1] - moved.source_disc.center[1];
    assert!(
        dx < 0.0 && dy < 0.0,
        "the first free candidate prefers above-left"
    );
    assert!(dx.hypot(dy) > 20.0 && dx.hypot(dy) <= 24.0001);
    assert!(!moved.unresolved_overlap);
    assert_eq!(presentation.hit(HISTORY, moved.disc.center), Some(1));
    assert_eq!(presentation.hit(HISTORY, moved.source_disc.center), Some(0));
    assert_eq!(context.disc, context.source_disc);
    assert_eq!(context.role, FlowRegionRole::Context);
    assert!(
        presentation
            .regions
            .iter()
            .all(|region| region.disc.visible(HISTORY))
    );
}

#[test]
fn no_free_position_keeps_the_strong_source_circle_and_reports_the_overlap() {
    let frame = frame(&[400, 400]);
    let presentation = plan(&frame, &[[100.0, 100.0]; 2]);
    assert!(
        presentation
            .regions
            .iter()
            .all(|region| region.role == FlowRegionRole::Peak)
    );
    assert!(
        presentation
            .regions
            .iter()
            .all(|region| region.disc == region.source_disc)
    );
    assert!(!presentation.regions[0].unresolved_overlap);
    assert!(presentation.regions[1].unresolved_overlap);
    assert_eq!(presentation.hit(HISTORY, [100.0, 100.0]), Some(1));
}

#[test]
fn actual_separation_at_the_source_never_moves_a_circle_for_a_cosmetic_gap() {
    let frame = frame(&[400, 400]);
    let presentation = plan(&frame, &[[100.0, 100.0], [124.0, 100.0]]);
    assert!(
        presentation
            .regions
            .iter()
            .all(|region| region.role == FlowRegionRole::Peak)
    );
    assert!(
        presentation
            .regions
            .iter()
            .all(|region| region.disc == region.source_disc)
    );
    assert!(
        presentation
            .regions
            .iter()
            .all(|region| !region.unresolved_overlap)
    );
}

#[test]
fn clipped_source_regions_are_not_moved_entirely_out_of_the_viewport() {
    let frame = frame(&[400, 200]);
    let history = [[0.0, 0.0], [1.0, 1.0]];
    let presentation = FlowPresentation::new(&frame, history, |_| Some([18.0, 18.0]));
    assert_eq!(presentation.regions.len(), 2);
    assert!(
        presentation
            .regions
            .iter()
            .all(|region| region.disc.visible(history))
    );
    assert!(
        presentation
            .regions
            .iter()
            .all(|region| region.disc == region.source_disc)
    );
    assert!(
        presentation
            .regions
            .iter()
            .any(|region| region.unresolved_overlap)
    );
}

#[test]
fn decluttering_near_an_edge_never_clips_a_previously_complete_circle() {
    let frame = frame(&[400, 200]);
    let presentation = plan(&frame, &[[20.0, 20.0]; 2]);
    let moved = presentation
        .regions
        .iter()
        .find(|region| region.dot_index == 1)
        .unwrap();
    assert_ne!(moved.disc.center, moved.source_disc.center);
    assert!(!moved.unresolved_overlap);
    for (axis, coordinate) in moved.disc.center.into_iter().enumerate() {
        assert!(coordinate - moved.disc.radius >= HISTORY[0][axis]);
        assert!(coordinate + moved.disc.radius <= HISTORY[1][axis]);
    }
}
