//! App-owned persistence and mirroring for the per-indicator mouse guide.

use super::*;

/// Set the guide through the path shared by the pane menu and control action.
pub(crate) fn set_indicator_mouse_vertical_line(
    app: &mut LayoutAdapter<'_>,
    tab_id: u64,
    pane_id: u64,
    slot: SlotId,
    enabled: bool,
) -> bool {
    let side = app.tabs.by_id(tab_id).and_then(|tab| {
        tab.panes()
            .find(|(pane, _)| pane.id == pane_id)
            .map(|(_, side)| side)
    });
    side.is_some_and(|side| {
        set_at(
            app,
            app.slot_at(tab_id, side, slot)
                .expect("addressed indicator pane"),
            enabled,
        )
    })
}

fn set_at(app: &mut LayoutAdapter<'_>, origin: TabSlot, enabled: bool) -> bool {
    // The origin refuses first: a slot off the layout, a closed view or an
    // overlay writes nothing.
    if app.edit_coordinates(origin).is_none() || !set_view(app, origin, enabled) {
        return false;
    }
    let edit = LayoutEdit::MouseVerticalLine(enabled);
    for target in app.mirror_slots(origin, edit).unwrap_or_default() {
        set_view(app, target, enabled);
    }
    app.mark_layouts_dirty();
    true
}

/// Set the guide on one view; a closed view or an overlay refuses it.
fn set_view(app: &mut LayoutAdapter<'_>, at: TabSlot, enabled: bool) -> bool {
    let pane = app.pane_mut_at(at.tab, at.side);
    let view = pane.and_then(|pane| pane.indicators.view_mut(at.slot));
    let guided = view.filter(|view| !view.descriptor.overlay);
    guided
        .map(|view| view.mouse_vertical_line = enabled)
        .is_some()
}
