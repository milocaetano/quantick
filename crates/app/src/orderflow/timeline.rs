//! Piecewise time projection over equal-width alternative-bar slots.

use quantick_engine::Bar;

/// A timestamp located inside one equal-width bar slot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimelinePosition {
    /// Global bar index supplied to [`BarTimeline::from_bars`].
    pub bar_index: usize,
    /// Fraction within this bar's visual slot, in `[0, 1]`.
    pub fraction: f64,
    /// Position across the complete supplied timeline, in `[0, 1]`.
    pub normalized: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Slot {
    bar_index: usize,
    start_ms: i64,
    end_ms: i64,
}

/// How many recent closed bars decide the live lane's reserved time span.
///
/// A single bar is too fragile a reference: one session break, one burst bar,
/// and the lane would be calibrated to a duration that never repeats, crushing
/// every print of the next bar into its left edge. A median over a short
/// window follows the instrument's rhythm without letting one outlier set it.
const RESERVE_SAMPLE_BARS: usize = 8;

/// Time the live lane reserves before any bar has closed, in exchange
/// milliseconds. Only the very first bars of a fresh series see it.
const DEFAULT_RESERVE_MS: i64 = 1_000;

/// Exchange time the forming bar's slot holds open before the tape has filled
/// it: the typical duration of the recent closed bars.
///
/// This is what makes the live lane a *reserved* band. The forming slot is
/// this wide in time from the instant it opens, so the chart shows an empty
/// band that fills left to right instead of a region that grows out of nothing
/// on every bar open. Once the bar outlives the reserve the slot keeps growing
/// with the tape, which compresses what is already drawn rather than pushing
/// it past the band's right edge.
#[must_use]
pub fn reserved_span_ms(closed: &[Bar]) -> i64 {
    let mut durations: Vec<i64> = closed
        .iter()
        .rev()
        .take(RESERVE_SAMPLE_BARS)
        .map(|bar| bar.close_time.saturating_sub(bar.open_time).max(1))
        .collect();
    if durations.is_empty() {
        return DEFAULT_RESERVE_MS;
    }
    durations.sort_unstable();
    durations[durations.len() / 2]
}

/// Mapping from exchange timestamps to the chart's equal-width bar slots.
///
/// Internal boundaries are anchored by the next bar's `open_time`. Therefore
/// book changes between two trades remain visible in the preceding slot
/// instead of disappearing into an unrepresented wall-clock gap.
#[derive(Debug, Clone, Default)]
pub struct BarTimeline {
    slots: Vec<Slot>,
    live_start_ms: Option<i64>,
    live_now_ms: Option<i64>,
}

impl BarTimeline {
    /// Build a timeline from closed bars and an optional forming bar.
    ///
    /// `first_bar_index` lets a visible slice retain global chart indices.
    /// `live_end_ms` is the live edge — the newest timestamp any recorded
    /// stream has reached. Supplying it turns the forming bar's slot into the
    /// live lane: it extends to that edge and, before the tape gets there, to
    /// [`reserved_span_ms`] past the bar's open.
    #[must_use]
    pub fn from_bars(
        first_bar_index: usize,
        closed: &[Bar],
        partial: Option<&Bar>,
        live_end_ms: Option<i64>,
    ) -> Self {
        let bars: Vec<&Bar> = closed.iter().chain(partial).collect();
        let mut slots = Vec::with_capacity(bars.len());
        // A lane belongs to a bar that is still forming. Without a partial the
        // last slot is a closed bar, and a closed bar reserves no future.
        let reserve_ms = match (partial, live_end_ms) {
            (Some(_), Some(_)) => reserved_span_ms(closed),
            _ => 0,
        };

        for (offset, bar) in bars.iter().enumerate() {
            let start_ms = bar.open_time;
            let natural_end = bars
                .get(offset + 1)
                .map_or(bar.close_time, |next| next.open_time);
            let live_end = if offset + 1 == bars.len() {
                live_end_ms.map_or(natural_end, |edge| {
                    edge.max(start_ms.saturating_add(reserve_ms))
                })
            } else {
                natural_end
            };
            let end_ms = natural_end.max(live_end).max(start_ms.saturating_add(1));
            slots.push(Slot {
                bar_index: first_bar_index + offset,
                start_ms,
                end_ms,
            });
        }

        let live_start_ms = live_end_ms
            .and(partial)
            .and_then(|_| slots.last())
            .map(|slot| slot.start_ms);
        Self {
            slots,
            live_start_ms,
            live_now_ms: live_start_ms.and(live_end_ms),
        }
    }

