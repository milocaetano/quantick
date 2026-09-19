//! The desktop adapter for headless layer policy. Reads and effects go straight
//! to the existing feature owner; local visibility belongs to LayerState.
use super::{ChartPane, PaneChrome};
use crate::config::FeedCapabilities;
use crate::orderflow_view::OrderflowView;
use crate::style::ChartStyle;
use crate::toolrail::Tool;
use quantick_layers::{ChartLayer, LayerActions, LayerBlock};
use quantick_layers::{LayerEffect, LayerFacts, LayerSource, LayerState, VisibilityWrite};

impl ChartPane {
    pub(super) fn layer_facts(&self, capabilities: Option<FeedCapabilities>) -> LayerFacts {
        let tape = self.orderflow.as_ref();
        LayerFacts {
            flow_pane: tape.is_some(),
            tape_on: tape.is_some_and(OrderflowView::lane_enabled),
            capture_enabled: tape.is_some_and(OrderflowView::enabled),
            depth_visible: tape.is_some_and(OrderflowView::depth_visible),
            book_capture: capabilities.is_some_and(|value| value.book_capture),
            traded_volume: capabilities.is_some_and(|value| value.traded_volume),
        }
    }
    pub fn layer_switched_on(&self, layer: ChartLayer, style: &ChartStyle) -> bool {
        match layer.0.source {
            LayerSource::Local => self.layers.requested(layer),
            LayerSource::Orderflow(switch) => self
                .orderflow
                .as_ref()
                .is_some_and(|owner| owner.layer_switch(switch)),
            LayerSource::Footprint => self.footprint.visible,
            LayerSource::Grid => style.canvas.grid_enabled,
            LayerSource::Drawings => !self.drawings.all_hidden(),
        }
    }
    pub fn layer_visible(&self, layer: ChartLayer, style: &ChartStyle) -> bool {
        LayerState::visible(
            layer,
            self.layer_switched_on(layer, style),
            self.layer_facts(None),
        )
    }
    /// Common operation for menus, toolbar commands and admitted control calls.
    pub fn set_layer_visible(
        &mut self,
        layer: ChartLayer,
        visible: bool,
        actions: &mut LayerActions,
    ) {
        let Ok(Some(effect)) = self.layers.set(layer, visible) else {
            return;
        };
        self.apply_layer_effect(effect, actions);
    }
    fn apply_layer_effect(&mut self, effect: LayerEffect, actions: &mut LayerActions) {
        match effect.source {
            LayerSource::Local => unreachable!("local transitions are applied by LayerState"),
            LayerSource::Orderflow(switch) => {
                if let Some(owner) = self.orderflow.as_mut() {
                    owner.set_layer_switch(switch, effect.visible);
                }
            }
            LayerSource::Footprint => self.footprint.visible = effect.visible,
            LayerSource::Grid => actions.grid = Some(effect.visible),
            LayerSource::Drawings => match effect.write {
                VisibilityWrite::Change => self.drawings.set_all_hidden(!effect.visible),
                VisibilityWrite::Opening => self.drawings.open_all_hidden(!effect.visible),
            },
        }
    }
    pub fn layer_blocked(
        &self,
        layer: ChartLayer,
        capabilities: FeedCapabilities,
    ) -> Option<LayerBlock> {
        LayerState::blocked(layer, self.layer_facts(Some(capabilities)))
    }
    pub fn layer_effective(
        &self,
        layer: ChartLayer,
        requested: bool,
        capabilities: FeedCapabilities,
    ) -> bool {
        LayerState::effective(layer, requested, self.layer_facts(Some(capabilities)))
    }
    pub(super) fn projection_demand(&self) -> bool {
        self.layers.projection_demand(|layer| match layer.0.source {
            LayerSource::Local => self.layers.requested(layer),
            LayerSource::Orderflow(switch) => self
                .orderflow
                .as_ref()
                .is_some_and(|owner| owner.layer_switch(switch)),
            _ => false,
        })
    }
    pub fn layer_states(&self, style: &ChartStyle) -> std::collections::BTreeMap<ChartLayer, bool> {
        self.layers
            .registry()
            .layers()
            .iter()
            .copied()
            .filter(|layer| layer.persisted())
            .map(|layer| (layer, self.layer_switched_on(layer, style)))
            .collect()
    }
    pub fn layer_mask(&self, style: &ChartStyle) -> u32 {
        self.layers
            .requested_mask(|layer| self.layer_switched_on(layer, style))
    }
    pub fn apply_layer_states(&mut self, states: &std::collections::BTreeMap<ChartLayer, bool>) {
        let facts = self.layer_facts(None);
        let mut discarded = LayerActions::default();
        for (&layer, &visible) in states {
            if LayerState::restorable(layer, facts) {
                self.set_layer_visible(layer, visible, &mut discarded);
            }
        }
    }
    pub fn inherit_layer_states(&mut self, states: &std::collections::BTreeMap<ChartLayer, bool>) {
        let facts = self.layer_facts(None);
        let mut discarded = LayerActions::default();
        for (&layer, &visible) in states {
            if let Ok(Some(effect)) = self.layers.initialize(layer, visible, facts) {
                self.apply_layer_effect(effect, &mut discarded);
            }
        }
    }
    pub(super) fn unhide_layer_for_armed_tool(&mut self, chrome: &mut PaneChrome<'_>) {
        let layer = match chrome.toolrail.tool() {
            Tool::Crosshair => ChartLayer::Crosshair,
            Tool::Drawing(_) => ChartLayer::Drawings,
            Tool::Pointer => return,
        };
        if !self.layer_visible(layer, chrome.style) {
            self.set_layer_visible(layer, true, chrome.layers);
        }
    }
}
