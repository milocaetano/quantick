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
//! O(bars since anchor) with a handful of multiplications per bar. A bar
//! close or a forming-bar change is therefore linear in the anchored span,
//! never in the whole session, and a frame without tape movement replays
//! nothing. Never on the per-trade path: ingestion does not know this module
//! exists.

use quantick_engine::Bar;
use quantick_indicators::native::AnchoredVwap;
use quantick_indicators::{Ctx, Indicator as _, IndicatorBar, PlotId};

use crate::drawings::{
    AVWAP_BAND_PAIRS, AVWAP_ROW_WIDTH, AvwapBand, AvwapCache, AvwapCacheKey, AvwapPayload, Drawings,
};
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
        refresh_one(payload, anchor_bar, inputs);
    }
}

/// The bar behind an absolute slot: prefix first, then the trade-derived
/// series, then the forming bar.
fn bar_at<'a>(inputs: &'a RefreshInputs<'_>, slot: usize) -> Option<&'a Bar> {
    if slot < inputs.prefix.len() {
        return inputs.prefix.get(slot);
    }
    let state_slot = slot - inputs.prefix.len();
    let closed = inputs.state.bars();
    if state_slot < closed.len() {
        return closed.get(state_slot);
    }
    (state_slot == closed.len())
        .then(|| inputs.state.partial())
        .flatten()
}

/// One committed plot row, read back as the cache's array shape.
fn plot_row(kernel: &AnchoredVwap, row: usize) -> [f64; AVWAP_ROW_WIDTH] {
    let plots = kernel.plots();
    let mut values = [f64::NAN; AVWAP_ROW_WIDTH];
    for (column, value) in values.iter_mut().enumerate() {
        *value = plots.value(PlotId::new(column), row);
    }
    values
}

/// The forming bar's row, previewed against the committed accumulators —
/// the kernel's own rollback contract, so a live tick costs one preview and
/// never a replay.
fn live_row(kernel: &mut AnchoredVwap, partial: &Bar) -> Option<[f64; AVWAP_ROW_WIDTH]> {
    let mut ctx = Ctx {
        bar_index: kernel.plots().len(),
        cvd: &[],
    };
    let frame = kernel
        .preview(&IndicatorBar::from(partial), &mut ctx)
        .ok()?;
    let mut values = [f64::NAN; AVWAP_ROW_WIDTH];
    for (column, value) in values.iter_mut().enumerate() {
        *value = frame.values.get(column).copied().unwrap_or(f64::NAN);
    }
    Some(values)
}

