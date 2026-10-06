//! The layer menu's object entries: an "objects" submenu listing what is drawn
//! on this chart — one row per object, the object manager's own row — and a
//! "clear objects…" that deletes them all behind the manager's count-bearing
//! question, as one undoable step.
//!
//! Both hand their clicks to the drawing chrome as the manager's own asks,
//! tagged with this pane: the controller applies them on the chart the
//! right-click named (the toast, its Undo and the locked-object confirmation
//! included), never on another one.

use eframe::egui;

use crate::bands;
use crate::surfaces::DrawingChromeSurface;
use crate::surfaces::drawing_chrome::object_row::{delete_all_question, object_row};
use crate::surfaces::drawing_chrome::{DrawingChromeAsk, ManagerRow};

use super::ChartPane;

/// Tallest the objects submenu grows before its list scrolls, so thirty
/// objects never push the menu past the screen.
const OBJECTS_MENU_MAX_HEIGHT_PX: f32 = 320.0;

/// What both entries say about their reach: this pane's own objects. A mark
/// shared from the other chart is painted here but belongs to that chart.
const OBJECTS_REACH_HINT: &str = "objects drawn on this chart; a mark shared from the other \
                                  chart is listed and cleared there";

impl ChartPane {
    /// The two menu entries. Disabled, not absent, on an empty chart, so the
    /// menu keeps its shape and says why.
    pub(super) fn draw_objects_menu_entries(
        &mut self,
        ui: &mut egui::Ui,
        chrome: &mut DrawingChromeSurface,
    ) {
        let count = self.drawings.items().len();
        let enabled = count > 0;
        ui.add_enabled_ui(enabled, |ui| {
            ui.menu_button(format!("objects ({count})"), |ui| {
                self.draw_object_rows(ui, chrome);
            })
            .response
            .on_hover_text(OBJECTS_REACH_HINT)
            .on_disabled_hover_text("nothing is drawn on this chart");
            let clear = ui
                .button("clear objects…")
                .on_hover_text(
                    "delete every object drawn on this chart after a confirmation; Ctrl+Z \
                     brings them back",
                )
                .on_disabled_hover_text("nothing is drawn on this chart");
            #[cfg(test)]
            {
                self.context_menu.clear_objects_rect = Some(clear.rect);
            }
            if clear.clicked() {
                self.context_menu.confirm_clear = true;
                ui.close_menu();
            }
        });
    }

    /// One row per object, top-most first like the manager. The menu stays
    /// open across a row's action so several objects can go in one visit.
    pub(crate) fn draw_object_rows(
        &mut self,
        ui: &mut egui::Ui,
        chrome: &mut DrawingChromeSurface,
    ) {
        #[cfg(test)]
        self.context_menu.object_rects.clear();
        let selected = self.drawings.selected();
        let mut ask = DrawingChromeAsk::default();
        egui::ScrollArea::vertical()
            .max_height(OBJECTS_MENU_MAX_HEIGHT_PX)
            .show(ui, |ui| {
                for (index, drawing) in self.drawings.items().iter().enumerate().rev() {
                    let row = ManagerRow::of(
                        index,
                        drawing,
                        selected == Some(index),
                        bands::label_for(&self.indicators, drawing),
                    );
                    let rects = object_row(ui, &row, index, &mut ask);
                    #[cfg(test)]
                    self.context_menu
                        .object_rects
                        .extend(rects.into_iter().map(|(label, rect)| (index, label, rect)));
                    #[cfg(not(test))]
                    let _ = rects;
                }
            });
        chrome.ask_from_menu(self.id, ask);
    }

    /// The question "clear objects…" raised, over this chart until answered.
    pub(crate) fn draw_clear_objects_confirm(
        &mut self,
        ctx: &egui::Context,
        chart: egui::Rect,
        chrome: &mut DrawingChromeSurface,
    ) {
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
        if answer == Some(true) {
            chrome.ask_from_menu(
                self.id,
                DrawingChromeAsk {
                    delete_all: true,
                    ..DrawingChromeAsk::default()
                },
            );
        }
        if answer.is_some() || !open {
            self.context_menu.confirm_clear = false;
        }
    }
}
