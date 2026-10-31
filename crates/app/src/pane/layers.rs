//! The desktop adapter for headless layer policy. Reads and effects go straight
//! to the existing feature owner; local visibility belongs to LayerState.
use super::{ChartPane, PaneChrome, PaneFootprint};
use crate::config::FeedCapabilities;
use crate::drawings::Drawings;
use crate::orderflow_view::OrderflowView;
use crate::orderflow_view::layers::{layer_facts, layer_switch, set_layer_switch};
use crate::state::ChartState;
use crate::style::ChartStyle;
use crate::toolrail::Tool;
use quantick_chart::pane_header::{
    BubbleSample, FlowHeaderState, HeaderIntent, LayerGlyph, LayerToggle, PaneHeader, PaneIdentity,
};
use quantick_chart::tick_membership::TickMembership;
use quantick_control_schema::bubble_scale::BubbleScaleTarget;
use quantick_layers::{ChartLayer, LayerActions, LayerBlock};
use quantick_layers::{LayerEffect, LayerFacts, LayerSource, LayerState, VisibilityWrite};
use smallvec::SmallVec;

/// The fields every layer answer reads, borrowed one by one so a menu can
/// hold them beside its own state while the pane lends both out.
#[derive(Clone, Copy)]
pub(crate) struct PaneLayerRead<'a> {
    pub(super) layers: &'a LayerState,
    pub(super) orderflow: Option<&'a OrderflowView>,
    pub(super) state: &'a ChartState,
    pub(super) footprint: &'a PaneFootprint,
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
            LayerSource::Grid => style.canvas.grid_enabled,
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
}

impl ChartPane {
    pub(crate) fn layer_read(&self) -> PaneLayerRead<'_> {
        PaneLayerRead {
            layers: &self.layers,
            orderflow: self.orderflow.as_ref(),
            state: &self.state,
            footprint: &self.footprint,
            drawings: &self.drawings,
        }
    }
    pub(crate) fn layer_facts(&self, capabilities: Option<FeedCapabilities>) -> LayerFacts {
        self.layer_read().facts(capabilities)
    }
    pub fn layer_switched_on(&self, layer: ChartLayer, style: &ChartStyle) -> bool {
        self.layer_read().switched_on(layer, style)
    }
    pub fn layer_visible(&self, layer: ChartLayer, style: &ChartStyle) -> bool {
        self.layer_read().visible(layer, style)
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
                    set_layer_switch(owner, switch, effect.visible);
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
        self.layer_read().blocked(layer, capabilities)
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
                .is_some_and(|owner| layer_switch(owner, switch)),
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

fn target(sample: BubbleSample) -> BubbleScaleTarget {
    match sample {
        BubbleSample::Candle => BubbleScaleTarget::Candle,
        BubbleSample::Tape => BubbleScaleTarget::Tape,
    }
}

impl ChartPane {
    /// The pane's layer glyphs: every lamp the *switch*, not what the source
    /// lets through it, with what blocks it beside it, read through the calls
    /// the pane's layer menu and `layers.visibility.set` read.
    pub(crate) fn header_glyphs(
        &self,
        identity: PaneIdentity,
        style: &ChartStyle,
        capabilities: FeedCapabilities,
    ) -> SmallVec<[LayerGlyph; 4]> {
        PaneHeader::toggles(identity)
            .iter()
            .map(|&toggle| LayerGlyph {
                toggle,
                on: self.layer_switched_on(toggle.layer(), style),
                blocked: self.layer_blocked(toggle.layer(), capabilities),
            })
            .collect()
    }

    /// The view and sizes a flow pane's header shows; `None` without an
    /// order-flow view. A sample is there only while its target is: the one
    /// size rule `orderflow.bubbles.size.set` judges by too.
    pub(crate) fn flow_header_state(&self) -> Option<FlowHeaderState> {
        let owner = self.orderflow.as_ref()?;
        let facts = self.bubble_scale_facts()?;
        let scale = |sample: BubbleSample| {
            let target = target(sample);
            facts
                .may_scale(target)
                .then(|| owner.bubble_display_scale(target.into()))
        };
        Some(FlowHeaderState {
            view: owner.cached_config().live_lane.view(),
            candle_sample: scale(BubbleSample::Candle),
            tape_sample: scale(BubbleSample::Tape),
        })
    }

    /// Carry out a press on this pane's header, through the door each control
    /// shares with its key, its menu and its named call.
    pub(crate) fn apply_header_intent(&mut self, intent: HeaderIntent, actions: &mut LayerActions) {
        match intent {
            HeaderIntent::SetLayer(toggle, on) => {
                self.set_layer_visible(toggle.layer(), on, actions);
            }
            HeaderIntent::OpenLayerSettings(LayerToggle::Footprint) => {
                actions.open_footprint_settings = true;
            }
            HeaderIntent::OpenLayerSettings(toggle) => {
                actions.open_layer_tab = Some(toggle.layer());
            }
            HeaderIntent::SetView(view) => {
                if let Some(owner) = self.orderflow.as_mut() {
                    owner.set_lane_view(view);
                }
            }
            HeaderIntent::SetSample(sample, scale) => {
                if let Some(owner) = self.orderflow.as_mut() {
                    owner.set_bubble_display_scale(target(sample).into(), scale);
                }
            }
        }
    }
}
