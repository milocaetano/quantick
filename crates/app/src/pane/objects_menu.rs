//! The layer menu's object entries: an "objects" submenu listing what is drawn
//! on this chart — one row per object, the object manager's own row — and a
//! "clear objects…" that deletes them all behind the manager's count-bearing
//! question, as one undoable step.
//!
//! Both act on this pane's store directly, as the menu's drawing section does:
//! the right-click named this chart, which need not be the focused one.

use eframe::egui;

use crate::bands;
use crate::drawings;
use crate::surfaces::drawing_chrome::ManagerRow;
use crate::surfaces::drawing_chrome::object_row::{
    LockedDelete, RowAction, delete_all_question, object_row,
};

use super::ChartPane;

/// Tallest the objects submenu grows before its list scrolls, so thirty
/// objects never push the menu past the screen.
const OBJECTS_MENU_MAX_HEIGHT_PX: f32 = 320.0;

impl ChartPane {
    /// The two menu entries. Disabled, not absent, on an empty chart, so the
    /// menu keeps its shape and says why.
    pub(super) fn draw_objects_menu_entries(&mut self, ui: &mut egui::Ui) {
        let count = self.drawings.items().len();
        if count == 0 {
            ui.add_enabled(false, egui::Button::new("objects"))
                .on_disabled_hover_text("nothing is drawn on this chart");
            ui.add_enabled(false, egui::Button::new("clear objects…"))
                .on_disabled_hover_text("nothing is drawn on this chart");
            return;
        }
        let objects = ui.menu_button(format!("objects ({count})"), |ui| {
            self.draw_object_rows(ui);
        });
        self.context_menu.objects_rect = Some(objects.response.rect);
        objects
            .response
            .on_hover_text("every object on this chart: hide, lock or delete one at a time");
        let clear = ui.button("clear objects…").on_hover_text(
            "delete every object on this chart after a confirmation; Ctrl+Z brings them back",
        );
        #[cfg(test)]
        {
            self.context_menu.clear_objects_rect = Some(clear.rect);
        }
        if clear.clicked() {
            self.context_menu.confirm_clear = true;
            ui.close_menu();
        }
    }

    /// One row per object, top-most first like the manager. The menu stays
    /// open across a row's action so several objects can go in one visit.
    pub(crate) fn draw_object_rows(&mut self, ui: &mut egui::Ui) {
        #[cfg(test)]
        self.context_menu.object_rects.clear();
        let selected = self.drawings.selected();
        let mut clicked = None;
        egui::ScrollArea::vertical()
            .max_height(OBJECTS_MENU_MAX_HEIGHT_PX)
            .show(ui, |ui| {
                for index in (0..self.drawings.items().len()).rev() {
                    let drawing = &self.drawings.items()[index];
                    let row = ManagerRow::of(
                        index,
                        drawing,
                        selected == Some(index),
                        bands::label_for(&self.indicators, drawing),
                    );
                    let response = object_row(ui, &row, LockedDelete::Disabled);
                    #[cfg(test)]
                    self.context_menu.object_rects.extend(
                        response
                            .rects
                            .into_iter()
                            .map(|(label, rect)| (index, label, rect)),
                    );
                    if let Some(action) = response.action {
                        clicked = Some((index, action));
                    }
                }
            });
        if let Some((index, action)) = clicked {
            self.apply_object_row(index, action);
        }
    }

    /// A row's click, through the same store commands the drawing section uses.
    pub(crate) fn apply_object_row(&mut self, index: usize, action: RowAction) {
        let Some(drawing) = self.drawings.items().get(index) else {
            return;
        };
        let (id, hidden, locked) = (drawing.id, drawing.hidden, drawing.locked);
        match action {
            RowAction::Select => self.drawings.select(Some(index)),
            RowAction::ToggleHidden => self.drawings.set_hidden_at(index, !hidden),
            RowAction::ToggleLocked => self.drawings.set_locked_at(index, !locked),
            RowAction::BringToFront => self.drawings.bring_to_front(index),
            RowAction::Delete => {
                self.drawings.select(Some(index));
                if self.drawings.delete_selected(false) == drawings::DeleteOutcome::Deleted {
                    // The instance dies with its drawing, as on every
                    // other delete path.
                    self.strategies.remove_for_drawing(id);
                }
            }
        }
    }

    /// The question "clear objects…" raised, over this chart until answered.
    pub(crate) fn draw_clear_objects_confirm(&mut self, ctx: &egui::Context, chart: egui::Rect) {
        if !self.context_menu.confirm_clear {
            return;
        }
        let count = self.drawings.items().len();
        let mut open = count > 0;
        let mut answer = None;
        egui::Window::new("Clear objects")
            .id(egui::Id::new(("clear_objects_confirm", self.id)))
            .collapsible(false)
            .resizable(false)
            .pivot(egui::Align2::CENTER_CENTER)
            .fixed_pos(chart.center())
            .open(&mut open)
            .show(ctx, |ui| answer = delete_all_question(ui, count));
        match answer {
            Some(true) => {
                self.clear_objects();
                self.context_menu.confirm_clear = false;
            }
            Some(false) => self.context_menu.confirm_clear = false,
            None if !open => self.context_menu.confirm_clear = false,
            None => {}
        }
    }

    /// The confirmed half: every object, locked ones too, in one undo step.
    pub(crate) fn clear_objects(&mut self) -> usize {
        let deleted = self.drawings.delete_all();
        self.strategies.sweep_orphans(&self.drawings);
        deleted
    }
}
