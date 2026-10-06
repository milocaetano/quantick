//! One row of the drawn-objects list, shared by the object manager window and
//! the chart's right-click "objects" submenu, so the two read and act alike.
//!
//! The row only reports what was clicked; each host applies it through its
//! own path — the manager through [`super::DrawingChromeAsk`], the menu on the
//! pane's store — so no lock or delete rule is re-implemented here.

use eframe::egui;

use super::ManagerRow;
use crate::bands::BandLabel;
use crate::drawings::{Drawing, DrawingAuthor, DrawingScope};
use crate::theme;

/// What a click on one row asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RowAction {
    Select,
    ToggleHidden,
    ToggleLocked,
    BringToFront,
    Delete,
}

/// How the row's Delete treats a locked object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LockedDelete {
    /// Clickable: the host raises the locked-object confirmation.
    Confirm,
    /// Disabled with a hint: the host has nowhere to ask (a context menu).
    Disabled,
}

/// The clicked action, and the test-only trace of the row's buttons.
pub(crate) struct RowResponse {
    pub action: Option<RowAction>,
    #[cfg(test)]
    pub rects: Vec<(&'static str, egui::Rect)>,
}

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

/// Draw one row: the name, its state chips and the per-object actions.
pub(crate) fn object_row(
    ui: &mut egui::Ui,
    row: &ManagerRow,
    locked_delete: LockedDelete,
) -> RowResponse {
    let mut action = None;
    #[cfg(test)]
    let mut rects = Vec::new();
    ui.horizontal(|ui| {
        let mut label = egui::RichText::new(&row.name);
        if row.hidden {
            label = label.weak();
        }
        if ui.selectable_label(row.selected, label).clicked() {
            action = Some(RowAction::Select);
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
        // Which band an object is on, for the objects that are
        // not on the candles. An object nothing on screen is
        // showing — its indicator removed, hidden, collapsed
        // or errored — is listed in amber and says which of
        // those it is. It still exists; deleting it stays the
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
            // The one state the chart alone cannot explain:
            // the mark resolves onto real bars, at a price
            // that belonged to another instrument.
            ui.label(egui::RichText::new("other market").small())
                .on_hover_text(
                    "Drawn while this tab showed a different instrument. The                                      moment still exists here; the price does not mean the                                      same thing",
                );
        }
        if row.off_series {
            // The mark outlived the bars it was drawn on and
            // the chart fades it (§D7b). The list is where it
            // can be found and removed, since a clamped object
            // may be nowhere near the window the trader is
            // looking at.
            ui.label(egui::RichText::new("off series").small())
                .on_hover_text(
                    "Drawn at a moment this chart's bars do not cover. It is                                      shown at the nearest edge, faded, until you move or                                      delete it",
                );
        }
        if row.shared {
            // Which marks are global is a question the list
            // must answer at a glance (Marina, §D7).
            ui.label(egui::RichText::new("all charts").small())
                .on_hover_text(
                    "Also drawn on the other chart of this tab, at the same \
                     moment in market time",
                );
        }
        ui.with_layout(
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                let delete = if row.locked && locked_delete == LockedDelete::Disabled {
    ui.add_enabled(false, egui::Button::new("Delete").small())
        .on_disabled_hover_text("unlock first — a locked object never deletes by accident")
} else {
    ui.small_button("Delete")
};
                #[cfg(test)]
                rects.push(("Delete", delete.rect));
                if delete.clicked() {
                    action = Some(RowAction::Delete);
                }
                let front = ui.small_button("Front");
                #[cfg(test)]
                rects.push(("Front", front.rect));
                if front.clicked() {
                    action = Some(RowAction::BringToFront);
                }
                let lock =
                    ui.small_button(if row.locked { "Unlock" } else { "Lock" });
                #[cfg(test)]
                rects.push(("Lock", lock.rect));
                if lock.clicked() {
                    action = Some(RowAction::ToggleLocked);
                }
                let eye = ui.small_button(if row.hidden { "Show" } else { "Hide" });
                #[cfg(test)]
                rects.push(("Eye", eye.rect));
                if eye.clicked() {
                    action = Some(RowAction::ToggleHidden);
                }
            },
        );
    });
    RowResponse {
        action,
        #[cfg(test)]
        rects,
    }
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
