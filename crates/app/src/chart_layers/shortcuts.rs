//! Keyboard bindings and visible hints for candle layers.
use super::ChartLayer;
use eframe::egui;
use quantick_orderflow::LaneView;

/// On a focused flow pane: the tape alone, and from there back beside the
/// candles. Read before the new-tab shortcut, whose `Ctrl+T` it would also match.
pub(crate) const TAPE_VIEW_SHORTCUT: egui::KeyboardShortcut = egui::KeyboardShortcut::new(
    egui::Modifiers::CTRL.plus(egui::Modifiers::SHIFT),
    egui::Key::T,
);

/// The tape key's mirror, on a focused flow pane: the trail alone, and from
/// there back beside the tape. Read before the replay shortcut, whose `Ctrl+R` it
/// would also match.
pub(crate) const TRAIL_VIEW_SHORTCUT: egui::KeyboardShortcut = egui::KeyboardShortcut::new(
    egui::Modifiers::CTRL.plus(egui::Modifiers::SHIFT),
    egui::Key::R,
);

/// Opens and closes the order-flow panel of a tab showing its flow pane. Read
/// before any `Ctrl+O` could match it too.
pub(crate) const ORDERFLOW_SETTINGS_SHORTCUT: egui::KeyboardShortcut = egui::KeyboardShortcut::new(
    egui::Modifiers::CTRL.plus(egui::Modifiers::SHIFT),
    egui::Key::O,
);

/// Consumes both view keys; answers the move a fresh press of one asks for.
pub(crate) fn take_view_key(ctx: &egui::Context) -> Option<fn(LaneView) -> LaneView> {
    let tape = take_view_press(ctx, TAPE_VIEW_SHORTCUT);
    let trail = take_view_press(ctx, TRAIL_VIEW_SHORTCUT);
    match (tape, trail) {
        (true, _) => Some(LaneView::tape_key),
        (false, true) => Some(LaneView::trail_key),
        (false, false) => None,
    }
}

/// The key that speaks for this side's switch.
pub(crate) fn side_shortcut(side: quantick_orderflow::LaneSide) -> egui::KeyboardShortcut {
    match side {
        quantick_orderflow::LaneSide::Trail => TRAIL_VIEW_SHORTCUT,
        quantick_orderflow::LaneSide::Tape => TAPE_VIEW_SHORTCUT,
    }
}

/// Consumes a panel key however it is held; answers whether it asks for a
/// change: only a fresh press outside a text field, as a held key's repeats
/// would strobe the views.
pub(crate) fn take_view_press(ctx: &egui::Context, shortcut: egui::KeyboardShortcut) -> bool {
    let fresh = ctx.input_mut(|input| {
        let fresh = input.events.iter().any(|event| {
            matches!(event, egui::Event::Key {
                key,
                modifiers,
                pressed: true,
                repeat: false,
                ..
            } if *key == shortcut.logical_key && modifiers.matches_logically(shortcut.modifiers))
        });
        input.consume_shortcut(&shortcut);
        fresh
    });
    fresh && !ctx.wants_keyboard_input()
}

pub(crate) fn binding(layer: ChartLayer) -> Option<egui::KeyboardShortcut> {
    let key = match layer {
        ChartLayer::Bubbles => egui::Key::B,
        ChartLayer::Footprint => egui::Key::F,
        _ => return None,
    };
    Some(egui::KeyboardShortcut::new(egui::Modifiers::CTRL, key))
}
