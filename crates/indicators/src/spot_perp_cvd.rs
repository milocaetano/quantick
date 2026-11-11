//! The Spot vs Perp CVD pane's model: one CVD line per venue leg, and the
//! spans where the two disagree in sign.
//!
//! Each line is the chart's own [`Cvd`] pane indicator run over that leg's
//! bars through an [`IndicatorHost`], so the pane and a CVD pane on the same
//! bars cannot drift. Each point sits at its bar's close time; the legs need
//! not share a bar type or a clock grid.
//!
//! A span opens at the first point, on either leg, from which both lines are
//! non-zero with opposite signs (a line within [`CVD_ZERO_TOLERANCE`] of
//! zero counts as zero: the running sum is f64, and a flat leg must not
//! flicker a span open on rounding residue), and closes at the first point from which
//! they are not. A span still open when the bars end is marked ongoing and
//! ends at the last point.

use quantick_engine::Bar;

use crate::host::IndicatorHost;
use crate::native::Cvd;
use crate::output::PlotId;

/// How close to zero a CVD line may sit and still count as zero.
pub const CVD_ZERO_TOLERANCE: f64 = 1e-9;

/// One CVD value at its bar's close.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CvdPoint {
    /// The bar's close time, epoch milliseconds.
    pub time_ms: i64,
    /// Cumulative buy minus sell quantity up to and including the bar.
    pub value: f64,
}

/// A stretch where one leg buys while the other sells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OppositeSpan {
    pub start_ms: i64,
    pub end_ms: i64,
    /// Still opposite at the last point.
    pub ongoing: bool,
}

/// The pane's model.
#[derive(Debug, Clone, PartialEq)]
pub struct SpotPerpCvd {
    /// One point per perp bar, in bar order.
    pub perp: Vec<CvdPoint>,
    /// One point per spot bar, in bar order.
    pub spot: Vec<CvdPoint>,
    /// Oldest first.
    pub opposite_spans: Vec<OppositeSpan>,
}

/// Both CVD lines and their opposite-sign spans, from each leg's bars in
/// time order (a forming bar may come last). The caller picks the anchor —
/// a session's bars give a session CVD.
#[must_use]
pub fn spot_perp_cvd(perp_bars: &[Bar], spot_bars: &[Bar]) -> SpotPerpCvd {
    let perp = cvd_line(perp_bars);
    let spot = cvd_line(spot_bars);
    let opposite_spans = opposite_spans(&perp, &spot);
    SpotPerpCvd {
        perp,
        spot,
        opposite_spans,
    }
}

fn cvd_line(bars: &[Bar]) -> Vec<CvdPoint> {
    let mut host = IndicatorHost::new();
    let id = host.add(Box::new(Cvd::new()));
    host.rebuild(bars, None);
    let Some(plots) = host.plots(id) else {
        return Vec::new();
    };
    bars.iter()
        .zip(plots.column(PlotId::new(0)))
        .map(|(bar, &value)| CvdPoint {
            time_ms: bar.close_time,
            value,
        })
        .collect()
}

/// Walk both lines in time order, comparing the latest value of each after
/// every distinct time.
fn opposite_spans(perp: &[CvdPoint], spot: &[CvdPoint]) -> Vec<OppositeSpan> {
    let mut spans = Vec::new();
    let (mut p, mut s) = (0, 0);
    let (mut perp_now, mut spot_now) = (None, None);
    let mut open: Option<i64> = None;
    let mut last_ms = None;
    while p < perp.len() || s < spot.len() {
        let time_ms = match (perp.get(p), spot.get(s)) {
            (Some(a), Some(b)) => a.time_ms.min(b.time_ms),
            (Some(a), None) => a.time_ms,
            (None, Some(b)) => b.time_ms,
            (None, None) => break,
        };
        while let Some(point) = perp.get(p).filter(|point| point.time_ms == time_ms) {
            perp_now = Some(point.value);
            p += 1;
        }
        while let Some(point) = spot.get(s).filter(|point| point.time_ms == time_ms) {
            spot_now = Some(point.value);
            s += 1;
        }
        let opposite = match (perp_now, spot_now) {
            (Some(a), Some(b)) => {
                a.abs() > CVD_ZERO_TOLERANCE
                    && b.abs() > CVD_ZERO_TOLERANCE
                    && (a < 0.0) != (b < 0.0)
            }
            _ => false,
        };
        match (open, opposite) {
            (None, true) => open = Some(time_ms),
            (Some(start_ms), false) => {
                spans.push(OppositeSpan {
                    start_ms,
                    end_ms: time_ms,
                    ongoing: false,
                });
                open = None;
            }
            _ => {}
        }
        last_ms = Some(time_ms);
    }
    if let (Some(start_ms), Some(end_ms)) = (open, last_ms) {
        spans.push(OppositeSpan {
            start_ms,
            end_ms,
            ongoing: true,
        });
    }
    spans
}
