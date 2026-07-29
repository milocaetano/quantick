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

/// How many recent closed bars decide how much market time the lane shows.
///
/// A single bar is too fragile a reference: one session break, one burst bar,
/// and the lane would be calibrated to a duration that never repeats. A median
/// over a short window follows the instrument's rhythm without letting one
/// outlier set it.
const RESERVE_SAMPLE_BARS: usize = 8;

/// Market time the lane shows before any bar has closed, in exchange
/// milliseconds. Only the very first bars of a fresh series see it.
const DEFAULT_RESERVE_MS: i64 = 1_000;

/// Shortest window the lane will show, in exchange milliseconds.
///
/// Bar durations are wildly uneven on an activity-sampled series: a burst
/// closes a tick bar in well under a second while a quiet stretch takes a
/// minute, so the median alone can collapse to almost nothing right after a
/// busy patch and leave the tape empty. Below a few seconds there is no tape
/// left to read, whatever the bars did.
const MIN_LANE_SPAN_MS: i64 = 4_000;

/// How much market time the live lane shows: the typical duration of the recent
/// closed bars.
///
/// Constant per frame, which is what makes the lane a tape. The window's right
/// edge is always the live edge, so a print enters there and slides left at a
/// fixed pixels-per-ms rate for exactly this long before leaving. Deriving it
/// from the bars means the lane holds roughly one bar's worth of flow whatever
/// the instrument or the bar type — without ever being tied to a particular
/// bar, which is what would make a bar close empty it.
#[must_use]
pub fn reserved_span_ms(closed: &[Bar]) -> i64 {
    let mut durations: Vec<i64> = closed
        .iter()
        .rev()
        .take(RESERVE_SAMPLE_BARS)
        .map(|bar| bar.close_time.saturating_sub(bar.open_time).max(1))
        .collect();
    let typical = if durations.is_empty() {
        DEFAULT_RESERVE_MS
    } else {
        durations.sort_unstable();
        durations[durations.len() / 2]
    };
    typical.max(MIN_LANE_SPAN_MS)
}

/// The rolling window of market time drawn to the right of every bar slot.
///
/// It always ends at the live edge and always covers the same span, so it is a
/// tape rather than a region that belongs to any one bar: the newest print
/// enters at its right edge, everything already in it slides left at a fixed
/// pixels-per-ms rate, and a print leaves through the left edge into the slot
/// of the bar it happened in. A bar closing is not an event here — nothing
/// empties, nothing restarts, and the eye never loses the book.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Lane {
    start_ms: i64,
    end_ms: i64,
}

/// One bar slot and the market time it represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotSpan {
    /// Position among the timeline's regions, for [`BarTimeline::slot_bounds`].
    pub index: usize,
    /// Global bar index of the bar this slot draws.
    pub bar_index: usize,
    /// Exchange timestamp the slot opens at.
    pub start_ms: i64,
    /// Exchange timestamp the slot ends at.
    pub end_ms: i64,
}

/// The live edge, as one visible slice of bars sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveEdge {
    /// Newest instant any recorded stream has reached.
    pub now_ms: i64,
    /// Market time the lane shows, ending at [`now_ms`](Self::now_ms).
    pub window_ms: i64,
    /// Whether the last bar supplied is the newest bar of the series.
    ///
    /// Only then may its slot be stretched to the live edge. The lane is
    /// pinned to the right of the chart and keeps running while the candles
    /// are panned into history; stretching the last *visible* bar to now in
    /// that state would draw the prints of every bar after it inside a bar
    /// they never belonged to.
    pub on_newest_bar: bool,
}

/// Mapping from exchange timestamps to the chart's equal-width bar slots, plus
/// the live lane past the last of them.
///
/// Internal boundaries are anchored by the next bar's `open_time`. Therefore
/// book changes between two trades remain visible in the preceding slot
/// instead of disappearing into an unrepresented wall-clock gap.
#[derive(Debug, Clone, Default)]
pub struct BarTimeline {
    slots: Vec<Slot>,
    lane: Option<Lane>,
}

