//! The desktop adapter for headless layer policy. Reads and effects go straight
//! to the existing feature owner; local visibility belongs to LayerState.
//!
//! Two views over the pane's layer fields, borrowed one by one:
//! [`PaneLayerRead`] answers, [`PaneLayerWrite`] switches. The pane lends
//! them out (`ChartPane::layer_read`, `ChartPane::layer_write`) and keeps no
//! layer policy of its own.
use super::{PaneChrome, PaneFootprint};
use crate::config::FeedCapabilities;
use crate::drawings::Drawings;
use crate::orderflow_view::OrderflowView;
use crate::orderflow_view::layers::{layer_facts, layer_switch, set_layer_switch};
use crate::state::ChartState;
use crate::style::ChartStyle;
use crate::toolrail::Tool;
use quantick_chart::tick_membership::TickMembership;
use quantick_layers::{ChartLayer, LayerActions, LayerBlock};
use quantick_layers::{LayerEffect, LayerFacts, LayerSource, LayerState, VisibilityWrite};
use quantick_stores::pane_style::PanePresentation;
use std::collections::BTreeMap;

use crate::toolrail::ToolRailExt as _;
/// The fields every layer answer reads, borrowed one by one so a menu can
/// hold them beside its own state while the pane lends both out.
#[derive(Clone, Copy)]
pub(crate) struct PaneLayerRead<'a> {
    pub(super) layers: &'a LayerState,
    pub(super) orderflow: Option<&'a OrderflowView>,
    pub(super) state: &'a ChartState,
    pub(super) footprint: &'a PaneFootprint,
    pub(super) style: &'a quantick_chart::pane_style::PaneStyle,
    pub(super) drawings: &'a Drawings,
}

impl PaneLayerRead<'_> {
    pub(crate) fn facts(&self, capabilities: Option<FeedCapabilities>) -> LayerFacts {
        let tape = self.orderflow.map(OrderflowView::cached_config);
        LayerFacts {
            book_capture: capabilities.is_some_and(|value| value.book_capture),
            traded_volume: capabilities.is_some_and(|value| value.traded_volume),
            tick_bars: TickMembership::applies_to(self.state.spec()),
            native_candle_prices: self.state.tape_price_step().is_some()
                && self.state.tape_reference_price().is_some(),
            ..tape.map_or_else(LayerFacts::default, layer_facts)
        }
    }
    pub(crate) fn switched_on(&self, layer: ChartLayer, style: &ChartStyle) -> bool {
        match layer.0.source {
            LayerSource::Local => self.layers.requested(layer),
            LayerSource::Orderflow(switch) => self
                .orderflow
                .is_some_and(|owner| layer_switch(owner, switch)),
            LayerSource::Footprint => self.footprint.visible,
            LayerSource::Grid => self.style.grid(style),
            LayerSource::TimeGrid => self.style.time_grid(style),
            LayerSource::Drawings => !self.drawings.all_hidden(),
        }
    }
    pub(crate) fn visible(&self, layer: ChartLayer, style: &ChartStyle) -> bool {
        LayerState::visible(layer, self.switched_on(layer, style), self.facts(None))
    }
    pub(crate) fn blocked(
        &self,
        layer: ChartLayer,
        capabilities: FeedCapabilities,
    ) -> Option<LayerBlock> {
        LayerState::blocked(layer, self.facts(Some(capabilities)))
    }
    pub(crate) fn effective(
        &self,
        layer: ChartLayer,
        requested: bool,
        capabilities: FeedCapabilities,
    ) -> bool {
        LayerState::effective(layer, requested, self.facts(Some(capabilities)))
    }
    pub(crate) fn projection_demand(&self) -> bool {
        self.layers.projection_demand(|layer| match layer.0.source {
            LayerSource::Local => self.layers.requested(layer),
            LayerSource::Orderflow(switch) => self
                .orderflow
                .is_some_and(|owner| layer_switch(owner, switch)),
            _ => false,
        })
    }
    pub(crate) fn states(&self, style: &ChartStyle) -> BTreeMap<ChartLayer, bool> {
        self.layers
            .registry()
            .layers()
            .iter()
            .copied()
            .filter(|layer| layer.persisted())
            .map(|layer| {
                (
                    layer,
                    if layer == ChartLayer::Grid {
                        style.canvas.grid_enabled
                    } else {
                        self.switched_on(layer, style)
                    },
                )
            })
            .collect()
    }
    pub(crate) fn mask(&self, style: &ChartStyle) -> u32 {
        self.layers
            .requested_mask(|layer| self.switched_on(layer, style))
    }
}