    /// Exchange timestamp where the live lane begins — the forming bar's open.
    ///
    /// Prints at or after it are arriving in real time and get the lane's own
    /// treatment; everything before it is settled history the chart is free to
    /// summarize. `None` when this timeline follows no live edge, in which
    /// case every slot is history.
    #[must_use]
    pub fn live_start_ms(&self) -> Option<i64> {
        self.live_start_ms
    }

    /// Position of the live edge inside the lane, in `[0,1]` chart coordinates.
    ///
    /// This is how far market time has walked across the reserved band. Marks
    /// land to its left; the space to its right is time the lane is holding
    /// open, not liquidity that vanished.
    #[must_use]
    pub fn live_now_position(&self) -> Option<TimelinePosition> {
        self.locate_clamped(self.live_now_ms?)
    }

    /// Number of represented bar slots.
    #[must_use]
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// Whether no bars are represented.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// Inclusive timestamp bounds represented by the timeline.
    #[must_use]
    pub fn timestamp_range(&self) -> Option<(i64, i64)> {
        Some((self.slots.first()?.start_ms, self.slots.last()?.end_ms))
    }

    /// Locate a timestamp. Values outside coverage return `None`.
    #[must_use]
    pub fn locate(&self, timestamp_ms: i64) -> Option<TimelinePosition> {
        let (first, last) = self.timestamp_range()?;
        if timestamp_ms < first || timestamp_ms > last {
            return None;
        }
        Some(self.locate_in_range(timestamp_ms))
    }

    /// Locate a timestamp after clipping it to the timeline bounds.
    #[must_use]
    pub fn locate_clamped(&self, timestamp_ms: i64) -> Option<TimelinePosition> {
        let (first, last) = self.timestamp_range()?;
        Some(self.locate_in_range(timestamp_ms.clamp(first, last)))
    }

