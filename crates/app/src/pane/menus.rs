//! The pane's context menus: the entries a right-click offers, and the
//! layer checkbox all three of them share.
//!
//! Grouped because they are one reader's concern — a trader asking "what can
//! I switch here?" — and because they share [`ChartPane::layer_checkbox`], the
//! single place a layer's label, hover text and disabled reason are decided.
//! The menus below are the doors; the fields they read and write live in
//! [`super::ChartPane`], which a child module sees without widening anything.
//!
//! No hook is declared or read here: `pane.rs` names every `QUANTICK_*` it
//! mentions in a comment only, so the generated registry is unchanged by this
//! module existing.

use eframe::egui;

use crate::chart_layers::{ChartLayer, LayerBlock};
use crate::drawings::{self, DrawingBand};
use crate::theme;
use quantick_orderflow::{
    LANE_WINDOW_PRESETS_MS, LaneWindow, MAX_LIVE_LANE_WINDOW_MS, MIN_LIVE_LANE_WINDOW_MS,
    lane_window_label, same_lane_window,
};

use super::{ChartPane, PaneChrome};

impl ChartPane {
    /// One layer's checkbox, wherever it is offered.
    ///
    /// Three menus show these — the candles' layer menu, the tape's, and each
    /// axis's own for the switch that belongs to it — and all three call this
    /// so a layer wears one label, one hover text and one disabled reason
    /// whichever door a trader came through. It reads and writes the field
    /// that owns the layer, never a copy.
    ///
    /// Returns why the layer could not be switched, for the caller that has a
    /// sub-entry to gate on the same answer.
    pub(super) fn layer_checkbox(
        &mut self,
        ui: &mut egui::Ui,
        layer: ChartLayer,
        chrome: &mut PaneChrome<'_>,
    ) -> Option<LayerBlock> {
        let blocked = self.layer_blocked(layer, chrome.capabilities);
        let mut visible = self.layer_visible(layer, chrome.style);
        let response = ui
            .add_enabled(
                blocked.is_none(),
                egui::Checkbox::new(&mut visible, layer.label()),
            )
            .on_hover_text(layer.hint());
        #[cfg(test)]
        self.layer_menu_rects.push((layer, response.rect));
        if let Some(reason) = blocked {
            response.on_disabled_hover_text(reason.explanation);
        } else if response.changed() {
            self.set_layer_visible(layer, visible, chrome.layers);
        }
        blocked
    }

    /// The candles' layer checkboxes: the list the menu has always shown.
    ///
    /// The tape's own entries are filtered out here and drawn by
    /// [`Self::draw_tape_menu_section`] instead — one list, split by the pane
    /// each layer belongs to, so neither menu can offer a switch for the canvas
    /// beside it.
    fn draw_chart_layer_entries(&mut self, ui: &mut egui::Ui, chrome: &mut PaneChrome<'_>) {
        for layer in ChartLayer::ALL.into_iter().filter(|layer| !layer.on_tape()) {
            let blocked = self.layer_checkbox(ui, layer, chrome);
            // The footprint's knobs live in a window of their own (the
            // Profitchart-style properties dialog, the boss's ask); the menu
            // offers the door. Available with the layer off too — configuring
            // before switching on is a legitimate order of operations.
            if layer == ChartLayer::Footprint && blocked.is_none() {
                ui.indent("footprint_configure", |ui| {
                    if ui
                        .button("configure footprint…")
                        .on_hover_text(
                            "style, band fineness, imbalance thresholds, POC and \
                             badges — in their own window",
                        )
                        .clicked()
                    {
                        chrome.layers.open_footprint_settings = true;
                        ui.close_menu();
                    }
                });
            }
        }
    }

