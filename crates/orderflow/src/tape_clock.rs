//! A tape clock supplied by its host. No wall clock or replay transport lives here.
//! Live time advances between observed prints; replay time is already advanced
//! by the replay source and stops at a print the chart has not applied yet.

/// How much of its window a quiet tape leaves empty after its last print: the
/// print stops at 55 % of the side, where the seam stands.
pub const TAPE_HOLD_ROOM_PERCENT: i64 = 45;

/// A tape nothing is coming to: its window stops advancing once the last print
/// has walked [`TAPE_HOLD_ROOM_PERCENT`] of it, so what it retains stays on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TapeHold {
    pub last_print_ms: i64,
    pub window_ms: i64,
}

impl TapeHold {
    /// The latest instant the window may end at.
    #[must_use]
    pub const fn cap_ms(self) -> i64 {
        let window = if self.window_ms > 0 {
            self.window_ms
        } else {
            0
        };
        let room =
            window / 100 * TAPE_HOLD_ROOM_PERCENT + window % 100 * TAPE_HOLD_ROOM_PERCENT / 100;
        self.last_print_ms.saturating_add(room)
    }

    /// Where the window ends with the clock at `now_ms`.
    #[must_use]
    pub const fn window_end_ms(self, now_ms: i64) -> i64 {
        let cap = self.cap_ms();
        if now_ms < cap { now_ms } else { cap }
    }
}

#[derive(Debug, Default)]
pub struct TapeClock {
    now_ms: Option<i64>,
    monotonic_ms: Option<u64>,
}

impl TapeClock {
    #[must_use]
    pub const fn now_ms(&self) -> Option<i64> {
        self.now_ms
    }

    /// Begin another market or replay epoch without carrying an old anchor.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Advance in the market's clock by elapsed host-monotonic milliseconds.
    /// A late print never makes time run backwards; a newer factual timestamp
    /// may advance the edge. An empty source carries no clock to extrapolate.
    pub fn live_at(&mut self, applied_ms: Option<i64>, monotonic_ms: u64) -> Option<i64> {
        let Some(applied_ms) = applied_ms else {
            self.reset();
            return None;
        };
        let now_ms = match (self.now_ms, self.monotonic_ms) {
            (Some(now_ms), Some(previous)) => {
                let elapsed =
                    i64::try_from(monotonic_ms.saturating_sub(previous)).unwrap_or(i64::MAX);
                now_ms.saturating_add(elapsed).max(applied_ms)
            }
            _ => applied_ms,
        };
        self.monotonic_ms = Some(self.monotonic_ms.unwrap_or(0).max(monotonic_ms));
        self.now_ms = Some(now_ms);
        self.now_ms
    }

    /// The source playhead already accounts for pause, speed and seek. Clamp
    /// it at the first unapplied print so a released backlog cannot vanish
    /// off the left before the chart consumes it. A briefly stale playhead
    /// cannot move behind the data already applied to this chart.
    pub fn replay_at(
        &mut self,
        position_ms: i64,
        applied_ms: Option<i64>,
        next_unapplied_ms: Option<i64>,
    ) -> Option<i64> {
        self.monotonic_ms = None;
        self.now_ms = applied_ms.map(|applied_ms| {
            position_ms
                .min(next_unapplied_ms.unwrap_or(i64::MAX))
                .max(applied_ms)
        });
        self.now_ms
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOLD: TapeHold = TapeHold {
        last_print_ms: 100_000,
        window_ms: 15_000,
    };

    #[test]
    fn the_window_runs_on_until_the_print_has_walked_its_share_then_stops() {
        assert_eq!(HOLD.cap_ms(), 106_750);
        assert_eq!(HOLD.window_end_ms(100_000), 100_000);
        assert_eq!(HOLD.window_end_ms(106_749), 106_749);
        assert_eq!(HOLD.window_end_ms(106_750), 106_750);
        assert_eq!(HOLD.window_end_ms(160_000), 106_750, "a minute on");
    }

    #[test]
    fn the_print_stands_at_fifty_five_percent_of_the_held_window() {
        for window_ms in [1_000, 15_000, 60_000, 299] {
            let hold = TapeHold {
                last_print_ms: 5_000,
                window_ms,
            };
            let start = hold.cap_ms() - window_ms;
            let at = (5_000 - start) as f64 / window_ms as f64;
            assert!((at - 0.55).abs() < 0.01, "{window_ms}: {at}");
        }
    }

    #[test]
    fn a_degenerate_window_holds_at_the_print_and_an_extreme_one_does_not_wrap() {
        let empty = TapeHold {
            last_print_ms: 5_000,
            window_ms: -1,
        };
        assert_eq!(empty.cap_ms(), 5_000);
        let huge = TapeHold {
            last_print_ms: i64::MAX - 1,
            window_ms: i64::MAX,
        };
        assert_eq!(huge.cap_ms(), i64::MAX);
    }
}
