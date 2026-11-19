//! Presentation of the workspace's docking destinations; no alternate hit rule.
use quantick_workspace::canvas::{
    self, CanvasPoint, CanvasRect, CanvasState, DockTarget, DropTarget, Edge, PaneSlot,
};

pub struct Guide {
    pub zone: CanvasRect,
    pub badge: CanvasRect,
    pub label: &'static str,
    pub text_size: f32,
    pub selected: bool,
}

pub fn guides(
    state: &CanvasState,
    area: CanvasRect,
    held: PaneSlot,
    at: CanvasPoint,
) -> Vec<Guide> {
    let panes = canvas::open_panes(state, area, Some(held));
    let selected = canvas::drop_target(area, &panes, at);
    canvas::drop_guides(area, &panes, Some(at))
        .into_iter()
        .map(|(target, zone)| {
            let (label, text_size) = match target {
                DropTarget::Dock(
                    DockTarget::Canvas(edge) | DockTarget::Beside { side: edge, .. },
                ) => (
                    match edge {
                        Edge::Left => "←",
                        Edge::Right => "→",
                        Edge::Top => "↑",
                        Edge::Bottom => "↓",
                    },
                    22.0,
                ),
                DropTarget::Dock(DockTarget::Swap(_)) => ("Swap", 11.0),
                DropTarget::Park(_) => ("Park", 11.0),
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
                label,
                text_size,
                selected: target == selected,
            }
        })
        .collect()
}
