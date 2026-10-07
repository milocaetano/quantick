//! The pane's context menus: the entries a right-click offers, and the
//! layer checkbox all three of them share.
//!
//! Grouped because they are one reader's concern — a trader asking "what can
//! I switch here?" — and because they share [`PaneContextMenu::layer_checkbox`],
//! the single place a layer's label, hover text and disabled reason are
//! decided.
//!
//! The menus are a view component: they own the menu's own state
//! ([`PaneContextMenu`]), read the pane through a [`PaneMenuView`], and answer
//! with [`PaneMenuIntent`]s. Nothing here writes the pane; the pane applies
//! every intent in one place, `ChartPane::apply_menu_intent`, so a click and a
//! scripted caller share the same operation.
//!
//! No hook is declared or read here: `pane.rs` names every `QUANTICK_*` it
//! mentions in a comment only, so the generated registry is unchanged by this
//! module existing.

use eframe::egui;

use crate::drawings::{ChartPoint, DrawingTool, Drawings};
use crate::indicator_worker::SlotId;
use crate::indicators::IndicatorViews;
use crate::paper_trading::PaperTrading;
use crate::surfaces::drawing_chrome::DrawingChromeAsk;
use crate::theme;
use quantick_layers::{ChartLayer, LayerBlock};
use quantick_orderflow::{
    LANE_WINDOW_PRESETS_MS, LaneWindow, MAX_LIVE_LANE_WINDOW_MS, MIN_LIVE_LANE_WINDOW_MS,
    lane_window_label, same_lane_window,
};

use super::drawing_projection::PaneSeriesRead;
use super::{PaneContextMenu, PaneStrategies};

/// One thing a pane menu asked the pane to do. The menus never write the
/// pane; `ChartPane::apply_menu_intent` is where each of these happens.
pub(crate) enum PaneMenuIntent {
    /// A layer's checkbox changed.
    SetLayerVisible { layer: ChartLayer, visible: bool },
    /// FLOW's "exclude first daily region from scale" changed.
    SetIgnoreFlowOpening(bool),
    /// "configure footprint…": the footprint's own window.
    OpenFootprintSettings,
    /// The tape's market-time window was chosen.
    SetLaneWindow(LaneWindow),
    /// A registry tool's placing entry, at the point its press resolved.
    Place {
        tool: DrawingTool,
        point: ChartPoint,
    },
    /// An indicator's hide/show checkbox.
    ToggleIndicatorHidden(SlotId),
    /// The right-click landed on this drawing: it is selected, like a press.
    SelectDrawing(usize),
    /// The drawing section's rename committed.
    RenameDrawing { index: usize, name: String },
    /// Lock or unlock the drawing.
    SetDrawingLocked { index: usize, locked: bool },
    /// Hide or show the drawing.
    SetDrawingHidden { index: usize, hidden: bool },
    /// Delete the drawing, and the strategy armed on it.
    DeleteDrawing(usize),
    /// An object-manager ask from the objects submenu or "clear objects…".
    ObjectsAsk(Box<DrawingChromeAsk>),
}

/// One layer's switch as the pane answers for it this frame.
#[derive(Clone, Copy)]
pub(crate) struct LayerRow {
    pub(crate) layer: ChartLayer,
    pub(crate) blocked: Option<LayerBlock>,
    pub(crate) visible: bool,
}

/// What the tape's section reads: its layers and its lane window.
pub(crate) struct TapeMenuView {
    pub(crate) layers: Vec<LayerRow>,
    pub(crate) window: LaneWindow,
    pub(crate) reference_ms: Option<i64>,
}

/// The pane, read-only, as the layer menu sees it.
pub(crate) struct PaneMenuView<'a> {
    pub(crate) drawings: &'a Drawings,
    pub(crate) indicators: &'a IndicatorViews,
    pub(crate) series: PaneSeriesRead<'a>,
    /// The candles' layer switches, in registry order.
    pub(crate) chart_layers: Vec<LayerRow>,
    /// FLOW's opening-scale preference, `Some` where the entry applies.
    pub(crate) flow_opening: Option<bool>,
    /// The tape's section, `Some` when the menu was opened on a tape.
    pub(crate) tape: Option<TapeMenuView>,
}

/// The sibling components that draw their own entries into the menu.
pub(crate) struct PaneMenuHosts<'a> {
    pub(crate) paper: &'a mut PaperTrading,
    pub(crate) strategies: &'a mut PaneStrategies,
}

