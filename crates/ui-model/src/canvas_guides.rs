//! Presentation of the workspace's docking destinations; no alternate hit rule.
use quantick_workspace::canvas::{
    self, CanvasPoint, CanvasRect, CanvasState, DockTarget, DropTarget, Edge, PaneSlot,
};

pub enum GuideMark {
    Split(Edge),
    Swap,
    Park,
}

pub struct Guide {
    pub zone: CanvasRect,
    pub badge: CanvasRect,
    pub mark: GuideMark,
    pub selected: bool,
}

pub fn guides(
    state: &CanvasState,
    area: CanvasRect,
    held: PaneSlot,
    at: CanvasPoint,
) -> Vec<Guide> {
    let panes = canvas::open_panes(state, area, Some(held));
    let visible = canvas::drop_guides(area, &panes, Some(at));
    visible
        .zones
        .into_iter()
        .map(|(target, zone)| {
            let mark = match target {
                DropTarget::Dock(
                    DockTarget::Canvas(edge) | DockTarget::Beside { side: edge, .. },
                ) => GuideMark::Split(edge),
                DropTarget::Dock(DockTarget::Swap(_)) => GuideMark::Swap,
                DropTarget::Park(_) => GuideMark::Park,
                DropTarget::Float => unreachable!("floating has no drop zone"),
            };
            let badge = CanvasRect::new(
                0.0,
                0.0,
                canvas::DOCK_GUIDE_PX.min(zone.w),
                canvas::DOCK_GUIDE_PX.min(zone.h),
            )
            .centred_in(zone);
            Guide {
                zone,
                badge,
                mark,
                selected: Some(target) == visible.selected,
            }
        })
        .collect()
}

/// The instruction plate stays entirely on the canvas and clear of every destination badge.
pub fn hint_fits(area: CanvasRect, plate: CanvasRect, guides: &[Guide]) -> bool {
    area.x <= plate.x
        && area.y <= plate.y
        && area.right() >= plate.right()
        && area.bottom() >= plate.bottom()
        && guides.iter().all(|guide| {
            let badge = guide.badge;
            plate.right() <= badge.x
                || badge.right() <= plate.x
                || plate.bottom() <= badge.y
                || badge.bottom() <= plate.y
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hint_cannot_cover_the_swap_badge_of_a_short_lower_pane() {
        let area = CanvasRect::new(0.0, 0.0, 1000.0, 340.0);
        let mut state = CanvasState::from_preset(canvas::preset("time").unwrap());
        let slot = state.add(canvas::PaneKind::Time, area).unwrap();
        state
            .dock(
                slot,
                DockTarget::Beside {
                    slot: 0,
                    side: Edge::Bottom,
                },
            )
            .unwrap();
        let added = state.add(canvas::PaneKind::Time, area).unwrap();
        let visible = guides(&state, area, added, CanvasPoint::new(500.0, 255.0));
        assert!(
            visible
                .iter()
                .any(|guide| matches!(guide.mark, GuideMark::Swap) && guide.selected)
        );
        assert!(!hint_fits(
            area,
            CanvasRect::new(350.0, 248.0, 300.0, 24.0),
            &visible
        ));
        assert!(hint_fits(
            area,
            CanvasRect::new(350.0, 111.0, 300.0, 24.0),
            &visible
        ));
    }
}
