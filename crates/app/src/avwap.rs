//! Refresh pass for anchored-VWAP drawings.
//!
//! The drawing owns *where* (one anchor, a bar on the tape); the `indicators`
//! crate owns *what* — the golden-tested
//! [`AnchoredVwap`](quantick_indicators::native::AnchoredVwap) kernel this
//! module replays over the pane's bars into the payload's cache. It runs once
//! per frame per pane, before the drawings paint, and it replays only when
//! something the average depends on actually changed — the cache key names
//! every such input, so the common frame costs one key comparison per object
//! and no math at all.
//!
//! Rate, declared: a key miss replays from the anchor to the live edge —
//! O(bars since anchor) with a handful of multiplications per bar. The app
//! increments the timeline key on ordinary live input as well as rebuilds. A bar
//! close or a forming-bar change is therefore linear in the anchored span,
//! never in the whole session, and a frame without tape movement replays
//! nothing. Never on the per-trade path: ingestion does not know this module
//! exists.

use quantick_anchored_studies::{AnchoredAverage, AverageInputs, AverageRequest};
use quantick_engine::Bar;

use crate::drawings::{AvwapPayload, Drawings};
use crate::state::ChartState;

/// The registry id of the anchored-VWAP tool — the one string this module
/// and the pane share to recognise its objects.
pub const TOOL_ID: &str = "anchored-vwap";

/// Everything one refresh reads. The prefix is the pane's venue-history
/// candles; the kernel folds them like any other bar — a candle carries the
/// volume and OHLC the average needs, no tape required.
pub struct RefreshInputs<'a> {
    pub state: &'a ChartState,
    /// Venue-history bars in front of the trade-derived series.
    pub prefix: &'a [Bar],
}

