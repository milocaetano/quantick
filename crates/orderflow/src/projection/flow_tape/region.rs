//! The bounds a FLOW merge never crosses, and the zoom ladder that picks them.
//!
//! A region is keyed in market data and nothing else, so every frame computes
//! the same regions and a closed one never regroups as the tape rolls or the
//! chart pans. One second of market time is the original bound: right for a
//! chart whose candle is many seconds wide on screen, but on a compressed
//! tick chart a second is under a pixel, and the regions of a whole candle
//! pile on one another without ever pooling. A zoom region is instead `k`
//! whole candles counted from the series' first (`slot / k`), with `k` a
//! power of two held across frames: zooming out pools neighbouring candles
//! into one sphere, zooming in splits them again, and nothing else moves one.

use super::constants::{
    FLOW_REGION_CANDLE_LADDER, FLOW_REGION_HOLD_BAND, FLOW_REGION_MIN_WIDTH_PX,
    FLOW_REGION_WINDOW_MS,
};

/// What a merged FLOW region may not cross.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FlowRegionSpan {
    /// Windows of [`FLOW_REGION_WINDOW_MS`] of market time.
    #[default]
    Time,
    /// This many whole candles, counted from the series' first.
    Candles(usize),
}

impl FlowRegionSpan {
    /// The region holding a native cell of candle `slot` first printed at
    /// `timestamp_ms`.
    #[must_use]
    pub fn key(self, slot: usize, timestamp_ms: i64) -> i64 {
        match self {
            Self::Time => timestamp_ms.div_euclid(FLOW_REGION_WINDOW_MS),
            Self::Candles(candles) => i64::try_from(slot / candles.max(1)).unwrap_or(i64::MAX),
        }
    }

    /// The candles of a zoom region; `None` for market time.
    #[must_use]
    pub fn candles(self) -> Option<usize> {
        match self {
            Self::Time => None,
            Self::Candles(candles) => Some(candles.max(1)),
        }
    }

    /// Whether a region merges whole before the painted clip reads it. A zoom
    /// region is many pixels wide, so clipping its cells first would regroup
    /// the region at the view's edge on every pan.
    #[must_use]
    pub(super) fn merges_before_clip(self) -> bool {
        self.candles().is_some()
    }

    /// The candles of a zoom region on a chart drawing one candle
    /// `px_per_candle` wide: the `held` rung while its region is at least
    /// [`FLOW_REGION_MIN_WIDTH_PX`] wide and its half is under
    /// [`FLOW_REGION_HOLD_BAND`] times that, otherwise the narrowest rung at
    /// least that wide (the widest when none is).
    #[must_use]
    pub fn held(held: Option<usize>, px_per_candle: f64) -> usize {
        let widest = FLOW_REGION_CANDLE_LADDER[FLOW_REGION_CANDLE_LADDER.len() - 1];
        if !(px_per_candle.is_finite() && px_per_candle > 0.0) {
            return held.unwrap_or(widest);
        }
        let width = |candles: usize| candles as f64 * px_per_candle;
        let halve_at = FLOW_REGION_MIN_WIDTH_PX * FLOW_REGION_HOLD_BAND;
        held.filter(|candles| {
            FLOW_REGION_CANDLE_LADDER.contains(candles)
                && width(*candles) >= FLOW_REGION_MIN_WIDTH_PX
                && (*candles == 1 || width(candles / 2) < halve_at)
        })
        .unwrap_or_else(|| {
            FLOW_REGION_CANDLE_LADDER
                .into_iter()
                .find(|candles| width(*candles) >= FLOW_REGION_MIN_WIDTH_PX)
                .unwrap_or(widest)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Market time keys by the second whatever the candle; a zoom region by
    /// whole candles whatever the time.
    #[test]
    fn a_region_is_keyed_in_market_data() {
        assert_eq!(FlowRegionSpan::Time.key(7, 2_999), 2);
        assert_eq!(FlowRegionSpan::Time.key(900, 2_999), 2);
        assert_eq!(FlowRegionSpan::Time.key(7, -1), -1);
        let four = FlowRegionSpan::Candles(4);
        assert_eq!(four.key(0, 5_000), 0);
        assert_eq!(four.key(3, 0), 0);
        assert_eq!(four.key(4, 0), 1);
        assert_eq!(four.key(11, 99_000), 2);
        assert_eq!(
            FlowRegionSpan::Candles(0).key(5, 0),
            5,
            "never a division by zero"
        );
        assert_eq!(FlowRegionSpan::default(), FlowRegionSpan::Time);
        assert_eq!(FlowRegionSpan::Time.candles(), None);
        assert_eq!(four.candles(), Some(4));
    }

    /// Zooming out widens the region one rung at a time; each rung is a whole
    /// union of the one before.
    #[test]
    fn the_region_widens_as_the_candles_narrow() {
        let rung = |px| FlowRegionSpan::held(None, px);
        assert_eq!(rung(120.0), 1);
        assert_eq!(rung(48.0), 1);
        assert_eq!(rung(47.0), 2);
        assert_eq!(rung(17.0), 4);
        assert_eq!(rung(8.0), 8);
        assert_eq!(rung(2.0), 32);
        assert_eq!(
            rung(0.001),
            2_048,
            "the widest rung when none is wide enough"
        );
        for pair in FLOW_REGION_CANDLE_LADDER.windows(2) {
            assert_eq!(pair[1], pair[0] * 2);
        }
    }

    /// Inside the band a wobbling zoom keeps its rung, in both directions,
    /// and an unmeasurable candle keeps whatever is held.
    #[test]
    fn a_held_rung_survives_a_wobble() {
        // Four candles at 12 px are exactly the least width.
        assert_eq!(FlowRegionSpan::held(Some(4), 12.0), 4);
        assert_eq!(FlowRegionSpan::held(Some(4), 11.9), 8, "too narrow: widen");
        // Still held while its half is under 60 px (candles under 30 px).
        assert_eq!(FlowRegionSpan::held(Some(4), 29.0), 4);
        assert_eq!(FlowRegionSpan::held(Some(4), 30.0), 2, "its half now fits");
        assert_eq!(FlowRegionSpan::held(Some(1), 500.0), 1);
        assert_eq!(FlowRegionSpan::held(Some(3), 20.0), 4, "off the ladder");
        for unmeasurable in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert_eq!(FlowRegionSpan::held(Some(8), unmeasurable), 8);
        }
    }
}
