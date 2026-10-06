//! One row of the drawn-objects list, shared by the object manager window and
//! the chart's right-click "objects" submenu, so the two read and act alike.
//!
//! A click only becomes a [`DrawingChromeAsk`]; the host applies both lists'
//! asks through the one controller path, so no lock or delete rule is
//! re-implemented here.

use eframe::egui;

use super::{DrawingChromeAsk, ManagerRow};
use crate::bands::BandLabel;
use crate::drawings::{Drawing, DrawingAuthor, DrawingScope};
use crate::theme;

impl ManagerRow {
    /// The row for the drawing at `index`, the one projection both lists read.
    pub(crate) fn of(index: usize, drawing: &Drawing, selected: bool, band: BandLabel) -> Self {
        Self {
            name: drawing.display_label(index),
            selected,
            locked: drawing.locked,
            hidden: drawing.hidden,
            shared: drawing.scope == DrawingScope::AllCharts,
            off_series: drawing.off_series,
            foreign_market: drawing.foreign_market,
            author: drawing.author.as_ref().map(DrawingAuthor::label),
            band,
        }
    }
}

/// Draw the row for the object at `index`: the name, its state chips and the
/// per-object actions, a click written into `ask`. Returns the test-only trace
/// of the action buttons.
pub(crate) fn object_row(
    ui: &mut egui::Ui,
    row: &ManagerRow,
    index: usize,
    ask: &mut DrawingChromeAsk,
) -> Vec<(&'static str, egui::Rect)> {
    #[cfg_attr(not(test), allow(unused_mut))]
    let mut rects = Vec::new();
    ui.horizontal(|ui| {
        let mut label = egui::RichText::new(&row.name);
        if row.hidden {
            label = label.weak();
        }
        if ui.selectable_label(row.selected, label).clicked() {
            ask.manager_select = Some(index);
        }
        if let Some(author) = &row.author {
            ui.label(
                egui::RichText::new("assistant")
                    .small()
                    .color(theme::TEXT_SUPPORT),
            )
            .on_hover_text(format!("Placed by {author}, not by you"));
        }
        if row.locked {
            ui.label(egui::RichText::new("locked").small());
        }
        if row.hidden {
            ui.label(egui::RichText::new("hidden").small());
        }
        // Which band an object is on, for the objects that are not on the
        // candles. An object nothing on screen is showing — its indicator
        // removed, hidden, collapsed or errored — is listed in amber and says
        // which of those it is. It still exists; deleting it stays the
        // trader's call.
        if let Some(chip) = row.band.chip() {
            let text = egui::RichText::new(chip).small();
            match row.band.hint() {
                Some(hint) => {
                    ui.label(text.color(theme::AMBER)).on_hover_text(hint);
                }
                None => {
                    ui.label(text);
                }
            }
        }
        if row.foreign_market {
            // The one state the chart alone cannot explain: the mark resolves
            // onto real bars, at a price that belonged to another instrument.
            ui.label(egui::RichText::new("other market").small())
                .on_hover_text(
                    "Drawn while this tab showed a different instrument. The moment still \
                     exists here; the price does not mean the same thing",
                );
        }
        if row.off_series {
            // The mark outlived the bars it was drawn on and the chart fades
            // it (§D7b). The list is where it can be found and removed, since
            // a clamped object may be nowhere near the window the trader is
            // looking at.
            ui.label(egui::RichText::new("off series").small())
                .on_hover_text(
                    "Drawn at a moment this chart's bars do not cover. It is shown at the \
                     nearest edge, faded, until you move or delete it",
                );
        }
        if row.shared {
            // Which marks are global is a question the list must answer at a
            // glance (Marina, §D7).
            ui.label(egui::RichText::new("all charts").small())
                .on_hover_text(
                    "Also drawn on the other chart of this tab, at the same moment in market \
                     time",
                );
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let mut button = |label: &'static str, text: &str| {
                let response = ui.small_button(text);
                #[cfg(test)]
                rects.push((label, response.rect));
                #[cfg(not(test))]
                let _ = label;
                response.clicked().then_some(index)
            };
            if let Some(index) = button("Delete", "Delete") {
                ask.manager_delete = Some(index);
            }
            if let Some(index) = button("Front", "Front") {
                ask.manager_bring_to_front = Some(index);
            }
            if let Some(index) = button("Lock", if row.locked { "Unlock" } else { "Lock" }) {
                ask.manager_toggle_locked = Some(index);
            }
            if let Some(index) = button("Eye", if row.hidden { "Show" } else { "Hide" }) {
                ask.manager_toggle_hidden = Some(index);
            }
        });
    });
    rects
}

/// The count-bearing gate (audit M7): deleting everything is one command, but
/// never one stray click — and locked objects go too, which the question says
/// out loud. `Some(true)` deletes, `Some(false)` keeps, `None` is unanswered.
pub(crate) fn delete_all_question(ui: &mut egui::Ui, count: usize) -> Option<bool> {
    let mut answer = None;
    ui.horizontal(|ui| {
        ui.label(format!("Delete all {count} drawing(s), locked included?"));
        if ui.button("Delete all").clicked() {
            answer = Some(true);
        }
        if ui.button("Keep").clicked() {
            answer = Some(false);
        }
    });
    answer
}