impl PaneContextMenu {
    /// One layer's checkbox, wherever it is offered.
    ///
    /// Three menus show these — the candles' layer menu, the tape's, and each
    /// axis's own for the switch that belongs to it — and all three call this
    /// so a layer wears one label, one hover text and one disabled reason
    /// whichever door a trader came through.
    ///
    /// Returns why the layer could not be switched, for the caller that has a
    /// sub-entry to gate on the same answer.
    pub(crate) fn layer_checkbox(
        &mut self,
        ui: &mut egui::Ui,
        row: LayerRow,
        intents: &mut Vec<PaneMenuIntent>,
    ) -> Option<LayerBlock> {
        let LayerRow {
            layer,
            blocked,
            mut visible,
        } = row;
        let response = ui
            .horizontal(|ui| {
                let response = ui
                    .add_enabled(
                        blocked.is_none(),
                        egui::Checkbox::new(&mut visible, layer.label()),
                    )
                    .on_hover_text(layer.hint());
                if let Some(shortcut) = crate::chart_layers::shortcuts::label(layer) {
                    ui.weak(shortcut);
                }
                response
            })
            .inner;
        #[cfg(test)]
        self.layer_menu_rects.push((layer, response.rect));
        if let Some(reason) = blocked {
            response.on_disabled_hover_text(reason.explanation);
        } else if response.changed() {
            intents.push(PaneMenuIntent::SetLayerVisible { layer, visible });
        }
        blocked
    }

    /// The candles' layer checkboxes: the list the menu has always shown.
    ///
    /// The tape's own entries are filtered out by the view and drawn by
    /// [`Self::draw_tape_menu_section`] instead — one list, split by the pane
    /// each layer belongs to, so neither menu can offer a switch for the canvas
    /// beside it.
    fn draw_chart_layer_entries(
        &mut self,
        ui: &mut egui::Ui,
        view: &PaneMenuView<'_>,
        intents: &mut Vec<PaneMenuIntent>,
    ) {
        for &row in &view.chart_layers {
            let blocked = self.layer_checkbox(ui, row, intents);
            // The footprint's knobs live in a window of their own (the
            // Profitchart-style properties dialog, the boss's ask); the menu
            // offers the door. Available with the layer off too — configuring
            // before switching on is a legitimate order of operations.
            if row.layer == ChartLayer::Bubbles
                && blocked.is_none()
                && let Some(mut ignore) = view.flow_opening
            {
                ui.indent("candle_opening_scale", |ui| {
                    if ui.checkbox(&mut ignore, "Exclude first daily region from scale")
                        .on_hover_text("Exclude the opening quantity in the region containing each UTC date's first recorded trade from FLOW sizing. Only that region may exceed the ordinary maximum, with proportional area and its full volume shown. Other regions share the visible full-volume reference. The first recorded trade is not a proven auction. If no other volume is visible, use the full scale. This preference lasts for this pane and does not change Tape.")
                        .changed() {
                        intents.push(PaneMenuIntent::SetIgnoreFlowOpening(ignore));
                    }
                });
            }
            if row.layer == ChartLayer::Footprint && blocked.is_none() {
                ui.indent("footprint_configure", |ui| {
                    if ui
                        .button("configure footprint…")
                        .on_hover_text(
                            "style, band fineness, imbalance thresholds, POC and \
                             badges — in their own window",
                        )
                        .clicked()
                    {
                        intents.push(PaneMenuIntent::OpenFootprintSettings);
                        ui.close_menu();
                    }
                });
            }
        }
    }

