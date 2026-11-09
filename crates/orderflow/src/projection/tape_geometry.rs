//! Shared tape radius limits and insets for dots, clock labels and price fit.

use crate::HeatmapConfig;
use crate::config::{BubbleScaleTarget, BubbleStyle, bubble_halo_padding};
use crate::projection::HeatmapCell;

use super::constants::{MAX_DECORATED_INSET_SHARE, MAX_VERTICAL_RADIUS_SHARE};

/// The tape's padded linear time span and the uniform radius it can display.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TapeHorizontalGeometry {
    /// Largest disc radius, capped uniformly when the pane cannot fit it.
    pub max_radius: f32,
    /// Minimum axis padding above and below the tape's factual price range.
    pub price_inset_px: f32,
    /// Distance in pixels from the pane edge to the oldest visible timestamp.
    pub inset_px: f32,
    /// Distance in pixels from the oldest visible timestamp to NOW.
    pub span_px: f32,
    /// The Tape display gain these insets hold: the configured
    /// `display_scale.tape`, lowered only where the pane cannot fit the
    /// magnified largest disc. The painter magnifies by this, never by the
    /// raw gain, so the newest dot stays whole at NOW.
    pub display_gain: f32,
}

impl TapeHorizontalGeometry {
    /// The native time geometry the tape's book, reduction fronts and
    /// picking share with its dots; `None` keeps them on the linear lane.
    ///
    /// Only a native tape with depth time-averaging on moves its book onto
    /// the dots' inset axis. Averaging off is the legacy display
    /// ([`LiveLaneStyle::depth_time_averaging`](crate::config::LiveLaneStyle::depth_time_averaging)):
    /// the book and its fronts keep the linear lane they were approved on,
    /// while the dots keep their own inset ([`Self::resolve`]).
    #[must_use]
    pub fn from_style(
        style: &crate::config::theme::OrderflowRenderStyle,
        width: f32,
        height: f32,
    ) -> Option<Self> {
        (style.native_tape && style.live_lane.depth_time_averaging)
            .then(|| Self::resolve(width, height, &style.sanitized().bubbles))
    }

    /// Resolve a common inset. Every volume retains its area ratio when a
    /// narrow or short pane requires a smaller largest radius; no dot is individually
    /// displaced. A pane exactly one diameter wide has a zero time span.
    #[must_use]
    pub fn resolve(width_px: f32, height_px: f32, bubbles: &BubbleStyle) -> Self {
        let extent = |value: f32| {
            if value.is_finite() {
                value.max(0.0)
            } else {
                0.0
            }
        };
        let (width, height) = (extent(width_px), extent(height_px));
        let fit = |radius: f32| {
            radius
                .max(0.0)
                .min(width / 2.0)
                .min(height * MAX_VERTICAL_RADIUS_SHARE)
        };
        let max_radius = fit(bubbles.max_radius);
        // The Tape gain magnifies every drawn disc after sizing, so the
        // insets pad for the magnified largest disc, never the sized one.
        let gain = bubbles.display_scale.get(BubbleScaleTarget::Tape);
        let drawn_radius = fit(max_radius * gain);
        let decorated = drawn_radius + bubble_halo_padding(drawn_radius) + bubbles.outline_width;
        let inset_px = decorated
            .min(width * MAX_DECORATED_INSET_SHARE)
            .max(drawn_radius);
        Self {
            max_radius,
            price_inset_px: decorated
                .min(height * MAX_DECORATED_INSET_SHARE)
                .max(drawn_radius),
            inset_px,
            span_px: (width - 2.0 * inset_px).max(0.0),
            display_gain: if max_radius > 0.0 {
                drawn_radius / max_radius
            } else {
                gain
            },
        }
    }

    /// The inset the native tape pads the price axis fit with, on a chart
    /// this wide and tall; `0` when the pane does not draw the native tape.
    ///
    /// Measured on the lane setting's own band, never on a divider already
    /// laid out: before the tape has a live edge no band is on screen, and a
    /// fit padded for no band would step the moment the band appears.
    #[must_use]
    pub fn native_price_inset_px(config: &HeatmapConfig, chart_width: f32, height: f32) -> f32 {
        if !config.native_tape() {
            return 0.0;
        }
        let width = config.live_lane.resolved_width_px(chart_width);
        Self::resolve(width, height, &config.bubbles).price_inset_px
    }

    /// Pixel coordinate relative to the pane's left edge for this time
    /// fraction: zero is the oldest visible timestamp and one is NOW.
    #[must_use]
    pub fn x(self, fraction: f64) -> f32 {
        self.inset_px + fraction as f32 * self.span_px
    }

    /// [`Self::from_style`] for a caller holding the configuration, on the
    /// same sanitized bubbles, so the view and the painter agree on one
    /// inset.
    #[must_use]
    pub fn from_config(config: &HeatmapConfig, width: f32, height: f32) -> Option<Self> {
        (config.native_tape() && config.live_lane.depth_time_averaging).then(|| {
            let mut bubbles = config.bubbles.clone();
            bubbles.sanitize();
            Self::resolve(width, height, &bubbles)
        })
    }

