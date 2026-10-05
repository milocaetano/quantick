//! A tape clock supplied by its host. No wall clock or replay transport lives here.
//! Live time advances between observed prints; replay time is already advanced
//! by the replay source and stops at a print the chart has not applied yet.

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
