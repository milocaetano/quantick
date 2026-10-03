use super::*;
use crate::orderflow_render::flow_execution::flow_disc;
use quantick_engine::{Side, Trade};
use quantick_orderflow::projection::{
    PriceWindow,
    flow_tape::{
        FLOW_REGION_WINDOW_MS, FlowExecution, FlowReference, FlowTapeView, project_flow_tape,
    },
};

fn projected(quantities: &[i64], opening: bool) -> FlowTapeFrame {
    let trades = quantities
        .iter()
        .enumerate()
        .map(|(index, &quantity)| Trade {
            agg_id: index as u64,
            timestamp_ms: index as i64 * FLOW_REGION_WINDOW_MS,
            price: 100.into(),
            quantity: quantity.into(),
            side: Side::Buy,
        })
        .collect::<Vec<_>>();
    project_flow_tape(
        trades
            .iter()
            .enumerate()
            .map(|(ordinal, trade)| FlowExecution {
                ordinal,
                slot: 0,
                accepted_ordinal: ordinal,
                ticks_per_bar: trades.len().into(),
                trade,
                opening: opening && ordinal == 0,
            }),
        1,
        trades.len(),
        trades.len(),
        FlowTapeView {
            first_slot: 0,
            end_slot: 1,
            clip_left: 0.into(),
            clip_right: 1.into(),
            width_px: 1.0,
            height_px: 100.0,
            prices: PriceWindow::new(90.into(), 110.into()).unwrap(),
            reference: FlowReference::Typed(4500.into()),
            radius_limit: 12.0,
            merge_support_radius: 6.0,
            exclude_opening: opening,
        },
    )
}

fn history() -> egui::Rect {
    egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(100.0, 100.0))
}

fn source_center(dot: &FlowTapeDot, painted_center: egui::Pos2) -> egui::Pos2 {
    let translation: egui::Vec2 = flow_disc(dot, egui::Pos2::ZERO).unwrap().center.into();
    painted_center - translation
}

fn hit_region(
    frame: &FlowTapeFrame,
    history: egui::Rect,
    pointer: egui::Pos2,
    mut center: impl FnMut(&FlowTapeDot) -> Option<egui::Pos2>,
) -> Option<&FlowTapeDot> {
    quantick_chart::flow_execution::hit_flow_region(
        frame,
        [history.min.into(), history.max.into()],
        pointer.into(),
        |dot| center(dot).map(Into::into),
    )
}

#[test]
fn inspection_uses_the_large_visible_disc_instead_of_a_covered_small_centre() {
    let frame = projected(&[4500, 600], false);
    let pointer = egui::pos2(55.0, 50.0);
    let hit = hit_region(&frame, history(), pointer, |dot| {
        let painted = if dot.mark.quantity == 4500.into() {
            egui::pos2(50.0, 50.0)
        } else {
            pointer
        };
        Some(source_center(dot, painted))
    })
    .unwrap();
    assert_eq!(hit.mark.quantity, 4500.into());
    assert_eq!(hit.members.iter().next().unwrap().ordinal, 0);
}

#[test]
fn the_faint_giant_yields_to_ordinary_foreground_but_remains_inspectable_outside_it() {
    let frame = projected(&[9000, 4500], true);
    let hit_at = |pointer| {
        hit_region(&frame, history(), pointer, |dot| {
            Some(source_center(dot, egui::pos2(50.0, 50.0)))
        })
        .unwrap()
    };
    assert_eq!(hit_at(egui::pos2(55.0, 50.0)).mark.quantity, 4500.into());
    let first = hit_at(egui::pos2(65.0, 50.0));
    assert!(first.opening_oversized);
    assert_eq!(first.mark.quantity, 9000.into());
}

#[test]
fn subpixel_regions_keep_nearest_pointer_tolerance_when_no_visible_disc_is_under_it() {
    let frame = projected(&[1, 2], false);
    assert!(frame.dots.iter().all(|dot| dot.radius < 0.3));
    let hit = hit_region(&frame, history(), egui::pos2(53.0, 50.0), |dot| {
        let x = if dot.mark.quantity == 1.into() {
            50.0
        } else {
            58.0
        };
        Some(source_center(dot, egui::pos2(x, 50.0)))
    })
    .unwrap();
    assert_eq!(hit.mark.quantity, 1.into());
}
