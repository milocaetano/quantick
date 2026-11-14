//! The reusable `DrawingActionBar`: the two always-visible, textual actions
//! of a selected drawing — Lock/Unlock and Delete. Every host (inspector,
//! object manager, future context menu) renders this bar and forwards its
//! intents to the one store command for each action; none of them re-implement
//! the lock/delete rules.

use eframe::egui;

/// What the user asked the action bar to do this frame. The caller owns the
/// mutation (and its confirmation/undo flow); the bar only reports intent.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ActionBarIntent {
    pub toggle_lock: bool,
    pub delete: bool,
}

/// Draw the two-column action bar. Labels are textual by contract — the
/// destructive action is never a bare glyph — and the delete button carries
/// its keyboard shortcut.
pub fn draw(ui: &mut egui::Ui, locked: bool) -> ActionBarIntent {
    let mut intent = ActionBarIntent::default();
    let column_width = (ui.available_width() - ui.spacing().item_spacing.x) / 2.0;
    let height = ui.spacing().interact_size.y;
    ui.horizontal(|ui| {
        let lock_label = if locked {
            "Unlock drawing"
        } else {
            "Lock drawing"
        };
        if ui
            .add_sized([column_width, height], egui::Button::new(lock_label))
            .clicked()
        {
            intent.toggle_lock = true;
        }
        if ui
            .add_sized(
                [column_width, height],
                egui::Button::new("Delete drawing").shortcut_text("Del"),
            )
            .clicked()
        {
            intent.delete = true;
        }
    });
    intent
}

#[cfg(test)]
mod tests;