/// The fields a layer switch writes, borrowed one by one: the headless
/// policy and every feature owner a switch lands on.
pub(crate) struct PaneLayerWrite<'a> {
    pub(super) pane: u64,
    pub(super) layers: &'a mut LayerState,
    pub(super) orderflow: Option<&'a mut OrderflowView>,
    pub(super) state: &'a ChartState,
    pub(super) presentation: &'a mut PanePresentation,
    pub(super) drawings: &'a mut Drawings,
}

impl PaneLayerWrite<'_> {
    fn read(&self) -> PaneLayerRead<'_> {
        PaneLayerRead {
            layers: self.layers,
            orderflow: self.orderflow.as_deref(),
            state: self.state,
            footprint: &self.presentation.footprint,
            style: &self.presentation.candles,
            drawings: self.drawings,
        }
    }
    /// Common operation for menus, toolbar commands and admitted control calls.
    pub(crate) fn set_visible(
        &mut self,
        layer: ChartLayer,
        visible: bool,
        actions: &mut LayerActions,
    ) {
        let Ok(Some(effect)) = self.layers.set(layer, visible) else {
            return;
        };
        self.apply_effect(effect, actions);
    }
    fn apply_effect(&mut self, effect: LayerEffect, actions: &mut LayerActions) {
        match effect.source {
            LayerSource::Local => unreachable!("local transitions are applied by LayerState"),
            LayerSource::Orderflow(switch) => {
                if let Some(owner) = self.orderflow.as_deref_mut() {
                    set_layer_switch(owner, switch, effect.visible);
                }
            }
            LayerSource::Footprint => self.presentation.footprint.visible = effect.visible,
            LayerSource::Grid => {
                actions.grid = Some(effect.visible);
                actions.grid_pane = Some(self.pane);
            }
            LayerSource::TimeGrid => {
                actions.time_grid = Some(effect.visible);
                actions.grid_pane = Some(self.pane);
            }
            LayerSource::Drawings => match effect.write {
                VisibilityWrite::Change => self.drawings.set_all_hidden(!effect.visible),
                VisibilityWrite::Opening => self.drawings.open_all_hidden(!effect.visible),
            },
        }
    }
    pub(crate) fn apply_states(&mut self, states: &BTreeMap<ChartLayer, bool>) {
        let facts = self.read().facts(None);
        let mut discarded = LayerActions::default();
        if let (Some(tape), Some(&on)) = (
            self.orderflow.as_deref_mut(),
            states.get(&ChartLayer::TapeChart),
        ) {
            tape.edit_lane(|lane| lane.restore_enabled(on));
        }
        for (&layer, &visible) in states {
            if LayerState::restorable(layer, facts) {
                self.set_visible(layer, visible, &mut discarded);
            }
        }
    }
    pub(crate) fn inherit_states(&mut self, states: &BTreeMap<ChartLayer, bool>) {
        let facts = self.read().facts(None);
        let mut discarded = LayerActions::default();
        for (&layer, &visible) in states {
            if let Ok(Some(effect)) = self.layers.initialize(layer, visible, facts) {
                self.apply_effect(effect, &mut discarded);
            }
        }
    }
    /// Arming a tool brings back the layer it draws on.
    pub(super) fn unhide_for_tool(&mut self, chrome: &mut PaneChrome<'_>) {
        let layer = match chrome.toolrail.tool() {
            Tool::Crosshair => ChartLayer::Crosshair,
            Tool::Drawing(_) => ChartLayer::Drawings,
            Tool::Pointer => return,
        };
        if !self.read().visible(layer, chrome.style) {
            self.set_visible(layer, true, chrome.layers);
        }
    }
}
