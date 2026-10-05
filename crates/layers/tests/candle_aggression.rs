use quantick_layers::{ChartLayer, LayerFacts, LayerRegistry, LayerSource, LayerState};

#[test]
fn candle_aggression_is_a_named_default_off_pane_layer() {
    let layer = LayerRegistry::default()
        .resolve("candle_aggression")
        .expect("the candle overlay is discoverable by its stable ID");
    assert_eq!(layer, ChartLayer::CandleAggression);
    assert_eq!(layer.0.source, LayerSource::Local);
    assert!(!layer.0.default_on);
    assert!(!layer.0.projection_demand, "no second order-flow worker");
    assert!(
        !layer.persisted(),
        "saved per asset with its bubble settings, not in the chart-layers file"
    );
    assert!(!LayerState::default().requested(layer));
}

#[test]
fn candle_aggression_reports_its_tick_and_native_price_requirements() {
    let available = LayerFacts {
        tick_bars: true,
        native_candle_prices: true,
        traded_volume: true,
        ..Default::default()
    };
    assert!(LayerState::effective(
        ChartLayer::CandleAggression,
        true,
        available
    ));
    for (facts, reason) in [
        (
            LayerFacts {
                tick_bars: false,
                ..available
            },
            "candle_aggression_requires_tick_bars",
        ),
        (
            LayerFacts {
                native_candle_prices: false,
                ..available
            },
            "candle_aggression_requires_native_prices",
        ),
        (
            LayerFacts {
                traded_volume: false,
                ..available
            },
            "source_prints_no_traded_volume",
        ),
        (
            LayerFacts {
                tape_only: true,
                ..available
            },
            "candle_layer_hidden_in_tape_only",
        ),
    ] {
        assert!(!LayerState::effective(
            ChartLayer::CandleAggression,
            true,
            facts
        ));
        assert_eq!(
            LayerState::blocked(ChartLayer::CandleAggression, facts).map(|block| block.code),
            Some(reason)
        );
    }
}
