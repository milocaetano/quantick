use super::*;

fn center(rect: CanvasRect) -> CanvasPoint {
    CanvasPoint::new(rect.x + rect.w / 2.0, rect.y + rect.h / 2.0)
}

#[test]
fn each_guide_previews_the_layout_that_releasing_commits() {
    for (width, height) in [(1600.0, 900.0), (1000.0, 600.0)] {
        let area = CanvasRect::new(10.0, 40.0, width, height);
        for layout in ["time+flow", "charts-4", "time+time+flow"] {
            let mut state = CanvasState::from_preset(preset(layout).unwrap());
            let added = state.add(PaneKind::Time, area).unwrap();
            let Some(FloatHost::Canvas { rect }) = state.host(added).cloned() else {
                panic!("added pane floats");
            };
            for slot in [added, 0] {
                let held = FloatGesture::Move {
                    slot,
                    from: rect,
                    start: center(rect),
                };
                let panes = open_panes(&state, area, Some(slot));
                for (target, zone) in drop_zones(area, &panes) {
                    let DropTarget::Dock(target) = target else {
                        continue;
                    };
                    let point = center(zone);
                    assert_eq!(
                        held.release(&state, area, area, point),
                        DropOutcome::Dock(target)
                    );
                    let mut after = state.clone();
                    after.dock(slot, target).unwrap();
                    let placed = after
                        .rects(area)
                        .into_iter()
                        .find(|(id, _)| *id == slot)
                        .unwrap()
                        .1;
                    assert_eq!(held.dock_preview(&state, area, area, point), Some(placed));
                    assert_eq!(
                        state.mode(added),
                        Some(PaneMode::Floating),
                        "preview is read-only"
                    );
                }
            }
        }
    }
}

#[test]
fn only_the_hovered_pane_offers_a_compass_and_gaps_do_not_dock() {
    let area = CanvasRect::new(100.0, 50.0, 1600.0, 900.0);
    let state = CanvasState::from_preset(preset("charts-4").unwrap());
    let panes = open_panes(&state, area, None);
    let point = center(panes[0].1);
    let guides = drop_guides(area, &panes, Some(point));
    assert_eq!(guides.len(), 13);
    for (target, zone) in &guides {
        if let DropTarget::Dock(DockTarget::Swap(slot) | DockTarget::Beside { slot, .. }) = target {
            assert_eq!(*slot, panes[0].0);
            assert!(zone.w >= drop::MIN_DOCK_GUIDE_PX && zone.h >= drop::MIN_DOCK_GUIDE_PX);
            // Check all inset corners, not just the easy center.
            for x in [zone.x + 0.5, zone.right() - 0.5] {
                for y in [zone.y + 0.5, zone.bottom() - 0.5] {
                    assert_eq!(drop_target(area, &panes, CanvasPoint::new(x, y)), *target);
                }
            }
        }
    }
    let swap = guides
        .iter()
        .find(|(target, _)| matches!(target, DropTarget::Dock(DockTarget::Swap(_))))
        .unwrap()
        .1;
    let gap = CanvasPoint::new(swap.x - drop::DOCK_GUIDE_GAP_PX / 2.0, center(swap).y);
    assert_eq!(drop_target(area, &panes, gap), DropTarget::Float);
    assert_eq!(
        drop_guides(area, &panes, Some(CanvasPoint::new(-1.0, -1.0))).len(),
        8
    );
}

#[test]
fn a_small_pane_keeps_swap_readable_and_does_not_overlap_canvas_targets() {
    let area = CanvasRect::new(0.0, 0.0, 1000.0, 600.0);
    for width in [40.0, 100.0, 150.0, 200.0] {
        let pane = (7, CanvasRect::new(0.0, 0.0, width, 120.0));
        let guides = drop_zones(area, &[pane]);
        for (target, rect) in guides
            .iter()
            .filter(|(at, _)| matches!(at, DropTarget::Dock(DockTarget::Swap(_))))
        {
            assert!(rect.w >= drop::MIN_DOCK_GUIDE_PX);
            assert_eq!(drop_target(area, &[pane], center(*rect)), *target);
            assert!(rect.right() <= width && rect.bottom() <= pane.1.bottom());
        }
        assert!(
            !guides
                .iter()
                .any(|(at, _)| matches!(at, DropTarget::Dock(DockTarget::Beside { .. })))
        );
    }
}