    /// The share of the lane's window the book reads beyond each side of
    /// it, under the insets on the dots' own clock: the live book's lead
    /// ([`crate::BarTimeline::with_lane_lead_ms`]), and a held tape's lead
    /// and trail. The inset's own share with [`LEAD_READ_MARGIN`], at most
    /// [`MAX_LEAD_SHARE`]; the inset beyond it is a labelled gap
    /// ([`BOOK_BEYOND_LEAD`](crate::projection::BOOK_BEYOND_LEAD)). A pane
    /// with no time span left has no clock to place depth on: its share is
    /// infinite, which reads nothing and marks both insets as gaps.
    #[must_use]
    pub fn lead_share(self) -> f64 {
        if self.inset_px <= 0.0 {
            0.0
        } else if self.span_px > 0.0 {
            (f64::from(self.inset_px / self.span_px) * LEAD_READ_MARGIN).min(MAX_LEAD_SHARE)
        } else {
            f64::INFINITY
        }
    }

    /// Pixels from the lane's left edge a span `(from, to)` in lane
    /// fractions covers on the dots' clock: below zero the lead, above one
    /// the stretch past NOW. Cut at both pane edges; an infinite end reaches
    /// the edge on its side. With no time span left every finite time sits
    /// in the one column at the inset, so nothing is stretched across an
    /// inset; only a span running out to infinity covers one.
    /// `None` for no span.
    #[must_use]
    pub fn lead_px(self, span: Option<(f64, f64)>) -> Option<(f32, f32)> {
        let width = f64::from(self.x(1.0) + self.inset_px);
        let px = |fraction: f64| {
            let x = if self.span_px > 0.0 {
                f64::from(self.inset_px) + fraction * f64::from(self.span_px)
            } else if fraction == f64::NEG_INFINITY {
                0.0
            } else if fraction == f64::INFINITY {
                width
            } else {
                f64::from(self.inset_px)
            };
            if x.is_nan() {
                0.0
            } else {
                x.clamp(0.0, width) as f32
            }
        };
        span.map(|(from, to)| (px(from), px(to)))
    }

    /// Pixels from the lane's left edge a book cell of a frame with
    /// `regions` slots is painted over, where the lane's own opening-to-NOW
    /// mapping would cut it short: older depth in the lead left of the
    /// oldest dot ([`HeatmapCell::lead_span`]), and a current row, which
    /// ends at NOW, carried on to the lane's right edge while the frame's
    /// current book is live. `None` leaves the cell where `x0..x1` puts it.
    #[must_use]
    pub fn book_span_px(
        self,
        cell: &HeatmapCell,
        regions: usize,
        current_book_live: bool,
    ) -> Option<(f32, f32)> {
        if cell.lead_span.is_some() {
            return self.lead_px(cell.lead_span);
        }
        (cell.current_book && current_book_live).then(|| {
            let regions = regions.max(1) as f64;
            let from = (finite_unit_f64(cell.x0) * regions - (regions - 1.0)).max(0.0);
            (self.x(from), self.x(1.0) + self.inset_px)
        })
    }
}

/// The most depth the native tape reads beyond each side of its window, in
/// lane windows.
///
/// The inset is capped at 45% of the pane a side, but never below the
/// largest disc, so at the largest bubbles a narrow pane keeps only a sliver
/// of time span and its insets ask for many windows. Eight cover a lane down
/// to about 2.3 insets wide; past that the read stops, and the rest of the
/// inset is a labelled gap
/// ([`BOOK_BEYOND_LEAD`](crate::projection::BOOK_BEYOND_LEAD)), never a dark
/// strip.
pub const MAX_LEAD_SHARE: f64 = 8.0;

/// How much longer than its inset the lead reads. The view sizes the lead
/// and the painter places it on separate frames, so a pane resized or a
/// divider dragged between them would otherwise leave a sliver at the edge;
/// what the margin over-reads is cut there.
pub const LEAD_READ_MARGIN: f64 = 1.25;

/// The inset past NOW of a live frame whose current book is no longer
/// current: a [`BOOK_NOT_CURRENT`](crate::projection::BOOK_NOT_CURRENT) gap
/// from NOW to the pane's right edge, where a live book would be previewed
/// ([`TapeHorizontalGeometry::book_span_px`]). `None` while the book is live
/// or when the frame read none.
#[must_use]
pub fn stale_book_gap(
    projection: &crate::projection::HeatmapProjection,
    current_book_live: bool,
) -> Option<crate::projection::GapPrimitive> {
    (!current_book_live && projection.current_book.is_some()).then(|| {
        crate::projection::GapPrimitive {
            from_generation: None,
            to_generation: None,
            x0: 1.0,
            x1: 1.0,
            reason: crate::projection::BOOK_NOT_CURRENT.to_owned(),
            lead_span: Some((1.0, f64::INFINITY)),
        }
    })
}

/// The gaps a frame paints: its own, and [`stale_book_gap`] past NOW while
/// its current book is no longer current.
pub fn drawn_gaps(
    projection: &crate::projection::HeatmapProjection,
    current_book_live: bool,
) -> impl Iterator<Item = std::borrow::Cow<'_, crate::projection::GapPrimitive>> {
    let stale = stale_book_gap(projection, current_book_live);
    projection
        .gaps
        .iter()
        .map(std::borrow::Cow::Borrowed)
        .chain(stale.map(std::borrow::Cow::Owned))
}

/// `share` of `window_ms`, rounded up; zero for a share that is not a
/// positive finite number.
#[must_use]
pub(super) fn share_of_window_ms(share: f64, window_ms: i64) -> i64 {
    if share.is_finite() && share > 0.0 {
        (share * window_ms.max(0) as f64).ceil() as i64
    } else {
        0
    }
}

/// Clamp a normalized projection coordinate, treating non-finite input as zero.
#[must_use]
pub fn finite_unit_f64(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}