impl BarTimeline {
    /// Build a timeline from closed bars and an optional forming bar.
    ///
    /// `first_bar_index` lets a visible slice retain global chart indices.
    /// Supplying a [`LiveEdge`] appends the live lane, covering the window of
    /// market time that ends at it.
    #[must_use]
    pub fn from_bars(
        first_bar_index: usize,
        closed: &[Bar],
        partial: Option<&Bar>,
        live: Option<LiveEdge>,
    ) -> Self {
        let bars: Vec<&Bar> = closed.iter().chain(partial).collect();
        let mut slots = Vec::with_capacity(bars.len());

        for (offset, bar) in bars.iter().enumerate() {
            let start_ms = bar.open_time;
            let natural_end = bars
                .get(offset + 1)
                .map_or(bar.close_time, |next| next.open_time);
            let live_end = match live {
                Some(edge) if edge.on_newest_bar && offset + 1 == bars.len() => edge.now_ms,
                _ => natural_end,
            };
            let end_ms = natural_end.max(live_end).max(start_ms.saturating_add(1));
            slots.push(Slot {
                bar_index: first_bar_index + offset,
                start_ms,
                end_ms,
            });
        }

        // The lane belongs to the tape, not to a bar: it is the last window of
        // market time, whoever was forming at the time. That is exactly what
        // keeps a bar close from resetting it.
        let lane = live.map(|edge| Lane {
            start_ms: edge.now_ms.saturating_sub(edge.window_ms.max(1)),
            end_ms: edge.now_ms,
        });
        Self { slots, lane }
    }

    /// Exchange timestamp where the live lane begins.
    ///
    /// Prints at or after it are inside the rolling window and get the lane's
    /// own treatment; everything before it has left the tape and belongs to
    /// the slot of its bar, where the chart is free to summarize it. `None`
    /// when this timeline follows no live edge.
    #[must_use]
    pub fn lane_start_ms(&self) -> Option<i64> {
        Some(self.lane?.start_ms)
    }

    /// Position of the live edge, which is the lane's right edge whenever a
    /// lane exists. Marks never sit to its right.
    #[must_use]
    pub fn live_now_position(&self) -> Option<TimelinePosition> {
        self.locate_clamped(self.lane?.end_ms)
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

    /// Number of equal-width regions the normalized x axis is divided into:
    /// one per bar, plus the lane when there is one.
    #[must_use]
    pub fn region_count(&self) -> usize {
        self.slots.len() + usize::from(self.lane.is_some())
    }

    /// Inclusive timestamp bounds represented by the timeline.
    #[must_use]
    pub fn timestamp_range(&self) -> Option<(i64, i64)> {
        let start = self.slots.first()?.start_ms;
        let end = self.slots.last()?.end_ms;
        Some((start, self.lane.map_or(end, |lane| end.max(lane.end_ms))))
    }

    /// Locate a timestamp. Values outside coverage return `None`.
    #[must_use]
    pub fn locate(&self, timestamp_ms: i64) -> Option<TimelinePosition> {
        if !self.covers(timestamp_ms) {
            return None;
        }
        Some(self.locate_in_range(timestamp_ms))
    }

    /// Whether any region of this timeline represents `timestamp_ms`.
    ///
    /// Between the last bar slot and the lane there can be a stretch of market
    /// time nothing represents: the lane is pinned to the right of the chart
    /// and keeps ending at now while the candles are panned into history, so
    /// the bars in between are simply off screen. Their prints are dropped
    /// rather than drawn at the edge of a bar they never belonged to.
    #[must_use]
    fn covers(&self, timestamp_ms: i64) -> bool {
        let Some((first, last)) = self.timestamp_range() else {
            return false;
        };
        if timestamp_ms < first || timestamp_ms > last {
            return false;
        }
        let slots_end = self.slots.last().map_or(i64::MIN, |slot| slot.end_ms);
        timestamp_ms <= slots_end || self.in_lane(timestamp_ms)
    }

    #[must_use]
    fn in_lane(&self, timestamp_ms: i64) -> bool {
        self.lane
            .is_some_and(|lane| timestamp_ms >= lane.start_ms && timestamp_ms <= lane.end_ms)
    }

    /// Locate a timestamp in its bar's slot, ignoring the lane.
    ///
    /// The lane is a second place the same instant can be drawn, and it wins in
    /// [`locate`](Self::locate) because that is where a raw print belongs while
    /// it is still on the tape. A closed bar's summary is about the bar, so it
    /// asks for the slot even when the prints behind it are still rolling.
    #[must_use]
    pub fn locate_in_slot(&self, timestamp_ms: i64) -> Option<TimelinePosition> {
        self.covers(timestamp_ms)
            .then(|| self.slot_position(timestamp_ms))
    }

    fn slot_position(&self, timestamp_ms: i64) -> TimelinePosition {
        let partition = self
            .slots
            .partition_point(|slot| slot.start_ms <= timestamp_ms);
        let slot_index = partition.saturating_sub(1).min(self.slots.len() - 1);
        let slot = self.slots[slot_index];
        let span = (slot.end_ms - slot.start_ms).max(1) as f64;
        let fraction = ((timestamp_ms - slot.start_ms) as f64 / span).clamp(0.0, 1.0);
        TimelinePosition {
            bar_index: slot.bar_index,
            fraction,
            normalized: (slot_index as f64 + fraction) / self.region_count() as f64,
        }
    }

    /// The bar slots a time range touches, in order.
    ///
    /// What the candles' pane needs to summarize a run: which bars it was
    /// resting through, and for how much of each. Empty when the range falls
    /// entirely outside the represented bars.
    pub fn slots_between(&self, start_ms: i64, end_ms: i64) -> impl Iterator<Item = SlotSpan> + '_ {
        let first = self
            .slots
            .partition_point(|slot| slot.end_ms <= start_ms)
            .min(self.slots.len());
        let last = self.slots.partition_point(|slot| slot.start_ms < end_ms);
        self.slots[first..last.max(first)]
            .iter()
            .enumerate()
            .map(move |(offset, slot)| SlotSpan {
                index: first + offset,
                bar_index: slot.bar_index,
                start_ms: slot.start_ms,
                end_ms: slot.end_ms,
            })
    }

