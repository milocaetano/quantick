//! Where the tape's window ends: pinned to the live edge, or held at a past
//! instant the trader panned to. The host tells it the live edge; nothing
//! here reads a clock.

/// How far the window may reach before the first retained print, as a share
/// of itself, when the tape is panned to the start of its history. The
/// boundary then sits inside the tape, labelled, instead of scrolling out of
/// view — an empty stretch the trader can see is history the chart never had.
pub const RETAINED_EDGE_SHARE: f64 = 0.5;

/// What a held tape says on its top edge: it is not now.
pub const PAST_TAPE_LABEL: &str = "past · double-click for live";

/// What the retained tape's first instant says inside a past window.
pub const RETAINED_EDGE_LABEL: &str = "no tape retained before";

/// Where the first complete retained instant falls across a window ending at
/// `end_ms`, as a fraction of it; `None` while the whole window is retained.
#[must_use]
pub fn retained_edge_fraction(
    end_ms: i64,
    window_ms: i64,
    retained_from_ms: Option<i64>,
) -> Option<f64> {
    let start = end_ms.saturating_sub(window_ms);
    let from = retained_from_ms.filter(|from| window_ms > 0 && *from > start)?;
    Some(((from - start) as f64 / window_ms as f64).min(1.0))
}

/// The instant the tape's right edge stands for.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TapeEnd {
    /// The right edge is now: prints enter it and slide left.
    #[default]
    Live,
    /// The right edge holds a past instant; the tape is frozen there.
    Past { end_ms: i64 },
}

impl TapeEnd {
    #[must_use]
    pub const fn is_live(self) -> bool {
        matches!(self, Self::Live)
    }

    #[must_use]
    pub const fn past_ms(self) -> Option<i64> {
        match self {
            Self::Live => None,
            Self::Past { end_ms } => Some(end_ms),
        }
    }

    /// The right edge's instant, given where the live edge is.
    #[must_use]
    pub const fn end_ms(self, live_ms: i64) -> i64 {
        match self {
            Self::Live => live_ms,
            Self::Past { end_ms } => end_ms,
        }
    }

    /// `requested_ms` as an end: at or after the live edge it is live, and
    /// it never reaches back past [`RETAINED_EDGE_SHARE`] of the window
    /// before the first complete retained instant. History too short to hold
    /// that is live too — there is no past to show.
    #[must_use]
    pub fn clamped(
        requested_ms: i64,
        live_ms: i64,
        window_ms: i64,
        retained_from_ms: Option<i64>,
    ) -> Self {
        let reach = (window_ms.max(0) as f64 * RETAINED_EDGE_SHARE) as i64;
        let earliest = retained_from_ms.map_or(i64::MIN, |from| from.saturating_add(reach));
        if requested_ms >= live_ms || earliest >= live_ms {
            return Self::Live;
        }
        Self::Past {
            end_ms: requested_ms.max(earliest),
        }
    }

    /// The same end re-clamped after the live edge, the window or retention
    /// moved under it.
    #[must_use]
    pub fn reclamped(self, live_ms: i64, window_ms: i64, retained_from_ms: Option<i64>) -> Self {
        match self {
            Self::Live => Self::Live,
            Self::Past { end_ms } => Self::clamped(end_ms, live_ms, window_ms, retained_from_ms),
        }
    }

    /// A horizontal drag of `delta_px` over a tape `span_px` wide showing
    /// `window_ms`. Rightward reveals older prints, the way a drag moves the
    /// candles; dragging past now pins the tape to live again.
    #[must_use]
    pub fn panned(
        self,
        delta_px: f32,
        span_px: f32,
        live_ms: i64,
        window_ms: i64,
        retained_from_ms: Option<i64>,
    ) -> Self {
        if !delta_px.is_finite() || delta_px == 0.0 || !span_px.is_finite() || span_px <= 0.0 {
            return self;
        }
        let shift = (f64::from(delta_px) * window_ms as f64 / f64::from(span_px)).round() as i64;
        Self::clamped(
            self.end_ms(live_ms).saturating_sub(shift),
            live_ms,
            window_ms,
            retained_from_ms,
        )
    }
}
