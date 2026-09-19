//! App-owned persistence and mirroring for the per-indicator mouse guide.

use super::*;

pub(super) fn queue_saved(app: &mut LayoutAdapter<'_>, owner: TabSlot, entry: &SavedIndicator) {
    if entry.mouse_vertical_line {
        app.indicators.pending_mouse_vertical_lines.push(owner);
    }
}

pub(super) fn apply_pending(app: &mut LayoutAdapter<'_>) {
    if app.indicators.pending_mouse_vertical_lines.is_empty() {
        return;
    }
    let pending = std::mem::take(&mut app.indicators.pending_mouse_vertical_lines);
    for owner in pending {
        let known = app
            .indicators
            .slot_kinds
            .iter()
            .any(|(candidate, _)| *candidate == owner);
        match app
            .pane_mut_at(owner.tab, owner.side)
            .and_then(|pane| pane.indicators.view_mut(owner.slot))
        {
            Some(view) if !view.descriptor.overlay => view.mouse_vertical_line = true,
            None if known => app.indicators.pending_mouse_vertical_lines.push(owner),
            Some(_) | None => {}
        }
    }
}

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
            TabSlot {
                tab: tab_id,
                side,
                slot,
            },
            enabled,
        )
    })
}

fn set_at(app: &mut LayoutAdapter<'_>, origin: TabSlot, enabled: bool) -> bool {
    let Some((layout, index)) = app.edit_coordinates(origin) else {
        return false;
    };
    if let Some(view) = app
        .pane_mut_at(origin.tab, origin.side)
        .and_then(|pane| pane.indicators.view_mut(origin.slot))
        .filter(|view| !view.descriptor.overlay)
    {
        view.mouse_vertical_line = enabled;
    } else {
        return false;
    };
    if let Some(entry) = app.layout_entry_mut(layout, index) {
        entry.mouse_vertical_line = enabled;
    }
    for (tab, side) in app.mirror_targets(origin, layout) {
        if let Some(slot) = app.layout_slots_at(tab, side).get(index).copied()
            && let Some(view) = app
                .pane_mut_at(tab, side)
                .and_then(|pane| pane.indicators.view_mut(slot))
            && !view.descriptor.overlay
        {
            view.mouse_vertical_line = enabled;
        }
    }
    app.mark_layouts_dirty();
    true
}
