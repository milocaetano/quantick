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

#[cfg(test)]
use crate::drawings::Drawings;
#[cfg(test)]
use crate::indicators::IndicatorViews;
use crate::surfaces::drawing_chrome::object_row::delete_all_question;

use super::PaneContextMenu;
use super::menus::PaneMenuIntent;
use quantick_chart_interaction::pane::{Effect, Intent, Model, update};

impl PaneContextMenu {
    #[cfg(test)]
    pub(crate) fn draw_object_rows(
        &mut self,
        ui: &mut egui::Ui,
        drawings: &Drawings,
        indicators: &IndicatorViews,
    ) -> Option<PaneMenuIntent> {
        #[cfg(test)]
        self.object_rects.clear();
        let entries = quantick_chart_interaction::pane_menu::object_entries(
            &super::menus::object_facts(drawings, indicators),
        );
        super::menu_renderer::render(
            ui,
            self,
            &mut Model::default(),
            &[quantick_chart_interaction::pane_menu::Entry {
                label: String::new(),
                hint: None,
                disabled: None,
                trace: quantick_chart_interaction::pane_menu::Trace::None,
                kind: quantick_chart_interaction::pane_menu::Kind::Scroll {
                    maximum_height: 320.0,
                    children: entries,
                },
            }],
            None,
        )
        .into_iter()
        .next()
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
