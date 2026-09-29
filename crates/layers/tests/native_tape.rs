//! Beside the candles, the native tape keeps every candle layer available but
//! the candle-slot bubbles it replaces, and says why.
use quantick_layers::{
    ChartLayer as L, LayerFacts, LayerSource, LayerState, OrderflowSwitch, Persistence, blocks,
};

fn ordinary() -> LayerFacts {
    LayerFacts {
        flow_pane: true,
        tape_on: true,
        book_capture: true,
        traded_volume: true,
        capture_enabled: true,
        depth_visible: true,
        tick_bars: true,
        native_candle_prices: true,
        ..Default::default()
    }
}

#[test]
fn the_native_tape_beside_the_candles_keeps_the_candle_layers() {
    let beside = LayerFacts {
        native_tape: true,
        ..ordinary()
    };
    for layer in [
        L::Footprint,
        L::Drawings,
        L::TradePaint,
        L::Heatmap,
        L::CandleAggression,
        L::LiveStrip,
        L::TapeBubbles,
        L::TapeHeatmap,
    ] {
        assert_eq!(
            LayerState::blocked(layer, beside).map(|block| block.code),
            None,
            "{layer:?} stays available beside the native tape"
        );
    }
}

#[test]
fn the_native_tape_reports_the_candle_slot_bubbles_it_replaces() {
    let beside = LayerFacts {
        native_tape: true,
        ..ordinary()
    };
    assert!(LayerState::effective(L::Bubbles, true, ordinary()));
    assert!(!LayerState::effective(L::Bubbles, true, beside));
    assert_eq!(
        LayerState::blocked(L::Bubbles, beside),
        Some(blocks::NATIVE_TAPE_CANDLE_BUBBLES)
    );
    assert_eq!(
        blocks::NATIVE_TAPE_CANDLE_BUBBLES.code,
        "candle_bubbles_replaced_by_native_tape"
    );
    assert!(
        LayerState::restorable(L::Bubbles, beside),
        "the trader's own choice is kept for when the native tape goes"
    );
    // Tape only keeps its own, wider reason.
    let tape_only = LayerFacts {
        tape_only: true,
        ..beside
    };
    assert_eq!(
        LayerState::blocked(L::Bubbles, tape_only),
        Some(blocks::TAPE_ONLY_CANDLES)
    );
}

/// The native tape is a switch of its own beside tape only: one registry
/// entry the layer menu, the settings checkbox and `layers.visibility.set`
/// share, saved with the order-flow preset exactly like tape only.
#[test]
fn the_native_tape_is_a_layer_switch_like_tape_only() {
    let layer = L::NativeTape;
    assert_eq!(layer.0.id, "native_tape");
    assert_eq!(
        layer.0.source,
        LayerSource::Orderflow(OrderflowSwitch::NativeTape)
    );
    assert_eq!(layer.0.scope, L::TapeOnly.0.scope);
    assert_eq!(layer.0.persistence, Persistence::OrderflowPreset);
    assert!(!layer.0.default_on, "off until someone asks");
    assert!(
        layer.0.hint.contains("execution time and price"),
        "{}",
        layer.0.hint
    );
    assert!(L::ALL.contains(&layer));

    let dots = LayerFacts {
        volume_dots: true,
        ..ordinary()
    };
    assert!(LayerState::effective(layer, true, dots));
    assert_eq!(
        LayerState::blocked(layer, ordinary()),
        Some(blocks::NATIVE_TAPE_NEEDS_VOLUME_DOTS),
        "without volume dots there is no execution tape to build"
    );
    assert!(!LayerState::effective(layer, true, ordinary()));
    assert_eq!(
        LayerState::blocked(
            layer,
            LayerFacts {
                tape_on: false,
                ..dots
            }
        ),
        Some(blocks::TAPE_OFF)
    );
    // Tape only draws the native tape, dots or not, so the switch has
    // nothing to change there: it is blocked, and the reason says so.
    for tape_only in [
        LayerFacts {
            tape_only: true,
            native_tape: true,
            ..ordinary()
        },
        LayerFacts {
            tape_only: true,
            native_tape: true,
            ..dots
        },
    ] {
        assert_eq!(
            LayerState::blocked(layer, tape_only).map(|block| block.code),
            Some("tape_only_always_draws_the_native_tape")
        );
        let block = LayerState::blocked(layer, tape_only).expect("blocked");
        assert!(
            block.explanation.contains("tape only") && block.explanation.contains("native tape"),
            "{}",
            block.explanation
        );
        assert!(!LayerState::effective(layer, true, tape_only));
    }
}
