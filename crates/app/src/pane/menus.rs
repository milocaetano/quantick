//! The pane's context menus: the entries a right-click offers, and the
//! layer checkbox all three of them share.
//!
//! Grouped because they are one reader's concern — a trader asking "what can
//! I switch here?" — and because they share [`PaneContextMenu::layer_checkbox`],
//! the single place a layer's label, hover text and disabled reason are
//! decided.
//!
//! The headless model owns capture, selection, rename and confirmation state.
//! This adapter projects the pane's read-only facts into concrete menu descriptions;
//! `menu_renderer` draws them and the pane executes their domain commands. The
//! paper ticket's trade section belongs to its tab-owned host.
//!
//! No hook is declared or read here: `pane.rs` names every `QUANTICK_*` it
//! mentions in a comment only, so the generated registry is unchanged by this
//! module existing.

use eframe::egui;

use crate::config::FeedCapabilities;
use crate::drawings::Drawings;
use crate::indicators::IndicatorViews;
use crate::paper_trading::PaperTrading;
use crate::style::ChartStyle;
use quantick_layers::{ChartLayer, LayerBlock};
use quantick_orderflow::LaneWindow;

use super::drawing_projection::PaneSeriesRead;
use super::layers::PaneLayerRead;
use super::{PaneContextMenu, PaneStrategies};

pub(crate) use quantick_chart_interaction::pane::MenuIntent as PaneMenuIntent;
use quantick_chart_interaction::pane::Model;

/// One layer's switch as the pane answers for it this frame.
#[derive(Clone, Copy)]
pub(crate) struct LayerRow {
    pub(crate) layer: ChartLayer,
    pub(crate) blocked: Option<LayerBlock>,
    pub(crate) visible: bool,
}

/// What the tape's section reads besides its layers: its lane window.
pub(crate) struct TapeMenuView {
    pub(crate) window: LaneWindow,
    pub(crate) reference_ms: Option<i64>,
}

/// The pane, read-only, as the layer menu sees it.
pub(crate) struct PaneMenuView<'a> {
    pub(crate) drawings: &'a Drawings,
    pub(crate) indicators: &'a IndicatorViews,
    pub(crate) strategies: &'a PaneStrategies,
    pub(crate) series: PaneSeriesRead<'a>,
    /// The pane layer policy used to project menu descriptions.
    pub(crate) layers: PaneLayerRead<'a>,
    pub(crate) capabilities: FeedCapabilities,
    pub(crate) style: &'a ChartStyle,
    /// FLOW's opening-scale preference, `Some` where the entry applies.
    pub(crate) flow_opening: Option<bool>,
    /// The tape's section, `Some` when the menu was opened on a tape.
    pub(crate) tape: Option<TapeMenuView>,
}

/// The layer rows a menu draws, read where the pane's layer answers are.
impl PaneLayerRead<'_> {
    /// One side's layer switches as a menu draws them, in registry order:
    /// the candles' (`on_tape == false`) or the tape's.
    pub(crate) fn rows(
        &self,
        on_tape: bool,
        capabilities: FeedCapabilities,
        style: &ChartStyle,
    ) -> Vec<LayerRow> {
        self.layers
            .registry()
            .layers()
            .iter()
            .copied()
            .filter(|layer| layer.on_tape() == on_tape)
            .map(|layer| self.row(layer, capabilities, style))
            .collect()
    }
    /// One layer's switch as a menu draws it: the answer to both questions a
    /// checkbox asks, read once.
    pub(crate) fn row(
        &self,
        layer: ChartLayer,
        capabilities: FeedCapabilities,
        style: &ChartStyle,
    ) -> LayerRow {
        LayerRow {
            layer,
            blocked: self.blocked(layer, capabilities),
            visible: self.visible(layer, style),
        }
    }
}

/// The host that still draws, and writes, its own entries in the menu.
pub(crate) struct PaneMenuHosts<'a> {
    pub(crate) paper: &'a mut PaperTrading,
}

use quantick_chart_interaction::pane::{Intent, update};
use quantick_chart_interaction::pane_menu::{
    self as menu, ChipFact, DrawingMenuFact, Facts, IndicatorFact, LayerFact, ObjectFact,
    PlaceFact, StrategyFact, StrategyPhase, TapeFact,
};