    /// What the tape draws, and how much market time it shows.
    ///
    /// Reached by right-clicking the tape itself, which is the only place
    /// these choices are about. Every entry writes the lane's own field, so
    /// the dock's copy of the same settings and this one can never disagree.
    fn draw_tape_menu_section(&mut self, ui: &mut egui::Ui, chrome: &mut PaneChrome<'_>) {
        if self.orderflow.is_none() {
            return;
        }
        ui.label(
            egui::RichText::new("tape")
                .size(11.0)
                .color(theme::TEXT_MUTED),
        );

        // The same loop the candles' entries run, over the other half of the
        // list. Nothing here is a second copy of the tape's state: each
        // checkbox reads and writes the lane's own field through
        // `layer_visible` / `set_layer_visible`, which is also what puts these
        // three in the layer state file.
        for layer in ChartLayer::ALL.into_iter().filter(|layer| layer.on_tape()) {
            let _ = self.layer_checkbox(ui, layer, chrome);
        }

        let reference_ms = self.frame.lane_reference_ms;
        let Some(orderflow) = self.orderflow.as_mut() else {
            return;
        };
        let current = orderflow.live_lane_window();
        let mut chosen = None;
        ui.menu_button(
            format!("tape window: {}", lane_window_label(current, reference_ms)),
            |ui| {
                let mut entry = |ui: &mut egui::Ui, option: LaneWindow| {
                    if ui
                        .selectable_label(
                            same_lane_window(current, option),
                            lane_window_label(option, reference_ms),
                        )
                        .clicked()
                    {
                        chosen = Some(option);
                        ui.close_menu();
                    }
                };
                entry(ui, LaneWindow::default());
                ui.separator();
                for ms in LANE_WINDOW_PRESETS_MS {
                    entry(ui, LaneWindow::Fixed { ms });
                }
                ui.separator();
                // Custom: the same number the presets set, typed. Seconds
                // rather than milliseconds because that is the unit the choice
                // is made in; the field clamps to what the tape can draw.
                let mut seconds = match current {
                    LaneWindow::Fixed { ms } => ms,
                    LaneWindow::Auto { .. } => reference_ms
                        .map_or(MIN_LIVE_LANE_WINDOW_MS, |reference| {
                            current.resolve_ms(reference)
                        }),
                } as f64
                    / 1_000.0;
                ui.horizontal(|ui| {
                    ui.label("custom");
                    if ui
                        .add(
                            egui::DragValue::new(&mut seconds)
                                .speed(1.0)
                                .range(
                                    (MIN_LIVE_LANE_WINDOW_MS as f64 / 1_000.0)
                                        ..=(MAX_LIVE_LANE_WINDOW_MS as f64 / 1_000.0),
                                )
                                .suffix(" s"),
                        )
                        .changed()
                    {
                        chosen = Some(LaneWindow::Fixed {
                            ms: (seconds * 1_000.0).round() as i64,
                        });
                    }
                });
            },
        )
        .response
        .on_hover_text(
            "how much market time the tape shows. Following the bars keeps roughly one bar's \
             worth of flow in the band whatever the instrument; a fixed window shows that much \
             time however fast the bars are closing, so prints stay readable through a burst",
        );
        if let Some(window) = chosen {
            orderflow.set_live_lane_window(window);
        }
    }

    pub fn draw_layer_menu(&mut self, ui: &mut egui::Ui, chrome: &mut PaneChrome<'_>) {
        // The drawing under the click is the most specific thing the click
        // named, so its section rides above everything — including the
        // trade actions, which answer for a bare price, not an object.
        #[cfg(test)]
        self.gestures.menu_rects.clear();
        if let Some(id) = self.context_menu.drawing {
            match self.drawings.index_of(id) {
                Some(index) => {
                    self.draw_drawing_menu_section(ui, index);
                    ui.separator();
                }
                // Deleted while the menu was open (undo, another surface):
                // the section vanishes instead of acting on a ghost.
                None => self.context_menu.drawing = None,
            }
        }
        // The trade section rides on top, anchored at the price the
        // right-click landed on. Gated on *this pane owning the menu*, not
        // on the pointer: the menu body re-runs every frame, and a popup
        // opened near a pane's edge extends past it, so a pointer-derived
        // gate dropped the section the moment the hand travelled onto a row
        // outside the originating pane — the menu reflowing under the
        // cursor mid-reach. `ContextMenu::price` is per pane and stable for
        // the menu's whole life, which is exactly the lifetime wanted.
        if let Some(price) = self.context_menu.price {
            chrome.paper.context_trade_actions(ui, price);
            ui.separator();
        }
        // Tools that place at the bar under the right-click (the anchored
        // VWAP's TradingView gesture) declare their entry on the registry;
        // the click was already resolved per tool, snap rules included, so
        // the menu only offers what the capture could honestly anchor.
        if !self.context_menu.places.is_empty() {
            let places = std::mem::take(&mut self.context_menu.places);
            for &(tool, point) in &places {
                let label = tool
                    .context_menu_label()
                    .expect("only declaring tools were captured");
                if ui.button(label).on_hover_text(tool.hover_text()).clicked() {
                    self.place_drawing_point(tool, &DrawingBand::Price, point, chrome);
                    ui.close_menu();
                }
            }
            self.context_menu.places = places;
            ui.separator();
        }
        #[cfg(test)]
        self.layer_menu_rects.clear();
        // The tape is a pane of its own and is configured as one: a right-click
        // on it answers for it, and the candles' own layers stay one submenu
        // away rather than disappearing. A click on the candles sees exactly
        // the menu it always saw.
        if self.context_menu.on_tape {
            self.draw_tape_menu_section(ui, chrome);
            ui.separator();
            ui.menu_button("chart layers", |ui| {
                self.draw_chart_layer_entries(ui, chrome);
            })
            .response
            .on_hover_text("what the candles beside the tape draw");
        } else {
            ui.label(
                egui::RichText::new("chart layers")
                    .size(11.0)
                    .color(theme::TEXT_MUTED),
            );
            self.draw_chart_layer_entries(ui, chrome);
        }

        // Borrowed straight from the view list — no per-frame copy of the
        // labels — and the one mutation waits until the loop lets go.
        let mut toggled = None;
        if !self.indicators.all().is_empty() {
            ui.separator();
            ui.label(
                egui::RichText::new("indicators")
                    .size(11.0)
                    .color(theme::TEXT_MUTED),
            );
            for view in self.indicators.all() {
                let mut visible = !view.hidden;
                if ui
                    .checkbox(&mut visible, view.label())
                    .on_hover_text("hide/show without removing (no recompute)")
                    .changed()
                {
                    toggled = Some(view.slot);
                }
            }
        }
        if let Some(slot) = toggled {
            self.indicators.toggle_hidden(slot);
            chrome.layers.indicators_changed = true;
        }
    }

