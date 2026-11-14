use super::{Core, Drawing, DrawingChromeAsk, DrawingEdit, InspectorActions, inspector};
use quantick_chart_interaction::pane::PaneSide;

pub struct ActionInput {
    pub actions: InspectorActions,
    pub index: usize,
    pub tab: u64,
    pub side: PaneSide,
    pub before: Option<Box<Drawing>>,
    pub gesture_settled: bool,
}
pub fn apply(
    core: &mut Core,
    inspector: &mut inspector::State,
    input: ActionInput,
) -> DrawingChromeAsk {
    let ActionInput {
        actions,
        index,
        tab,
        side,
        before: selected,
        gesture_settled,
    } = input;
    let mut ask = DrawingChromeAsk::default();
    // The undo baseline, taken *before* the edit this frame asked for lands.
    // A copy made afterwards would equal the new state and the change would
    // never reach the history. `env` still describes the object as it was at
    // the top of the frame, which is exactly the copy that is wanted — and it
    // costs its allocation only on the frame a gesture opens.
    if actions.edited
        && core.shared.edit_baseline.is_none()
        && let Some(selected) = selected
    {
        core.shared.edit_baseline = Some(DrawingEdit {
            tab,
            side,
            index,
            before: *selected,
        });
    }
    if gesture_settled {
        ask.commit_edit_gesture = core.shared.edit_baseline.take().map(Box::new);
    }
    ask.toggle_selected_hidden |= actions.toggle_hidden;
    if actions.toggle_lock {
        ask.toggle_selected_locked = true;
        core.shared.delete_confirm = false;
    }
    if actions.toggle_pin {
        core.shared.pinned = !core.shared.pinned;
        // The user has expressed a preference: the auto-pin width rule stops
        // firing for the rest of the session.
        inspector.pin_touched = true;
        if !core.shared.pinned {
            // Unpinning re-opens the floating window. The pinned host has been
            // claiming the selection each frame, so treat it as fresh again —
            // otherwise automatic placement never runs and the window falls
            // back to the fixed default corner.
            core.shared.last_selection = None;
            inspector.settle_frame = true;
        }
    }
    ask.request_delete |= actions.delete;
    ask.cancel_delete |= actions.cancel_delete;
    if actions.force_delete {
        core.shared.delete_confirm = false;
        ask.force_delete = true;
    }
    if actions.close {
        // Closing the panel is a smaller act than it used to be: it puts the
        // trader back on the context bar, with the object still selected.
        // Clearing the selection here would make the X the only control that
        // answers a bigger question than it asks.
        core.shared.open = false;
        if core.shared.pinned {
            core.shared.pinned = false;
            inspector.pin_touched = true;
        }
        core.shared.delete_confirm = false;
    }
    // Nothing to undo: this changed a preference, not the chart.
    ask.saved_default = actions.saved_default;
    ask
}