impl From<LayerRow> for LayerFact {
    fn from(row: LayerRow) -> Self {
        Self {
            layer: row.layer,
            blocked: row.blocked,
            visible: row.visible,
        }
    }
}
impl PaneMenuView<'_> {
    fn facts(&self, model: &Model) -> Facts {
        let drawing = model
            .menu
            .drawing
            .and_then(|id| self.drawings.index_of(id))
            .map(|index| {
                let drawing = &self.drawings.items()[index];
                let strategy = self
                    .strategies
                    .anchors
                    .for_drawing(drawing.id)
                    .map(|instance| {
                        use quantick_strategy::ArmedState;
                        let phase = match instance.armed.state() {
                            ArmedState::Armed | ArmedState::InPosition => StrategyPhase::Watching,
                            ArmedState::Fired { retest: true, .. } => StrategyPhase::RestingRetest,
                            ArmedState::Done | ArmedState::Disarmed { .. } => StrategyPhase::Ended,
                            ArmedState::Fired { retest: false, .. } => StrategyPhase::Fired,
                        };
                        StrategyFact {
                            status: instance.armed.status_line(),
                            phase,
                            footed: super::region_pause(drawing, self.drawings.all_hidden())
                                .is_none(),
                            span_alive: super::strategies::region_can_fire(
                                drawing,
                                self.series.closed_slots(),
                            ),
                        }
                    });
                DrawingMenuFact {
                    drawing: super::context_menu::drawing_fact(drawing),
                    label: drawing.display_label(index),
                    hidden: drawing.hidden,
                    strategy_region: drawing.tool.id() == crate::drawings::RECTANGLE_TOOL_ID
                        && drawing.band == crate::drawings::DrawingBand::Price,
                    strategy,
                }
            });
        Facts {
            layers: self
                .layers
                .rows(false, self.capabilities, self.style)
                .into_iter()
                .chain(self.layers.rows(true, self.capabilities, self.style))
                .map(Into::into)
                .collect(),
            flow_opening: self.flow_opening,
            tape: self.tape.as_ref().map(|tape| TapeFact {
                window: tape.window,
                reference_ms: tape.reference_ms,
            }),
            drawing,
            places: model
                .menu
                .places
                .iter()
                .filter_map(|&(tool_id, point)| {
                    let tool = crate::drawings::DRAWING_TOOLS
                        .into_iter()
                        .find(|tool| tool.id() == tool_id)?;
                    Some(PlaceFact {
                        tool: tool_id,
                        point,
                        label: tool.context_menu_label()?,
                        hint: tool.hover_text(),
                    })
                })
                .collect(),
            indicators: self
                .indicators
                .all()
                .iter()
                .map(|indicator| IndicatorFact {
                    slot: indicator.slot.0,
                    label: indicator.label().into(),
                    hidden: indicator.hidden,
                })
                .collect(),
            objects: object_facts(self.drawings, self.indicators),
        }
    }
}

pub(super) fn object_facts(drawings: &Drawings, indicators: &IndicatorViews) -> Vec<ObjectFact> {
    drawings
        .items()
        .iter()
        .enumerate()
        .map(|(index, drawing)| {
            let row = crate::surfaces::drawing_chrome::ManagerRow::of(
                index,
                drawing,
                drawings.selected_id() == Some(drawing.id),
                crate::bands::label_for(indicators, drawing),
            );
            let band = row.band.chip().map(|label| ChipFact {
                label,
                hint: row.band.hint().map(Into::into),
                amber: row.band.hint().is_some(),
                support: false,
            });
            ObjectFact {
                id: drawing.id,
                index,
                name: row.name,
                selected: row.selected,
                hidden: row.hidden,
                locked: row.locked,
                author: row.author,
                band,
                foreign_market: row.foreign_market,
                off_series: row.off_series,
                shared: row.shared,
            }
        })
        .collect()
}

impl PaneContextMenu {
    pub(crate) fn layer_checkbox(
        &mut self,
        ui: &mut egui::Ui,
        row: LayerRow,
    ) -> Option<PaneMenuIntent> {
        super::menu_renderer::render(
            ui,
            self,
            &mut Model::default(),
            &[menu::layer_entry(row.into())],
            None,
        )
        .into_iter()
        .next()
    }
    pub(crate) fn draw_layer_menu(
        &mut self,
        ui: &mut egui::Ui,
        view: &PaneMenuView<'_>,
        model: &mut Model,
        hosts: PaneMenuHosts<'_>,
    ) -> Vec<PaneMenuIntent> {
        let facts = view.facts(model);
        let _ = update(
            model,
            Intent::RefreshMenu {
                drawing: facts
                    .drawing
                    .as_ref()
                    .map(|drawing| drawing.drawing.clone()),
            },
        );
        #[cfg(test)]
        {
            self.menu_rects.clear();
            self.layer_menu_rects.clear();
            self.object_rects.clear();
        }
        let entries = menu::describe(model, &facts);
        super::menu_renderer::render(ui, self, model, &entries, Some(hosts.paper))
    }
}
