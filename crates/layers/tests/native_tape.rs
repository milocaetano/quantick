//! Beside the candles, the native tape keeps every candle layer available but
//! the candle-slot bubbles it replaces, and says why.
use quantick_layers::{ChartLayer as L, LayerFacts, LayerState, blocks};

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