fn refresh_one(payload: &mut AvwapPayload, anchor_bar: f32, inputs: &RefreshInputs<'_>) {
    let closed_len = inputs.state.bars().len();
    let closed_total = inputs.prefix.len() + closed_len;
    let partial = inputs.state.partial();
    let slots = closed_total + usize::from(partial.is_some());
    let Some(last_slot) = slots.checked_sub(1) else {
        payload.cache = None;
        return;
    };

    // The candle under the anchor, the frvp rounding rule: a slot's centre is
    // its integer coordinate. An anchor past the newest bar seeds on the
    // newest — the average must start on a bar that exists.
    let anchor_slot = if anchor_bar.is_finite() {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let slot = anchor_bar.round().max(0.0) as usize;
        slot.min(last_slot)
    } else {
        payload.cache = None;
        return;
    };

    let key = AvwapCacheKey {
        anchor_slot,
        timeline_revision: inputs.state.timeline_revision(),
        closed_len,
        prefix_len: inputs.prefix.len(),
        source: payload.source,
        bands: payload.bands,
    };
    let partial_sig = partial.map(|bar| crate::drawings::AvwapPartialSig {
        close_time: bar.close_time,
        trades: bar.trade_count,
    });

    // Closed-state hit: at most the live row moves. A tick on the forming
    // bar previews one row against the retained accumulators — O(1),
    // whatever the anchored span.
    if let Some(cache) = payload.cache.as_mut().filter(|cache| cache.key == key) {
        if cache.partial != partial_sig {
            if cache.partial.is_some() {
                cache.rows.pop();
            }
            if let Some(bar) = partial
                && let Some(row) = live_row(&mut cache.kernel, bar)
            {
                cache.rows.push(row);
            }
            cache.partial = partial_sig;
        }
        return;
    }

    // A bar close with everything else unchanged extends the retained
    // kernel over the new closed bars — O(new bars), never the whole span.
    if let Some(cache) = payload.cache.as_mut().filter(|cache| {
        cache.key
            == AvwapCacheKey {
                closed_len: cache.key.closed_len,
                ..key
            }
            && cache.key.closed_len < closed_len
    }) {
        if cache.partial.is_some() {
            cache.rows.pop();
        }
        let done = anchor_slot + cache.kernel.plots().len();
        for slot in done..closed_total {
            let Some(bar) = bar_at(inputs, slot) else {
                break;
            };
            let mut ctx = Ctx {
                bar_index: cache.kernel.plots().len(),
                cvd: &[],
            };
            if cache
                .kernel
                .on_close(&IndicatorBar::from(bar), &mut ctx)
                .is_err()
            {
                break;
            }
            cache.rows.push(plot_row(&cache.kernel, cache.rows.len()));
        }
        if let Some(bar) = partial
            && let Some(row) = live_row(&mut cache.kernel, bar)
        {
            cache.rows.push(row);
        }
        cache.partial = partial_sig;
        cache.key = key;
        return;
    }

    // Full replay from the anchor bar — an anchor drag, a config edit or a
    // timeline rebuild. The anchor instant is that bar's own open, so every
    // bar from it onward participates — the same `close_time >= anchor`
    // rule the kernel's golden fixture pins.
    let Some(anchor_ms) = bar_at(inputs, anchor_slot).map(|bar| bar.open_time) else {
        payload.cache = None;
        return;
    };
    let bands: [(bool, f64); AVWAP_BAND_PAIRS] =
        payload.bands.map(|band: AvwapBand| (band.on, band.mult));
    let mut kernel = AnchoredVwap::new(anchor_ms, payload.source, bands);
    let mut rows = Vec::with_capacity(last_slot - anchor_slot + 1);
    for slot in anchor_slot..closed_total.max(anchor_slot) {
        if slot > last_slot {
            break;
        }
        let Some(bar) = bar_at(inputs, slot) else {
            break;
        };
        // No cross-bar series is read: every anchored source is price-scaled,
        // so the host-maintained cvd can honestly stay empty here.
        let mut ctx = Ctx {
            bar_index: kernel.plots().len(),
            cvd: &[],
        };
        if kernel.on_close(&IndicatorBar::from(bar), &mut ctx).is_err() {
            break;
        }
        rows.push(plot_row(&kernel, rows.len()));
    }
    if let Some(bar) = partial
        && let Some(row) = live_row(&mut kernel, bar)
    {
        rows.push(row);
    }
    payload.cache = Some(AvwapCache {
        key,
        partial: partial_sig,
        first_slot: anchor_slot,
        rows,
        kernel,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drawings::{AvwapBand, ChartPoint, DRAWING_TOOLS};
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
    fn refresh_reproduces_the_kernel_over_the_pane_bars() {
        let state = state_with_fixture();
        assert_eq!(state.bars().len(), 4, "the tape cuts four closed bars");
        let mut drawings = Drawings::default();
        assert!(drawings.place(avwap_tool(), ChartPoint::at(1.0, 100.0)));

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
            .expect("an avwap object holds an avwap payload");
        let cache = payload.cache.as_ref().expect("refreshed");
        assert_eq!(cache.first_slot, 1);
        assert_eq!(cache.rows.len(), 3, "anchor bar to the newest closed bar");
        // The golden fixture's hand-computed values (see the indicators
        // crate's avwap_plots.csv): vwap 96 -> 112 -> 96, band1 = ±1σ.
        assert_eq!(cache.rows[0][0], 96.0);
        assert_eq!(cache.rows[1][0], 112.0);
        assert_eq!(cache.rows[2][0], 96.0);
        assert_eq!(cache.rows[1][1], 128.0, "+1σ with σ=16");
        assert_eq!(cache.rows[1][2], 96.0, "-1σ with σ=16");
        assert!(cache.rows[0][3].is_nan(), "band 2 is off by default");
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
            .key;
        refresh(&mut drawings, &inputs);
        let payload = drawings.items_mut()[0]
            .payload
            .as_any_mut()
            .downcast_mut::<AvwapPayload>()
            .unwrap();
        assert_eq!(payload.cache.as_ref().unwrap().key, key_before);

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
        assert_ne!(cache.key, key_before);
        assert_eq!(cache.rows[1][3], 144.0, "+2σ with σ=16");
        assert_eq!(cache.rows[1][4], 80.0, "-2σ with σ=16");
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
        assert!(cache.partial.is_some());
        assert_eq!(cache.rows.len(), 2, "the anchor bar and the live row");
        // (80·16 + 200·2) / 18 = 1680 / 18 = 93.333…
        assert_eq!(cache.rows[1][0], 1680.0 / 18.0);
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
        assert_eq!(cache.first_slot, 3);
        assert_eq!(cache.rows.len(), 1);
        assert_eq!(cache.rows[0][0], 80.0, "the newest bar's own hlc3");
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
        assert_eq!(cache.first_slot, 0);
        assert_eq!(cache.rows.len(), 5, "prefix candle + four closed bars");
        // hlc3 of the prefix candle: (110+90+100)/3 = 100, vol 10 -> vwap 100.
        assert_eq!(cache.rows[0][0], 100.0);
    }
}
