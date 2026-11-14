//! The application's side of the shared DTO vocabulary: the conversions
//! from what the window holds to the wire shapes `quantick-control-host`
//! declares, and the one walk every projection takes over a tab's panes.

use crate::{
    pane::{ChartPane, PaneSide},
    tab::{CanvasLayout, Tab},
};

pub(crate) use quantick_control_host::wire::*;

pub(crate) fn pane_side_dto(side: PaneSide) -> PaneSideDto {
    match side {
        PaneSide::Flow => PaneSideDto::Flow,
        PaneSide::Time(_) => PaneSideDto::Time,
    }
}

/// The wire's word for `layout`: a registry entry reads as its nearest word.
pub(crate) fn layout_dto(layout: CanvasLayout) -> CanvasLayoutDto {
    match layout {
        CanvasLayout::Single => CanvasLayoutDto::Flow,
        CanvasLayout::Time => CanvasLayoutDto::Time,
        CanvasLayout::TimeAndFlow => CanvasLayoutDto::TimeAndFlow,
        CanvasLayout::TimeTimeAndFlow => CanvasLayoutDto::TimeTimeAndFlow,
        CanvasLayout::Preset(preset) => {
            layout_dto(crate::config::DeclaredLayout::nearest(preset).into())
        }
    }
}

/// The panes the active layout actually shows, in the order they are drawn.
///
/// One rule for every projection that walks a tab's canvases: a pane the
/// cursor can resolve against is a pane the scene lists, because both ask
/// here, and both ask `Tab::drawn_panes`, which the focus asks too.
pub(crate) fn visible_panes(tab: &Tab) -> Vec<(&ChartPane, PaneSide)> {
    let drawn = tab.drawn_panes();
    let time = (tab.time_panes.iter().enumerate()).map(|(slot, time)| (time, PaneSide::Time(slot)));
    let flow = std::iter::once((&tab.flow_pane, PaneSide::Flow));
    time.chain(flow)
        .filter(|(_, side)| drawn.shows(*side))
        .collect()
}

#[cfg(test)]
mod bar_wire_tests;