    /// Normalized x of a whole slot: `[left, right)` of its region.
    #[must_use]
    pub fn slot_bounds(&self, index: usize) -> (f64, f64) {
        let regions = self.region_count().max(1) as f64;
        (index as f64 / regions, (index + 1) as f64 / regions)
    }

    /// Locate a timestamp inside the lane, clipping it to the lane's own
    /// window. `None` when this timeline has no lane.
    ///
    /// The tape's half of the same question: a run that reaches into the
    /// window is drawn there too, from wherever the window opens.
    #[must_use]
    pub fn locate_in_lane_clamped(&self, timestamp_ms: i64) -> Option<TimelinePosition> {
        let lane = self.lane?;
        Some(self.locate_in_range(timestamp_ms.clamp(lane.start_ms, lane.end_ms)))
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
        // the prior slot's right edge. The slot is resolved even for a print
        // drawn in the lane: a print is always *from* some bar, which is what
        // the closed-bar summary groups on, and it is drawn in that bar's slot
        // the moment it leaves the rolling window.
        let partition = self
            .slots
            .partition_point(|slot| slot.start_ms <= timestamp_ms);
        let slot_index = partition.saturating_sub(1).min(self.slots.len() - 1);
        let slot = self.slots[slot_index];
        // Inside the rolling window the lane wins over the bar slot, however
        // many bars the window happens to span. Bars keep their slots; the
        // tape is simply drawn once, in the newest place it belongs.
        let (index, start_ms, end_ms) = match self.lane {
            Some(lane) if timestamp_ms >= lane.start_ms => {
                (self.slots.len(), lane.start_ms, lane.end_ms)
            }
            _ => (slot_index, slot.start_ms, slot.end_ms),
        };
        let span = (end_ms - start_ms).max(1) as f64;
        let fraction = ((timestamp_ms - start_ms) as f64 / span).clamp(0.0, 1.0);
        TimelinePosition {
            bar_index: slot.bar_index,
            fraction,
            normalized: (index as f64 + fraction) / self.region_count() as f64,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;

    /// The live edge as the chart supplies it while the newest bar is on
    /// screen: the lane shows the recent bars' typical duration, unzoomed.
    fn live(now_ms: i64, closed: &[Bar]) -> Option<LiveEdge> {
        Some(LiveEdge {
            now_ms,
            window_ms: reserved_span_ms(closed),
            on_newest_bar: true,
        })
    }

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
        let timeline = BarTimeline::from_bars(0, &closed, Some(&partial), live(400, &closed));
        let end = timeline.locate(400).unwrap();
        assert_eq!(end.bar_index, 1);
        assert_eq!(end.fraction, 1.0);
        assert_eq!(end.normalized, 1.0);
    }

    /// The lane is a window on market time, so the newest print is always at
    /// its right edge and an older one is always further left, by exactly how
    /// old it is. That is the whole "sliding tape" contract.
    #[test]
    fn the_lane_is_a_window_ending_at_the_live_edge() {
        // Four closed bars of ten seconds, then a bar that just opened.
        let closed = [
            bar(0, 10_000),
            bar(10_000, 20_000),
            bar(20_000, 30_000),
            bar(30_000, 40_000),
        ];
        let timeline = BarTimeline::from_bars(
            0,
            &closed,
            Some(&bar(40_000, 40_000)),
            live(40_000, &closed),
        );

        // Five bar slots plus the lane; the window is one typical bar wide.
        assert_eq!(timeline.region_count(), 6);
        assert_eq!(timeline.lane_start_ms(), Some(30_000));
        // 40_001 rather than 40_000: an instant-wide forming slot still gets
        // the one-millisecond floor every degenerate slot gets.
        assert_eq!(timeline.timestamp_range(), Some((0, 40_001)));

        let now = timeline.locate(40_000).unwrap();
        assert_eq!(now.fraction, 1.0, "the live edge is the lane's right edge");
        assert_eq!(now.normalized, 1.0);
        // Half a window old, so half a lane to the left — whichever bar it
        // belongs to.
        let older = timeline.locate(35_000).unwrap();
        assert_eq!(older.fraction, 0.5);
        assert!((older.normalized - 5.5 / 6.0).abs() < 1e-9);
        assert_eq!(older.bar_index, 3, "the bar it happened in is still known");
    }

    /// The reset the sliding tape exists to remove: the same print keeps the
    /// same distance from the live edge across a bar close.
    #[test]
    fn a_bar_closing_does_not_move_what_is_already_on_the_tape() {
        let closed = [bar(0, 10_000), bar(10_000, 20_000)];
        // Same instant, same live edge, but one more bar has closed.
        let forming = BarTimeline::from_bars(
            0,
            &closed,
            Some(&bar(20_000, 25_000)),
            live(25_000, &closed),
        );
        let after_close = BarTimeline::from_bars(
            0,
            &[bar(0, 10_000), bar(10_000, 20_000), bar(20_000, 25_000)],
            Some(&bar(25_000, 25_000)),
            live(25_000, &closed),
        );

        for timestamp in [18_000, 20_000, 22_000, 25_000] {
            let before = forming.locate(timestamp).unwrap();
            let after = after_close.locate(timestamp).unwrap();
            assert_eq!(
                before.fraction, after.fraction,
                "print at {timestamp} jumped inside the lane when the bar closed"
            );
        }
        // The lane itself never empties either: it still ends at the same edge
        // and still reaches the same way back.
        assert_eq!(forming.lane_start_ms(), after_close.lane_start_ms());
    }

    /// Ageing out of the window is what moves a print off the tape — not its
    /// bar closing. It lands in the slot of the bar it happened in.
    #[test]
    fn a_print_leaves_the_lane_into_the_slot_of_its_own_bar() {
        let closed = [bar(0, 10_000), bar(10_000, 20_000)];
        let timeline = BarTimeline::from_bars(
            0,
            &closed,
            Some(&bar(20_000, 40_000)),
            live(40_000, &closed),
        );
        // A ten-second window ending at 40 s: everything before 30 s has left.
        assert_eq!(timeline.lane_start_ms(), Some(30_000));

        let aged_out = timeline.locate(15_000).unwrap();
        assert_eq!(aged_out.bar_index, 1);
        // Region 1 of 4 (three bars plus the lane), halfway across its slot.
        assert!((aged_out.normalized - 1.5 / 4.0).abs() < 1e-9);

        let on_tape = timeline.locate(35_000).unwrap();
        assert!(on_tape.normalized > 3.0 / 4.0, "still in the lane");
        assert_eq!(on_tape.bar_index, 2);
    }

    /// The lane is pinned to the chart, not to the visible bars: panning into
    /// history keeps the tape running, and the bars that scrolled off the
    /// right are simply not drawn — never folded into the last visible one.
    #[test]
    fn a_slice_panned_into_history_keeps_the_lane_and_drops_the_gap() {
        // Two visible bars ending at 20 s, while the live edge is at 100 s.
        let visible = [bar(0, 10_000), bar(10_000, 20_000)];
        let timeline = BarTimeline::from_bars(
            0,
            &visible,
            None,
            Some(LiveEdge {
                now_ms: 100_000,
                window_ms: 10_000,
                on_newest_bar: false,
            }),
        );

        assert_eq!(timeline.region_count(), 3, "two bars plus the lane");
        assert_eq!(timeline.lane_start_ms(), Some(90_000));
        // The last visible slot keeps its own end: it is not the newest bar,
        // so nothing stretches it to the live edge.
        assert_eq!(timeline.locate(20_000).unwrap().fraction, 1.0);
        assert_eq!(timeline.locate(20_001), None);
        // Everything between the visible bars and the tape belongs to bars off
        // screen to the right, and is drawn nowhere at all.
        assert_eq!(timeline.locate(50_000), None);
        assert_eq!(timeline.locate_in_slot(50_000), None);
        // The tape itself is unaffected by where the candles are pointed.
        let on_tape = timeline.locate(95_000).unwrap();
        assert!((on_tape.normalized - 2.5 / 3.0).abs() < 1e-9);
        assert_eq!(timeline.live_now_position().unwrap().normalized, 1.0);
    }

    /// The window is given, not guessed: the zoom lives in the config and the
    /// timeline draws exactly the window it was handed.
    #[test]
    fn the_lane_shows_the_window_it_was_given() {
        let closed = [bar(0, 10_000), bar(10_000, 20_000)];
        for window_ms in [1_000, 5_000, 40_000] {
            let timeline = BarTimeline::from_bars(
                0,
                &closed,
                Some(&bar(20_000, 25_000)),
                Some(LiveEdge {
                    now_ms: 25_000,
                    window_ms,
                    on_newest_bar: true,
                }),
            );
            assert_eq!(timeline.lane_start_ms(), Some(25_000 - window_ms));
        }
    }

    #[test]
    fn neither_an_outlier_nor_a_burst_can_wreck_the_window() {
        // A session break stretches one bar far beyond the instrument's rhythm:
        // the median ignores it.
        let broken = [
            bar(0, 10_000),
            bar(10_000, 20_000),
            bar(20_000, 300_000),
            bar(300_000, 310_000),
        ];
        assert_eq!(reserved_span_ms(&broken), 10_000);

        // A burst closes tick bars in milliseconds. Following the median down
        // there would leave the tape holding an instant and reading as empty,
        // which is what the floor is for.
        let burst = [bar(0, 5), bar(5, 9), bar(9, 20), bar(20, 26)];
        assert_eq!(reserved_span_ms(&burst), MIN_LANE_SPAN_MS);
        assert_eq!(
            reserved_span_ms(&[]),
            DEFAULT_RESERVE_MS.max(MIN_LANE_SPAN_MS)
        );
    }

    #[test]
    fn a_timeline_without_a_live_edge_has_no_lane() {
        let closed = [bar(0, 100), bar(100, 200)];
        let timeline = BarTimeline::from_bars(0, &closed, Some(&bar(200, 200)), None);
        assert_eq!(timeline.region_count(), 3, "three bars, no lane");
        assert_eq!(timeline.timestamp_range(), Some((0, 201)));
        assert_eq!(timeline.lane_start_ms(), None);
        assert_eq!(timeline.live_now_position(), None);
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