    /// The per-drawing section of the layer menu: the object the
    /// right-click landed on, by name, with its own actions. This is the
    /// context-menu host `drawings/action_bar.rs` reserved a seat for.
    fn draw_drawing_menu_section(&mut self, ui: &mut egui::Ui, index: usize) {
        let label = self.drawings.items()[index].display_label(index);
        ui.label(
            egui::RichText::new(label)
                .size(11.0)
                .color(theme::TEXT_MUTED),
        );
        // Rename applies when the field loses focus (Enter included) — one
        // undo step, not one per keystroke. Whitespace clears back to the
        // derived label; the store normalises it.
        let rename = ui.add(
            egui::TextEdit::singleline(&mut self.context_menu.rename)
                .hint_text("name this object")
                .desired_width(150.0),
        );
        #[cfg(test)]
        self.gestures.menu_rects.push(("Rename", rename.rect));
        if rename.lost_focus() {
            let name = std::mem::take(&mut self.context_menu.rename);
            self.drawings.rename_at(index, &name);
            self.context_menu.rename = name;
        }
        self.draw_strategy_menu_entries(ui, index);
        let locked = self.drawings.items()[index].locked;
        let hidden = self.drawings.items()[index].hidden;
        let lock = ui
            .button(if locked { "Unlock" } else { "Lock" })
            .on_hover_text("a locked object rejects geometry edits and plain deletes");
        #[cfg(test)]
        self.gestures
            .menu_rects
            .push((if locked { "Unlock" } else { "Lock" }, lock.rect));
        if lock.clicked() {
            self.drawings.set_locked_at(index, !locked);
            ui.close_menu();
        }
        let eye = ui.button(if hidden { "Show" } else { "Hide" });
        #[cfg(test)]
        self.gestures
            .menu_rects
            .push((if hidden { "Show" } else { "Hide" }, eye.rect));
        if eye.clicked() {
            self.drawings.set_hidden_at(index, !hidden);
            ui.close_menu();
        }
        let delete = if locked {
            ui.add_enabled(false, egui::Button::new("Delete"))
                .on_disabled_hover_text("unlock first — a locked object never deletes by accident")
        } else {
            let delete = ui.button("Delete");
            if delete.clicked() {
                let doomed = self.drawings.items()[index].id;
                self.drawings.select(Some(index));
                if self.drawings.delete_selected(false) == drawings::DeleteOutcome::Deleted {
                    // The instance dies with its drawing, immediately — not
                    // on the next closed bar, which a quiet tape may never
                    // bring.
                    self.remove_strategy_for_drawing(doomed);
                }
                self.context_menu.drawing = None;
                ui.close_menu();
            }
            delete
        };
        #[cfg(test)]
        self.gestures.menu_rects.push(("Delete", delete.rect));
        #[cfg(not(test))]
        let _ = delete;
    }
}
