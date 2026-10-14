//! Expand source admission to complete causal blocks without changing the camera clip.
use crate::tick_membership::TickMembership;
use quantick_orderflow::{TickBurstGrouping, projection::flow_tape::FlowBurstView};
use std::ops::Range;

pub(super) fn context(
    membership: &TickMembership,
    slots: &mut Range<usize>,
    width_px: &mut f32,
    calibration: TickBurstGrouping,
) -> Option<FlowBurstView> {
    let (first, last) = membership.time_bounds(slots.clone())?;
    let from_ms = first.div_euclid(calibration.window_ms) * calibration.window_ms;
    let until_ms = last
        .div_euclid(calibration.window_ms)
        .saturating_add(1)
        .saturating_mul(calibration.window_ms);
    let expanded = membership.slots_at_times(from_ms, until_ms)?;
    *width_px *= expanded.len() as f32 / slots.len() as f32;
    *slots = expanded;
    Some(FlowBurstView {
        calibration,
        from_ms,
        until_ms,
        settled_through_ms: (membership.latest_timestamp_ms()?.div_euclid(100) * 100).min(until_ms),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{BarSpec, ChartState};
    use quantick_engine::{Side, Trade};

    #[test]
    fn late_timestamps_do_not_remove_the_visible_candle_from_admission() {
        let mut state = ChartState::new(BarSpec::Tick(1));
        for timestamp_ms in [73_000, 1_000, 74_000] {
            state.ingest_live(&Trade {
                agg_id: 1,
                timestamp_ms,
                price: 200_000.into(),
                quantity: 1.into(),
                side: Side::Buy,
            });
        }
        let mut slots = 0..1;
        let mut width = 32.0;
        let burst = context(
            state.tick_membership().unwrap(),
            &mut slots,
            &mut width,
            TickBurstGrouping {
                window_ms: 72_000,
                width_px: 420.0,
                price_units_per_pixel: 0.8,
                max_radius: 18.0,
            },
        )
        .unwrap();
        assert_eq!(slots, 0..3);
        assert_eq!((burst.from_ms, burst.until_ms), (72_000, 144_000));
        assert_eq!(width, 96.0);
    }

    #[test]
    fn middle_candle_admits_its_whole_block_at_the_original_pixels_per_slot() {
        let mut state = ChartState::new(BarSpec::Tick(2));
        for time in [1_000, 2_000, 3_000, 4_000, 71_000, 72_000, 73_000, 74_000] {
            state.ingest_live(&Trade {
                agg_id: 1,
                timestamp_ms: time,
                price: 200_000.into(),
                quantity: 1.into(),
                side: Side::Buy,
            });
        }
        let mut slots = 1..2;
        let mut width = 32.0;
        let burst = context(
            state.tick_membership().unwrap(),
            &mut slots,
            &mut width,
            TickBurstGrouping {
                window_ms: 72_000,
                width_px: 420.0,
                price_units_per_pixel: 0.8,
                max_radius: 18.0,
            },
        )
        .unwrap();
        assert_eq!(
            slots,
            0..3,
            "complete candles include an explicitly clipped next-block record"
        );
        assert_eq!(width, 96.0);
        assert_eq!((burst.from_ms, burst.until_ms), (0, 72_000));
    }

    #[test]
    fn time_queries_match_full_scan_before_and_after_late_candles() {
        for times in [
            [1_000, 2_000, 3_000, 4_000, 71_000, 72_000, 73_000, 74_000],
            [73_000, 74_000, 1_000, 2_000, 71_000, 75_000, 500, 76_000],
        ] {
            let mut state = ChartState::new(BarSpec::Tick(2));
            for timestamp_ms in times {
                state.ingest_live(&Trade {
                    agg_id: 1,
                    timestamp_ms,
                    price: 200_000.into(),
                    quantity: 1.into(),
                    side: Side::Buy,
                });
            }
            let membership = state.tick_membership().unwrap();
            assert_eq!(
                membership.latest_timestamp_ms(),
                times.iter().copied().max()
            );
            for (from, until) in [
                (0, 72_000),
                (72_000, 144_000),
                (3_000, 4_000),
                (90_000, 100_000),
            ] {
                let matches: Vec<_> = times
                    .chunks(2)
                    .enumerate()
                    .filter_map(|(slot, pair)| {
                        (*pair.iter().max().unwrap() >= from && *pair.iter().min().unwrap() < until)
                            .then_some(slot)
                    })
                    .collect();
                let expected = matches
                    .first()
                    .map(|first| *first..matches.last().unwrap() + 1);
                assert_eq!(membership.slots_at_times(from, until), expected);
            }
            assert_eq!(
                membership.time_bounds(0..4),
                Some((*times.iter().min().unwrap(), *times.iter().max().unwrap()))
            );
        }
    }
}