/// Bring every anchored-VWAP drawing's cached rows up to date.
///
/// Mutates only derived payload state ([`AvwapCache`]), which is excluded
/// from payload equality — so this pass can never register as a user edit in
/// the undo history, however often it runs.
pub fn refresh(drawings: &mut Drawings, inputs: &RefreshInputs<'_>) {
    let core = AverageInputs {
        closed: inputs.state.bars(),
        partial: inputs.state.partial(),
        prefix: inputs.prefix,
        timeline_revision: inputs.state.timeline_revision(),
    };
    for drawing in drawings.items_mut() {
        if drawing.tool.id() != TOOL_ID {
            continue;
        }
        let Some(anchor_bar) = drawing.points.first().map(|point| point.bar) else {
            continue;
        };
        let Some(payload) = drawing.payload.as_any_mut().downcast_mut::<AvwapPayload>() else {
            continue;
        };
        AnchoredAverage::refresh(
            &mut payload.cache,
            AverageRequest {
                anchor_bar,
                source: payload.source,
                bands: payload.bands,
            },
            &core,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drawings::{ChartPoint, DRAWING_TOOLS};
    use quantick_anchored_studies::AvwapBand;
    use quantick_engine::Trade;
    use rust_decimal::Decimal;

    fn trade(id: u64, ms: i64, price: i64, qty: i64) -> Trade {
        Trade {
            agg_id: id,
            timestamp_ms: ms,
            price: Decimal::from(price),
            quantity: Decimal::from(qty),
            side: quantick_engine::Side::Buy,
        }
    }

    fn avwap_tool() -> crate::drawings::DrawingTool {
        DRAWING_TOOLS
            .into_iter()
            .find(|tool| tool.id() == TOOL_ID)
            .expect("the anchored VWAP is registered")
    }

    /// The golden fixture's binary-exact tape, cut as tick(3) bars: anchoring
    /// on bar 1 must reproduce the kernel's own numbers — the refresh is a
    /// bridge, never a second implementation.
    fn state_with_fixture() -> ChartState {
        let mut state = ChartState::new(crate::state::BarSpec::Tick(3));
        let trades = [
            trade(1, 1_000, 100, 1),
            trade(2, 1_100, 100, 1),
            trade(3, 1_200, 100, 1),
            trade(4, 2_000, 112, 2),
            trade(5, 2_100, 80, 2),
            trade(6, 2_200, 96, 4),
            trade(7, 3_000, 144, 2),
            trade(8, 3_100, 112, 2),
            trade(9, 3_200, 128, 4),
            trade(10, 4_000, 96, 4),
            trade(11, 4_100, 64, 4),
            trade(12, 4_200, 80, 8),
        ];
        for trade in &trades {
            state.ingest_live(trade);
        }
        state
    }

    #[test]
    fn actual_live_revision_invalidates_key_and_preserves_closed_rows() {
        let mut state = state_with_fixture();
        let mut drawings = Drawings::default();
        assert!(drawings.place(avwap_tool(), ChartPoint::at(1.0, 100.0)));
        refresh(
            &mut drawings,
            &RefreshInputs {
                state: &state,
                prefix: &[],
            },
        );
        let before = drawings.items()[0]
            .payload
            .as_any()
            .downcast_ref::<AvwapPayload>()
            .unwrap()
            .cache
            .as_ref()
            .unwrap()
            .output()
            .key;
        assert_eq!(before.timeline_revision, 12);
        state.ingest_live(&trade(13, 5_000, 136, 32));
        refresh(
            &mut drawings,
            &RefreshInputs {
                state: &state,
                prefix: &[],
            },
        );
        let cache = drawings.items()[0]
            .payload
            .as_any()
            .downcast_ref::<AvwapPayload>()
            .unwrap()
            .cache
            .as_ref()
            .unwrap();
        assert_eq!(cache.output().key.timeline_revision, 13);
        assert_ne!(cache.output().key, before);
        assert_eq!(cache.committed_rows(), 3);
        assert_eq!(cache.output().rows.len(), 4);
        assert_eq!(
            cache
                .output()
                .rows
                .iter()
                .map(|row| row[0])
                .collect::<Vec<_>>(),
            [96.0, 112.0, 96.0, 116.0]
        );
        assert!(cache.output().rows.iter().all(|row| row[3].is_nan()));
        println!(
            "AVWAP live baseline: revision 12 -> 13; closed rows 3; values [96, 112, 96, 116]"
        );
    }

    #[test]
    fn a_key_hit_replays_nothing_and_a_config_edit_replays() {
        let state = state_with_fixture();
        let mut drawings = Drawings::default();
        assert!(drawings.place(avwap_tool(), ChartPoint::at(1.0, 100.0)));
        let inputs = RefreshInputs {
            state: &state,
            prefix: &[],
        };
        refresh(&mut drawings, &inputs);
        let key_before = drawings.items()[0]
            .payload
            .as_any()
            .downcast_ref::<AvwapPayload>()
            .unwrap()
            .cache
            .as_ref()
            .unwrap()
            .output()
            .key;
        refresh(&mut drawings, &inputs);
        let payload = drawings.items_mut()[0]
            .payload
            .as_any_mut()
            .downcast_mut::<AvwapPayload>()
            .unwrap();
        assert_eq!(payload.cache.as_ref().unwrap().output().key, key_before);

        // Switching a band on changes the key and the rows.
        payload.bands[1] = AvwapBand {
            on: true,
            mult: 2.0,
        };
        refresh(&mut drawings, &inputs);
        let payload = drawings.items()[0]
            .payload
            .as_any()
            .downcast_ref::<AvwapPayload>()
            .unwrap();
        let cache = payload.cache.as_ref().unwrap();
        assert_ne!(cache.output().key, key_before);
        assert_eq!(cache.output().rows[1][3], 144.0, "+2σ with σ=16");
        assert_eq!(cache.output().rows[1][4], 80.0, "-2σ with σ=16");
    }

    #[test]
    fn the_forming_bar_joins_as_the_live_row() {
        let mut state = state_with_fixture();
        // One more trade opens a forming bar at price 200, qty 2.
        state.ingest_live(&trade(13, 5_000, 200, 2));
        assert!(state.partial().is_some());
        let mut drawings = Drawings::default();
        // Anchor on the newest closed bar (slot 3): sums are that bar's
        // hlc3=80 vol=16 plus the forming trade's 200×2.
        assert!(drawings.place(avwap_tool(), ChartPoint::at(3.0, 80.0)));
        let inputs = RefreshInputs {
            state: &state,
            prefix: &[],
        };
        refresh(&mut drawings, &inputs);
        let payload = drawings.items()[0]
            .payload
            .as_any()
            .downcast_ref::<AvwapPayload>()
            .unwrap();
        let cache = payload.cache.as_ref().unwrap();
        assert!(cache.output().partial.is_some());
        assert_eq!(
            cache.output().rows.len(),
            2,
            "the anchor bar and the live row"
        );
        // (80·16 + 200·2) / 18 = 1680 / 18 = 93.333…
        assert_eq!(cache.output().rows[1][0], 1680.0 / 18.0);
    }

    #[test]
    fn an_anchor_past_the_newest_bar_seeds_on_the_newest() {
        let state = state_with_fixture();
        let mut drawings = Drawings::default();
        assert!(drawings.place(avwap_tool(), ChartPoint::at(99.0, 80.0)));
        refresh(
            &mut drawings,
            &RefreshInputs {
                state: &state,
                prefix: &[],
            },
        );
        let payload = drawings.items()[0]
            .payload
            .as_any()
            .downcast_ref::<AvwapPayload>()
            .unwrap();
        let cache = payload.cache.as_ref().expect("clamped, not refused");
        assert_eq!(cache.output().first_slot, 3);
        assert_eq!(cache.output().rows.len(), 1);
        assert_eq!(cache.output().rows[0][0], 80.0, "the newest bar's own hlc3");
    }

    #[test]
    fn an_empty_pane_holds_no_cache() {
        let state = ChartState::new(crate::state::BarSpec::Tick(3));
        let mut drawings = Drawings::default();
        assert!(drawings.place(avwap_tool(), ChartPoint::at(0.0, 100.0)));
        refresh(
            &mut drawings,
            &RefreshInputs {
                state: &state,
                prefix: &[],
            },
        );
        let payload = drawings.items()[0]
            .payload
            .as_any()
            .downcast_ref::<AvwapPayload>()
            .unwrap();
        assert!(payload.cache.is_none(), "no bars, no average, no guess");
    }

    /// Venue-history candles participate like any other bar — the average
    /// needs OHLC and volume, which a candle carries; nothing is approximated.
    #[test]
    fn the_prefix_participates_in_the_average() {
        let state = state_with_fixture();
        let prefix = [Bar {
            open_time: 0,
            close_time: 900,
            open: Decimal::from(100),
            high: Decimal::from(110),
            low: Decimal::from(90),
            close: Decimal::from(100),
            buy_volume: Decimal::from(5),
            sell_volume: Decimal::from(5),
            trade_count: 10,
        }];
        let mut drawings = Drawings::default();
        // Slot 0 is the prefix candle; the trade-derived bars shift right.
        assert!(drawings.place(avwap_tool(), ChartPoint::at(0.0, 100.0)));
        refresh(
            &mut drawings,
            &RefreshInputs {
                state: &state,
                prefix: &prefix,
            },
        );
        let payload = drawings.items()[0]
            .payload
            .as_any()
            .downcast_ref::<AvwapPayload>()
            .unwrap();
        let cache = payload.cache.as_ref().unwrap();
        assert_eq!(cache.output().first_slot, 0);
        assert_eq!(
            cache.output().rows.len(),
            5,
            "prefix candle + four closed bars"
        );
        // hlc3 of the prefix candle: (110+90+100)/3 = 100, vol 10 -> vwap 100.
        assert_eq!(cache.output().rows[0][0], 100.0);
    }
}
