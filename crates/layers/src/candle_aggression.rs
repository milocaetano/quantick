//! Optional, native-price aggression over tick candles.
use crate::{
    ChartLayer, LayerBlock, LayerDescriptor, LayerFacts, LayerScope, LayerSource, LayerState,
    Persistence, Requirement,
};

#[allow(non_upper_case_globals)]
impl ChartLayer {
    pub const CandleAggression: Self = Self(&LayerDescriptor {
        id: "candle_aggression",
        label: "candle aggression",
        hint: "one small, translucent buy/sell bubble per tick candle at its quantity-weighted trade price. Area follows visible candle volume; no numbers or candle restyling. Inferred sides are labelled in the status bar. Approximated and cap-coarsened rows are omitted. The flow pane's switch is saved per asset with its bubble settings while the asset's \"Save changes for this asset\" is on; a context-pane toggle lasts for that context. The tape keeps its own settings",
        source: LayerSource::Local,
        scope: LayerScope::Pane,
        persistence: Persistence::OrderflowPreset,
        requirement: Requirement::Volume,
        on_tape: false,
        needs_tape: false,
        needs_depth: false,
        capture_gates_visibility: false,
        default_on: false,
        projection_demand: false,
    });
}

impl LayerState {
    /// Whether the candles carry the per-candle summary: asked for by its own
    /// layer, or by the aggression bubbles beside the native tape, where the
    /// summary is what they draw. One answer, so it is drawn once.
    pub fn candle_summary(facts: LayerFacts, bubbles: bool, candle_aggression: bool) -> bool {
        [
            (ChartLayer::Bubbles, bubbles),
            (ChartLayer::CandleAggression, candle_aggression),
        ]
        .into_iter()
        .any(|(layer, requested)| {
            draws_summary(layer, facts) && Self::effective(layer, requested, facts)
        })
    }
}

/// `layer` draws the summary: candle aggression always, the aggression
/// bubbles beside the native tape, which keys every print on the tape.
pub(crate) fn draws_summary(layer: ChartLayer, facts: LayerFacts) -> bool {
    layer == ChartLayer::CandleAggression || (layer == ChartLayer::Bubbles && facts.native_tape)
}

pub(crate) fn blocked(facts: LayerFacts) -> Option<LayerBlock> {
    if !facts.tick_bars {
        Some(LayerBlock::new(
            "candle_aggression_requires_tick_bars",
            "candle aggression is available on tick candles",
        ))
    } else if !facts.native_candle_prices {
        Some(LayerBlock::new(
            "candle_aggression_requires_native_prices",
            "candle aggression is waiting for enough trades to establish their native price grid",
        ))
    } else {
        None
    }
}