    /// What the tape draws, and how much market time it shows.
    ///
    /// Reached by right-clicking the tape itself, which is the only place
    /// these choices are about. Every entry asks for the lane's own field, so
    /// the dock's copy of the same settings and this one can never disagree.
    fn draw_tape_menu_section(
        &mut self,
        ui: &mut egui::Ui,
        tape: &TapeMenuView,
        intents: &mut Vec<PaneMenuIntent>,
    ) {
        ui.label(
            egui::RichText::new("tape")
                .size(11.0)
                .color(theme::TEXT_MUTED),
        );

        // The same checkbox the candles' entries use, over the other half of
        // the list. Nothing here is a second copy of the tape's state: each
        // row is read through `layer_visible` and written through
        // `set_layer_visible`, which is also what puts these three in the
        // layer state file.
        for &row in &tape.layers {
            let _ = self.layer_checkbox(ui, row, intents);
        }

        let reference_ms = tape.reference_ms;
        let current = tape.window;
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
            intents.push(PaneMenuIntent::SetLaneWindow(window));
        }
    }

    /// The whole layer menu, top to bottom. Returns what the trader asked for
    /// this frame, for the pane to apply.
    pub(crate) fn draw_layer_menu(
        &mut self,
        ui: &mut egui::Ui,
        view: &PaneMenuView<'_>,
        hosts: PaneMenuHosts<'_>,
    ) -> Vec<PaneMenuIntent> {
        let mut intents = Vec::new();
        // The drawing under the click is the most specific thing the click
        // named, so its section rides above everything — including the
        // trade actions, which answer for a bare price, not an object.
        #[cfg(test)]
        self.menu_rects.clear();
        if let Some(id) = self.drawing {
            match view.drawings.index_of(id) {
                Some(index) => {
                    self.draw_drawing_menu_section(ui, view, index, hosts.strategies, &mut intents);
                    ui.separator();
                }
                // Deleted while the menu was open (undo, another surface):
                // the section vanishes instead of acting on a ghost.
                None => self.drawing = None,
            }
        }
        // Tools that place at the bar under the right-click (the anchored
        // VWAP's TradingView gesture) declare their entry on the registry;
        // the click was already resolved per tool, snap rules included, so
        // the menu only offers what the capture could honestly anchor. This
        // frequent chart action leads the general sections below it.
        if !self.places.is_empty() {
            for &(tool, point) in &self.places {
                let label = tool
                    .context_menu_label()
                    .expect("only declaring tools were captured");
                if ui.button(label).on_hover_text(tool.hover_text()).clicked() {
                    intents.push(PaneMenuIntent::Place { tool, point });
                    ui.close_menu();
                }
            }
            ui.separator();
        }
        #[cfg(test)]
        self.layer_menu_rects.clear();
        // The long candles inventory stays one submenu away on either pane.
        // It sits near the top so the right-opening menu also fits in a narrow
        // window instead of inheriting the trade section's vertical offset.
        let chart_layers = ui.menu_button("chart layers", |ui| {
            self.draw_chart_layer_entries(ui, view, &mut intents);
        });
        self.chart_layers_rect = Some(chart_layers.response.rect);
        chart_layers.response.on_hover_text(if self.on_tape {
            "what the candles beside the tape draw"
        } else {
            "what this chart draws"
        });
        self.draw_objects_menu_entries(ui, view, &mut intents);
        ui.separator();
        if let Some(price) = self.price {
            // Stable for the menu's whole life: re-reading the pointer while
            // it moves over a row would reflow the price-specific actions.
            hosts.paper.context_trade_actions(ui, price);
            ui.separator();
        }
        // The tape is a pane of its own and is configured as one: a right-click
        // on it answers for it, keeping the primary menu focused on actions.
        if self.on_tape {
            if let Some(tape) = &view.tape {
                self.draw_tape_menu_section(ui, tape, &mut intents);
            }
            ui.separator();
        }

        // Borrowed straight from the view list — no per-frame copy of the
        // labels.
        if !view.indicators.all().is_empty() {
            ui.separator();
            ui.label(
                egui::RichText::new("indicators")
                    .size(11.0)
                    .color(theme::TEXT_MUTED),
            );
            for indicator in view.indicators.all() {
                let mut visible = !indicator.hidden;
                if ui
                    .checkbox(&mut visible, indicator.label())
                    .on_hover_text("hide/show without removing (no recompute)")
                    .changed()
                {
                    intents.push(PaneMenuIntent::ToggleIndicatorHidden(indicator.slot));
                }
            }
        }
        intents
    }

    /// The per-drawing section of the layer menu: the object the
    /// right-click landed on, by name, with its own actions. This is the
    /// context-menu host `drawings/action_bar.rs` reserved a seat for.
    fn draw_drawing_menu_section(
        &mut self,
        ui: &mut egui::Ui,
        view: &PaneMenuView<'_>,
        index: usize,
        strategies: &mut PaneStrategies,
        intents: &mut Vec<PaneMenuIntent>,
    ) {
        let drawing = &view.drawings.items()[index];
        ui.label(
            egui::RichText::new(drawing.display_label(index))
                .size(11.0)
                .color(theme::TEXT_MUTED),
        );
        // Rename applies when the field loses focus (Enter included) — one
        // undo step, not one per keystroke. Whitespace clears back to the
        // derived label; the store normalises it.
        let rename = ui.add(
            egui::TextEdit::singleline(&mut self.rename)
                .hint_text("name this object")
                .desired_width(150.0),
        );
        #[cfg(test)]
        self.menu_rects.push(("Rename", rename.rect));
        if rename.lost_focus() {
            intents.push(PaneMenuIntent::RenameDrawing {
                index,
                name: self.rename.clone(),
            });
        }
        strategies.draw_menu_entries(ui, view.drawings, index, view.series, self);
        let locked = drawing.locked;
        let hidden = drawing.hidden;
        let lock = ui
            .button(if locked { "Unlock" } else { "Lock" })
            .on_hover_text("a locked object rejects geometry edits and plain deletes");
        #[cfg(test)]
        self.menu_rects
            .push((if locked { "Unlock" } else { "Lock" }, lock.rect));
        if lock.clicked() {
            intents.push(PaneMenuIntent::SetDrawingLocked {
                index,
                locked: !locked,
            });
            ui.close_menu();
        }
        let eye = ui.button(if hidden { "Show" } else { "Hide" });
        #[cfg(test)]
        self.menu_rects
            .push((if hidden { "Show" } else { "Hide" }, eye.rect));
        if eye.clicked() {
            intents.push(PaneMenuIntent::SetDrawingHidden {
                index,
                hidden: !hidden,
            });
            ui.close_menu();
        }
        let delete = if locked {
            ui.add_enabled(false, egui::Button::new("Delete"))
                .on_disabled_hover_text("unlock first — a locked object never deletes by accident")
        } else {
            let delete = ui.button("Delete");
            if delete.clicked() {
                intents.push(PaneMenuIntent::DeleteDrawing(index));
                self.drawing = None;
                ui.close_menu();
            }
            delete
        };
        #[cfg(test)]
        self.menu_rects.push(("Delete", delete.rect));
        #[cfg(not(test))]
        let _ = delete;
    }
}
