//! The layer menu's object entries: an "objects" submenu listing what is drawn
//! on this chart — one row per object, the object manager's own row — and a
//! "clear objects…" that deletes them all behind the manager's count-bearing
//! question, as one undoable step.
//!
//! Both answer with the manager's own asks as a [`PaneMenuIntent`]; the pane
//! hands them to the drawing chrome tagged with itself, so the controller
//! applies them on the chart the right-click named (the toast, its Undo and
//! the locked-object confirmation included), never on another one.

use eframe::egui;

use crate::bands;
use crate::drawings::Drawings;
use crate::indicators::IndicatorViews;
use crate::surfaces::drawing_chrome::object_row::{delete_all_question, object_row};
use crate::surfaces::drawing_chrome::{DrawingChromeAsk, ManagerRow};

use super::PaneContextMenu;
use super::menus::{PaneMenuIntent, PaneMenuView};
use quantick_chart_interaction::pane::{Effect, Intent, Model, ObjectAction, update};

/// Tallest the objects submenu grows before its list scrolls, so thirty
/// objects never push the menu past the screen.
const OBJECTS_MENU_MAX_HEIGHT_PX: f32 = 320.0;

/// What both entries say about their reach: this pane's own objects. A mark
/// shared from the other chart is painted here but belongs to that chart.
const OBJECTS_REACH_HINT: &str = "objects drawn on this chart, its all-charts marks included \
                                  (they leave every chart); a mark shared from another chart \
                                  is listed and cleared on that chart";

impl PaneContextMenu {
    /// The two menu entries. Disabled, not absent, on an empty chart, so the
    /// menu keeps its shape and says why.
    pub(super) fn draw_objects_menu_entries(
        &mut self,
        ui: &mut egui::Ui,
        view: &PaneMenuView<'_>,
        model: &mut Model,
        intents: &mut Vec<PaneMenuIntent>,
    ) {
        let count = view.drawings.items().len();
        let enabled = count > 0;
        ui.add_enabled_ui(enabled, |ui| {
            ui.menu_button(format!("objects ({count})"), |ui| {
                intents.extend(self.draw_object_rows(ui, view.drawings, view.indicators));
            })
            .response
            .on_hover_text(OBJECTS_REACH_HINT)
            .on_disabled_hover_text("nothing is drawn on this chart");
            let clear = ui
                .button("clear objects…")
                .on_hover_text(
                    "delete every object drawn on this chart, its all-charts marks from every \
                     chart too, after a confirmation; Ctrl+Z brings them back",
                )
                .on_disabled_hover_text("nothing is drawn on this chart");
            #[cfg(test)]
            {
                self.clear_objects_rect = Some(clear.rect);
            }
            if clear.clicked() {
                let _ = update(model, Intent::AskClear { count });
                ui.close_menu();
            }
        });
    }

    /// One row per object, top-most first like the manager. The menu stays
    /// open across a row's action so several objects can go in one visit.
    pub(crate) fn draw_object_rows(
        &mut self,
        ui: &mut egui::Ui,
        drawings: &Drawings,
        indicators: &IndicatorViews,
    ) -> Option<PaneMenuIntent> {
        #[cfg(test)]
        self.object_rects.clear();
        let selected = drawings.selected();
        let mut ask = DrawingChromeAsk::default();
        egui::ScrollArea::vertical()
            .max_height(OBJECTS_MENU_MAX_HEIGHT_PX)
            .show(ui, |ui| {
                for (index, drawing) in drawings.items().iter().enumerate().rev() {
                    let row = ManagerRow::of(
                        index,
                        drawing,
                        selected == Some(index),
                        bands::label_for(indicators, drawing),
                    );
                    let rects = object_row(ui, &row, index, &mut ask);
                    #[cfg(test)]
                    self.object_rects
                        .extend(rects.into_iter().map(|(label, rect)| (index, label, rect)));
                    #[cfg(not(test))]
                    let _ = rects;
                }
            });
        let clicked = ask
            .manager_select
            .or(ask.manager_toggle_hidden)
            .or(ask.manager_toggle_locked)
            .or(ask.manager_bring_to_front)
            .or(ask.manager_delete)
            .is_some();
        clicked.then(|| {
            let action = if let Some(index) = ask.manager_select {
                ObjectAction::Select(drawings.items()[index].id)
            } else if let Some(index) = ask.manager_toggle_hidden {
                ObjectAction::ToggleHidden(drawings.items()[index].id)
            } else if let Some(index) = ask.manager_toggle_locked {
                ObjectAction::ToggleLocked(drawings.items()[index].id)
            } else if let Some(index) = ask.manager_bring_to_front {
                ObjectAction::BringToFront(drawings.items()[index].id)
            } else {
                ObjectAction::Delete(
                    drawings.items()[ask.manager_delete.expect("clicked row action")].id,
                )
            };
            PaneMenuIntent::ObjectsAsk(action)
        })
    }

    /// The question "clear objects…" raised, over this chart until answered.
    /// `count` is how many objects the chart holds; `pane` names the window.
    pub(crate) fn draw_clear_objects_confirm(
        &mut self,
        model: &mut Model,
        ctx: &egui::Context,
        chart: egui::Rect,
        count: usize,
        pane: u64,
    ) -> Option<PaneMenuIntent> {
        if !model.menu.confirm_clear {
            return None;
        }
        let mut open = count > 0;
        let mut answer = None;
        egui::Window::new("Clear objects")
            .id(egui::Id::new(("clear_objects_confirm", pane)))
            .collapsible(false)
            .resizable(false)
            .pivot(egui::Align2::CENTER_CENTER)
            .fixed_pos(chart.center())
            .open(&mut open)
            .show(ctx, |ui| answer = delete_all_question(ui, count));
        if answer.is_some() || !open {
            return update(
                model,
                Intent::AnswerClear {
                    count,
                    answer: answer == Some(true),
                },
            )
            .into_iter()
            .find_map(|effect| match effect {
                Effect::Menu(intent) => Some(intent),
                _ => None,
            });
        }
        None
    }
}
