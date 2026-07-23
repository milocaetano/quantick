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

/// Mapping from exchange timestamps to the chart's equal-width bar slots.
///
/// Internal boundaries are anchored by the next bar's `open_time`. Therefore
/// book changes between two trades remain visible in the preceding slot
/// instead of disappearing into an unrepresented wall-clock gap.
#[derive(Debug, Clone, Default)]
pub struct BarTimeline {
    slots: Vec<Slot>,
}

impl BarTimeline {
    /// Build a timeline from closed bars and an optional forming bar.
    ///
    /// `first_bar_index` lets a visible slice retain global chart indices.
    /// `live_end_ms` extends only the final slot and is useful when the book has
    /// advanced beyond the forming bar's latest trade.
    #[must_use]
    pub fn from_bars(
        first_bar_index: usize,
        closed: &[Bar],
        partial: Option<&Bar>,
        live_end_ms: Option<i64>,
    ) -> Self {
        let bars: Vec<&Bar> = closed.iter().chain(partial).collect();
        let mut slots = Vec::with_capacity(bars.len());

        for (offset, bar) in bars.iter().enumerate() {
            let start_ms = bar.open_time;
            let natural_end = bars
                .get(offset + 1)
                .map_or(bar.close_time, |next| next.open_time);
            let live_end = if offset + 1 == bars.len() {
                live_end_ms.unwrap_or(natural_end)
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

        Self { slots }
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