    fn locate_in_range(&self, timestamp_ms: i64) -> TimelinePosition {
        // Pick the last slot whose start is <= the timestamp. At an exact
        // boundary this selects the new bar; the normalized x is identical to
        // the prior slot's right edge.
        let partition = self
            .slots
            .partition_point(|slot| slot.start_ms <= timestamp_ms);
        let slot_index = partition.saturating_sub(1).min(self.slots.len() - 1);
        let slot = self.slots[slot_index];
        let span = (slot.end_ms - slot.start_ms).max(1) as f64;
        let fraction = ((timestamp_ms - slot.start_ms) as f64 / span).clamp(0.0, 1.0);
        let normalized = (slot_index as f64 + fraction) / self.slots.len() as f64;
        TimelinePosition {
            bar_index: slot.bar_index,
            fraction,
            normalized,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;

    fn bar(open_ms: i64, close_ms: i64) -> Bar {
        Bar {
            open_time: open_ms,
            close_time: close_ms,
            open: Decimal::ONE,
            high: Decimal::ONE,
            low: Decimal::ONE,
            close: Decimal::ONE,
            buy_volume: Decimal::ONE,
            sell_volume: Decimal::ZERO,
            trade_count: 1,
        }
    }

    #[test]
    fn empty_timeline_has_no_positions() {
        let timeline = BarTimeline::from_bars(0, &[], None, None);
        assert!(timeline.is_empty());
        assert_eq!(timeline.timestamp_range(), None);
        assert_eq!(timeline.locate(1), None);
    }

    #[test]
    fn maps_irregular_wall_time_into_equal_bar_slots() {
        let bars = [bar(100, 110), bar(200, 900), bar(1_000, 1_001)];
        let timeline = BarTimeline::from_bars(10, &bars, None, None);

        let first_mid = timeline.locate(150).unwrap();
        assert_eq!(first_mid.bar_index, 10);
        assert!((first_mid.normalized - (0.5 / 3.0)).abs() < 1e-9);

        let second_mid = timeline.locate(600).unwrap();
        assert_eq!(second_mid.bar_index, 11);
        assert!((second_mid.normalized - (1.5 / 3.0)).abs() < 1e-9);
    }

    #[test]
    fn exact_next_open_selects_next_bar_without_an_x_jump() {
        let bars = [bar(100, 120), bar(200, 220)];
        let timeline = BarTimeline::from_bars(0, &bars, None, None);
        let boundary = timeline.locate(200).unwrap();
        assert_eq!(boundary.bar_index, 1);
        assert_eq!(boundary.fraction, 0.0);
        assert!((boundary.normalized - 0.5).abs() < 1e-9);
    }

    #[test]
    fn partial_uses_live_book_time_as_its_right_edge() {
        let closed = [bar(100, 150)];
        let partial = bar(200, 225);
        let timeline = BarTimeline::from_bars(0, &closed, Some(&partial), Some(400));
        let end = timeline.locate(400).unwrap();
        assert_eq!(end.bar_index, 1);
        assert_eq!(end.fraction, 1.0);
        assert_eq!(end.normalized, 1.0);
    }

    #[test]
    fn the_live_lane_is_reserved_before_the_tape_fills_it() {
        // Four closed bars of 100 ms, then a bar that just opened.
        let closed = [bar(0, 100), bar(100, 200), bar(200, 300), bar(300, 400)];
        let partial = bar(400, 400);
        let timeline = BarTimeline::from_bars(0, &closed, Some(&partial), Some(400));

        // The forming slot already spans a typical bar, so the newest print
        // sits at the lane's left edge instead of at its right one.
        assert_eq!(timeline.timestamp_range(), Some((0, 500)));
        assert_eq!(timeline.live_start_ms(), Some(400));
        let now = timeline.locate(400).unwrap();
        assert_eq!(now.bar_index, 4);
        assert_eq!(now.fraction, 0.0);

        // Halfway through a typical bar, the live edge is halfway across the
        // lane: the band fills left to right at a fixed pixels-per-ms scale.
        let half = BarTimeline::from_bars(0, &closed, Some(&bar(400, 450)), Some(450));
        assert_eq!(half.timestamp_range(), Some((0, 500)));
        assert_eq!(half.locate(450).unwrap().fraction, 0.5);
    }

    #[test]
    fn a_bar_outliving_its_reserve_compresses_inside_the_lane() {
        let closed = [bar(0, 100), bar(100, 200)];
        // The forming bar has already run three reference durations.
        let timeline = BarTimeline::from_bars(0, &closed, Some(&bar(200, 500)), Some(500));

        assert_eq!(timeline.timestamp_range(), Some((0, 500)));
        // Nothing spills past the lane and the order is preserved: the early
        // print merely sits closer to its neighbours than it did before.
        let early = timeline.locate(250).unwrap();
        let late = timeline.locate(500).unwrap();
        assert_eq!(early.bar_index, 2);
        assert!((early.fraction - (50.0 / 300.0)).abs() < 1e-9);
        assert_eq!(late.fraction, 1.0);
        assert!(early.normalized < late.normalized);
    }

    #[test]
    fn one_atypical_bar_does_not_calibrate_the_lane() {
        // A session break stretches one bar far beyond the instrument's rhythm.
        let closed = [
            bar(0, 100),
            bar(100, 200),
            bar(200, 60_000),
            bar(60_000, 60_100),
        ];
        assert_eq!(reserved_span_ms(&closed), 100);
        assert_eq!(reserved_span_ms(&[]), DEFAULT_RESERVE_MS);
    }

    #[test]
    fn a_timeline_without_a_live_edge_reserves_nothing() {
        let closed = [bar(0, 100), bar(100, 200)];
        let opening = bar(200, 200);
        let timeline = BarTimeline::from_bars(0, &closed, Some(&opening), None);
        assert_eq!(timeline.timestamp_range(), Some((0, 201)));
        assert_eq!(timeline.live_start_ms(), None);

        // A closed last bar never reserves future time either.
        let history = BarTimeline::from_bars(0, &closed, None, Some(10_000));
        assert_eq!(history.timestamp_range(), Some((0, 10_000)));
        assert_eq!(history.live_start_ms(), None);
    }

    #[test]
    fn locate_rejects_and_locate_clamped_clips_outside_values() {
        let bars = [bar(100, 200)];
        let timeline = BarTimeline::from_bars(7, &bars, None, None);
        assert_eq!(timeline.locate(99), None);
        assert_eq!(timeline.locate(201), None);
        assert_eq!(timeline.locate_clamped(0).unwrap().normalized, 0.0);
        assert_eq!(timeline.locate_clamped(999).unwrap().normalized, 1.0);
    }

    #[test]
    fn degenerate_bar_timestamps_still_have_a_finite_slot() {
        let bars = [bar(100, 100)];
        let timeline = BarTimeline::from_bars(0, &bars, None, None);
        assert_eq!(timeline.timestamp_range(), Some((100, 101)));
        let position = timeline.locate(101).unwrap();
        assert!(position.normalized.is_finite());
        assert_eq!(position.normalized, 1.0);
    }
}
