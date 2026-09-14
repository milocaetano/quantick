//! The live-lane family: how much market time the tape shows
//! ([`LaneWindow`]), how the tape is drawn and clustered ([`LiveLaneStyle`]),
//! what it looks like on disk, and the labels the lane's menu and axis print.
//!
//! The parent module owns the shared validation (`finite_clamp`) and the
//! [`HeatmapConfig`](super::HeatmapConfig) that carries one of these; the
//! bubble radii the lane scales come from the sibling bubble family.

use serde::{Deserialize, Serialize};

use super::bubbles::{
    BubbleStyle, MAX_BUBBLE_MAX_RADIUS, MAX_BUBBLE_MIN_RADIUS, MIN_BUBBLE_MAX_RADIUS,
};
use super::{MAX_BUBBLE_CLUSTER_MS, finite_clamp};
use crate::history::TapeAge;

/// Default share of the chart width taken by the live lane. Enough room for
/// the tape to be read as a tape, with two thirds of the chart left for the
/// history it is read against.
pub const DEFAULT_LIVE_LANE_SHARE: f32 = 0.35;
/// Narrowest live lane accepted, as a share of the chart.
pub const MIN_LIVE_LANE_SHARE: f32 = 0.05;
/// Widest live lane accepted. The tape may take half the chart and no more:
/// past that there is no history left to read the tape against.
pub const MAX_LIVE_LANE_SHARE: f32 = 0.5;
/// Narrowest lane the layout will draw, in pixels. A floor rather than a
/// setting: below this the band stops being a tape whatever share was asked
/// for. Capped by the chart itself, so a tiny window is still split by share.
pub const MIN_LIVE_LANE_WIDTH_PX: f32 = 24.0;
/// Default zoom of the lane's time window: one typical bar's worth of market
/// time, the same window the lane showed before the knob existed.
pub const DEFAULT_LIVE_LANE_ZOOM: f32 = 1.0;
/// Most zoomed-out lane accepted — four times the typical bar duration in the
/// same band, so prints crowd together and cluster into fewer, bigger marks.
pub const MIN_LIVE_LANE_ZOOM: f32 = 0.25;
/// Most zoomed-in lane accepted — an eighth of it, so prints run across the
/// band fast and far apart.
pub const MAX_LIVE_LANE_ZOOM: f32 = 8.0;
/// Least market time a zoomed-in lane will show. Below an instant there is no
/// tape left, whatever the zoom asked for.
pub const MIN_LIVE_LANE_WINDOW_MS: i64 = 200;
/// Most market time a zoomed-out lane will show. A tape is the recent past;
/// past a quarter of an hour the chart's own history says it better.
pub const MAX_LIVE_LANE_WINDOW_MS: i64 = 900_000;
/// The fixed tape windows the lane's menu offers, in exchange milliseconds.
///
/// Round durations a trader already thinks in, not a sample of the accepted
/// range: the point of a preset is that it is chosen without reading a number.
/// Anything else is reachable through the custom entry, which accepts the
/// whole [`MIN_LIVE_LANE_WINDOW_MS`]..=[`MAX_LIVE_LANE_WINDOW_MS`] band.
pub const LANE_WINDOW_PRESETS_MS: [i64; 5] = [15_000, 30_000, 60_000, 120_000, 300_000];
/// Default multiplier applied to the bubble radii inside the live lane.
pub const DEFAULT_LIVE_LANE_RADIUS_SCALE: f32 = 1.0;
/// Bounds accepted for the live lane's radius multiplier.
pub const MIN_LIVE_LANE_RADIUS_SCALE: f32 = 0.25;
/// See [`MIN_LIVE_LANE_RADIUS_SCALE`].
pub const MAX_LIVE_LANE_RADIUS_SCALE: f32 = 4.0;

/// How much market time the tape shows.
///
/// One number, one owner. The lane is a fixed band of screen, so its window is
/// what decides how fast a print crosses it — and there is exactly one way to
/// ask for that, phrased in one of two languages:
///
/// - [`Auto`](Self::Auto) follows the bars: the tape holds roughly one bar's
///   worth of flow whatever the instrument or the bar type, and the zoom
///   scales that reference. This is what the lane has always done, and it
///   stays the default.
/// - [`Fixed`](Self::Fixed) pins an amount of market time and ignores what the
///   bars are doing. On an activity-sampled series a burst closes bars faster
///   than a human reads prints, and a trader who wants the last two minutes in
///   front of them is asking for two minutes, not for a multiplier.
///
/// The gesture over the tape edits whichever language is in force — the zoom
/// in `Auto`, the milliseconds in `Fixed` — so dragging never silently
/// switches modes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LaneWindow {
    /// Follows the recent bars' typical duration, scaled by `zoom`.
    Auto {
        /// `1.0` fits about one bar's worth of market time in the band. Above
        /// one shows less time (prints run across faster and further apart),
        /// below one shows more (they crowd together and cluster).
        zoom: f32,
    },
    /// A fixed amount of market time, whatever the bars do.
    Fixed {
        /// Exchange milliseconds shown in the band.
        ms: i64,
    },
}

impl Default for LaneWindow {
    fn default() -> Self {
        Self::Auto {
            zoom: DEFAULT_LIVE_LANE_ZOOM,
        }
    }
}

impl LaneWindow {
    /// Clamp into a range the renderer can use.
    pub fn sanitize(&mut self) {
        match self {
            Self::Auto { zoom } => {
                *zoom = finite_clamp(
                    *zoom,
                    MIN_LIVE_LANE_ZOOM,
                    MAX_LIVE_LANE_ZOOM,
                    DEFAULT_LIVE_LANE_ZOOM,
                );
            }
            Self::Fixed { ms } => {
                *ms = (*ms).clamp(MIN_LIVE_LANE_WINDOW_MS, MAX_LIVE_LANE_WINDOW_MS);
            }
        }
    }

    /// Market time the lane shows, given the automatic reference — the recent
    /// bars' typical duration ([`reserved_span_ms`](crate::reserved_span_ms)).
    ///
    /// Both modes end in the same clamp: a session-long reference cannot turn
    /// the tape into a second chart, and a burst cannot make it an instant
    /// wide, whichever language asked for the window.
    #[must_use]
    pub fn resolve_ms(self, reference_ms: i64) -> i64 {
        let window = match self {
            Self::Auto { zoom } => {
                let zoom = if zoom.is_finite() {
                    zoom.clamp(MIN_LIVE_LANE_ZOOM, MAX_LIVE_LANE_ZOOM)
                } else {
                    DEFAULT_LIVE_LANE_ZOOM
                };
                (reference_ms.max(1) as f64 / f64::from(zoom)).round() as i64
            }
            Self::Fixed { ms } => ms,
        };
        window.clamp(MIN_LIVE_LANE_WINDOW_MS, MAX_LIVE_LANE_WINDOW_MS)
    }

    /// How the resolved window compares to the automatic reference.
    ///
    /// The factor the clustering window is divided by, so a cluster covers a
    /// constant *distance on screen* rather than a constant amount of time. In
    /// `Auto` it works out to the zoom itself; in `Fixed` it is whatever the
    /// pinned window is worth against the same reference — which is what keeps
    /// a two-minute tape from drawing as a smear.
    #[must_use]
    fn cluster_scale(self, reference_ms: i64) -> f64 {
        reference_ms.max(1) as f64 / self.resolve_ms(reference_ms).max(1) as f64
    }

    /// Apply a multiplicative zoom gesture, in whichever language is in force.
    ///
    /// `> 1` shows less market time in the same band, `< 1` more. A gesture
    /// never changes the mode: pinning two minutes and then scrolling gives a
    /// different fixed window, not a return to following the bars.
    pub fn zoom_by(&mut self, factor: f32) {
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        match self {
            Self::Auto { zoom } => *zoom *= factor,
            Self::Fixed { ms } => {
                *ms = ((*ms as f64 / f64::from(factor)).round() as i64)
                    .clamp(MIN_LIVE_LANE_WINDOW_MS, MAX_LIVE_LANE_WINDOW_MS);
            }
        }
    }
}

/// A duration as a trader reads it: `8 s`, `1 min`, `1 min 30 s`, `250 ms`.
///
/// Sub-second windows keep their milliseconds because that is the only place
/// the number carries information; past a minute the seconds are dropped when
/// they are zero, so a preset reads as the round number it was chosen as.
#[must_use]
pub fn format_window_ms(ms: i64) -> String {
    let ms = ms.max(0);
    if ms < 1_000 {
        return format!("{ms} ms");
    }
    let seconds = ms / 1_000;
    let (minutes, seconds) = (seconds / 60, seconds % 60);
    match (minutes, seconds) {
        (0, seconds) => format!("{seconds} s"),
        (minutes, 0) => format!("{minutes} min"),
        (minutes, seconds) => format!("{minutes} min {seconds} s"),
    }
}

/// Share of the lane's own window a print must fall behind before the axis
/// says so.
///
/// Below it the newest bubble is a couple of pixels off the edge — saying
/// anything would be noise on a tape that is, to the eye, current. The
/// threshold is a share rather than a duration because the lane's window is
/// the trader's own setting: what reads as "behind" on a four-second tape is
/// invisible on a two-minute one.
///
/// A sixth of the tape, because the caption has to earn its appearance. A
/// tighter threshold would light it up every time an ordinary market went
/// quiet for a moment, and a warning that cries wolf during lunch is one the
/// trader stops reading before the session that matters.
const LANE_LAG_LABEL_SHARE: f64 = 1.0 / 6.0;

/// Shortest silence the axis will call a silence, whatever the window.
///
/// The share alone collapses at the small end: the tape zooms to
/// [`MIN_LIVE_LANE_WINDOW_MS`] of 200 ms, where a sixth is 33 ms — under the
/// interval between two ordinary prints (a WIN recording measures 20 ms at the
/// median and 285 ms at the worst). The caption would then be lit permanently
/// on a tape that is perfectly current, which is the cry-wolf failure the
/// share was chosen to avoid, reached from the other direction. Below a second
/// there is no delay a person reads as one.
const MIN_LANE_LAG_MS: i64 = 1_000;

/// What the axis under the tape says about how old its newest mark is, or
/// `None` while the tape is current enough that saying anything is noise.
///
/// The caller decides *whether to ask*: this answers about the aggression
/// marks, so a pane drawing only the depth map has no missing bubbles to
/// explain and must not be handed an age at all.
///
/// The lane's right edge is the newer of the book clock and the print clock,
/// and only prints draw bubbles. So a book running ahead of the tape — a quiet
/// stretch, or prints held up on their way here — puts every bubble left of
/// the edge, and past the whole window puts none of them on the tape at all.
/// An empty tape reads as "nothing is trading", which is the one thing it
/// never means: the prints are on the chart, in their bar's slot. The axis is
/// where that gets said, because it is already the readout for what the lane
/// is showing.
#[must_use]
pub fn lane_lag_label(window_ms: i64, tape_age: Option<TapeAge>) -> Option<String> {
    // Nothing has printed since the tape started watching. There is no mark to
    // be late, so the wording never invites the reader to look for one, and no
    // window threshold applies — an empty tape with a running book is worth
    // saying at any zoom.
    let age = match tape_age? {
        TapeAge::NothingYet(watched) => {
            return Some(format!("no print for {}", format_window_ms(watched)));
        }
        TapeAge::Behind(age) => age,
    };
    let window = window_ms.max(1);
    #[allow(clippy::cast_precision_loss)]
    let share_floor = (window as f64 * LANE_LAG_LABEL_SHARE) as i64;
    if age <= share_floor.max(MIN_LANE_LAG_MS) {
        return None;
    }
    // Past the window there is no bubble left on the tape to be late: the
    // wording changes with it, because "last print 40 s ago" beside an empty
    // tape still invites the reader to look for the mark it describes.
    //
    // Strictly past. The lane spans `[now - window, now]`, so at exactly the
    // window the newest mark is still drawn, on the tape's leftmost pixel —
    // telling the reader there is nothing to look for while it is on screen
    // is the same lie in the other direction.
    if age > window {
        return Some(format!("no print for {}", format_window_ms(age)));
    }
    Some(format!("last print {} back", format_window_ms(age)))
}

/// How a tape window reads in a menu.
///
/// `reference_ms` is the automatic reference, when the caller knows it. The
/// automatic entry states the duration it currently works out to whenever it
/// is supplied: "auto" alone names a policy, and a trader choosing between it
/// and `30 s` is comparing durations, so the one number that would let them
/// compare has no business being the one number hidden.
#[must_use]
pub fn lane_window_label(window: LaneWindow, reference_ms: Option<i64>) -> String {
    match window {
        LaneWindow::Auto { .. } => reference_ms.map_or_else(
            || "auto".to_owned(),
            |reference| {
                format!(
                    "auto (≈ {})",
                    format_window_ms(window.resolve_ms(reference))
                )
            },
        ),
        LaneWindow::Fixed { ms } => format_window_ms(ms),
    }
}

/// Whether a menu entry names the window already in force.
///
/// Two `Auto`s match whatever their zoom, so selecting the automatic entry
/// while already following the bars cannot quietly reset a zoom the trader
/// set; two `Fixed`s have to name the same duration.
#[must_use]
pub fn same_lane_window(current: LaneWindow, option: LaneWindow) -> bool {
    match (current, option) {
        (LaneWindow::Auto { .. }, LaneWindow::Auto { .. }) => true,
        (LaneWindow::Fixed { ms: current }, LaneWindow::Fixed { ms: option }) => current == option,
        _ => false,
    }
}

/// How the rolling tape past the last bar is drawn.
///
/// The live lane is a pane of its own, pinned to the right edge of the chart:
/// history is compressed into equal-width bar slots that pan and zoom, the lane
/// is a fixed band of screen showing a fixed window of market time that always
/// ends at now. Being its own pane is what earns it settings of its own — how
/// wide it is, how much time fits in it, how big its bubbles are, and which of
/// the two flow layers it draws, none of which the candles beside it get a say
/// in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "LiveLaneStyleRepr", into = "LiveLaneStyleRepr")]
pub struct LiveLaneStyle {
    /// Width of the lane as a share of the chart.
    ///
    /// Measured against the chart rather than the candle so that the tape is a
    /// *place* on screen: zooming the time axis changes how many bars fit
    /// beside it, never how much room it gets. Fixed, and so is the market time
    /// it shows, which together give the tape a constant pixels-per-ms rate —
    /// a print enters at the right edge and slides left at the same speed. A
    /// bar closing changes neither, so nothing on the tape ever jumps.
    pub width_share: f32,
    /// How much market time fits in the band: following the bars, or pinned.
    ///
    /// The clustering window follows whatever this resolves to
    /// ([`effective_cluster_ms`](Self::effective_cluster_ms)), which is what
    /// turns a crowded tape into fewer, bigger marks instead of a smear.
    pub window: LaneWindow,
    /// Clustering window applied to prints inside the lane, before the window
    /// scales it.
    ///
    /// `None` inherits [`HeatmapConfig::bubble_cluster_ms`](super::HeatmapConfig::bubble_cluster_ms). A shorter window
    /// than history's buys detail where there is room for it; a longer one
    /// gathers a fast tape into readable marks.
    pub cluster_ms: Option<i64>,
    /// Multiplier applied to both bubble radii inside the lane.
    pub radius_scale: f32,
    /// Whether the lane's boundary and the live-edge line are drawn.
    pub show_marks: bool,
    /// Whether the tape is on the canvas at all.
    ///
    /// Off, the lane reserves no width ([`Self::resolved_width_px`] answers
    /// zero), so the candles take the whole canvas and nothing downstream —
    /// divider, projection, marks — has a band to work in. The two layer
    /// switches below keep whatever they were set to while it is off: turning
    /// the tape back on must return the tape that was switched off, not a
    /// default one.
    pub enabled: bool,
    /// Whether the depth map is drawn *on the tape*.
    ///
    /// Each pane answers for its own canvas, and the tape answers with a value
    /// of its own rather than by following the candles: the compressed history
    /// is read one way and the rolling tape another, so a trader who clears the
    /// candles has not thereby asked for the book to vanish where the book is
    /// actually being read. On by default — the tape's whole job is to show the
    /// book and the prints landing into it.
    pub show_depth: bool,
    /// Whether aggression bubbles are drawn *on the tape*. Same rule as
    /// [`show_depth`](Self::show_depth), and on by default for the same reason.
    pub show_aggressions: bool,
}

impl Default for LiveLaneStyle {
    fn default() -> Self {
        Self {
            width_share: DEFAULT_LIVE_LANE_SHARE,
            window: LaneWindow::default(),
            cluster_ms: None,
            radius_scale: DEFAULT_LIVE_LANE_RADIUS_SCALE,
            show_marks: true,
            enabled: true,
            show_depth: true,
            show_aggressions: true,
        }
    }
}

/// What a [`LiveLaneStyle`] looks like on disk.
///
/// The window is one field in memory and two on disk, and the split is
/// deliberate: `time_zoom` is the name every preset written before this mode
/// existed already uses, so reading one back has to find it where it was left.
/// `window_ms` is absent from all of them, which is exactly the sentence "this
/// lane follows the bars" — the mode a file from before the choice existed is
/// in.
///
/// Nothing outside serialization sees this type. [`LaneWindow`] stays the only
/// owner of the number in memory, so no two fields can disagree about how much
/// market time the tape is showing.
///
/// The two layer switches are `Option<bool>` here and plain `bool` in memory,
/// and that is the whole of the compatibility story. A file written while the
/// tape still inherited the candles' switches left the key out whenever nobody
/// had chosen, which read as "follow the chart"; there is no chart to follow
/// any more, so an absent answer becomes the tape's own default — on. A file
/// that says `false` said it on purpose and is still obeyed.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct LiveLaneStyleRepr {
    width_share: f32,
    time_zoom: f32,
    window_ms: Option<i64>,
    cluster_ms: Option<i64>,
    radius_scale: f32,
    show_marks: bool,
    enabled: bool,
    show_depth: Option<bool>,
    show_aggressions: Option<bool>,
}

impl Default for LiveLaneStyleRepr {
    fn default() -> Self {
        Self::from(LiveLaneStyle::default())
    }
}

impl From<LiveLaneStyleRepr> for LiveLaneStyle {
    fn from(repr: LiveLaneStyleRepr) -> Self {
        Self {
            width_share: repr.width_share,
            // A pinned window is the explicit choice; without one the file is
            // asking to follow the bars, at whatever zoom it recorded.
            window: repr.window_ms.map_or(
                LaneWindow::Auto {
                    zoom: repr.time_zoom,
                },
                |ms| LaneWindow::Fixed { ms },
            ),
            cluster_ms: repr.cluster_ms,
            radius_scale: repr.radius_scale,
            show_marks: repr.show_marks,
            enabled: repr.enabled,
            // Absent, or the `null` an inheriting file wrote: the tape's own
            // default. See `LiveLaneStyleRepr`.
            show_depth: repr.show_depth.unwrap_or(true),
            show_aggressions: repr.show_aggressions.unwrap_or(true),
        }
    }
}

impl From<LiveLaneStyle> for LiveLaneStyleRepr {
    fn from(style: LiveLaneStyle) -> Self {
        // Pinning a window parks the zoom at its default rather than keeping
        // the last one: "automatic" means the bars decide, and a remembered
        // multiplier waiting to reappear is not that.
        let (time_zoom, window_ms) = match style.window {
            LaneWindow::Auto { zoom } => (zoom, None),
            LaneWindow::Fixed { ms } => (DEFAULT_LIVE_LANE_ZOOM, Some(ms)),
        };
        Self {
            width_share: style.width_share,
            time_zoom,
            window_ms,
            cluster_ms: style.cluster_ms,
            radius_scale: style.radius_scale,
            show_marks: style.show_marks,
            enabled: style.enabled,
            show_depth: Some(style.show_depth),
            show_aggressions: Some(style.show_aggressions),
        }
    }
}

impl LiveLaneStyle {
    /// Clamp every value into a range the renderer can use.
    pub fn sanitize(&mut self) {
        self.width_share = finite_clamp(
            self.width_share,
            MIN_LIVE_LANE_SHARE,
            MAX_LIVE_LANE_SHARE,
            DEFAULT_LIVE_LANE_SHARE,
        );
        self.window.sanitize();
        self.radius_scale = finite_clamp(
            self.radius_scale,
            MIN_LIVE_LANE_RADIUS_SCALE,
            MAX_LIVE_LANE_RADIUS_SCALE,
            DEFAULT_LIVE_LANE_RADIUS_SCALE,
        );
        self.cluster_ms = self
            .cluster_ms
            .map(|window| window.clamp(0, MAX_BUBBLE_CLUSTER_MS));
    }

    /// Lane width in pixels for a chart this wide.
    ///
    /// The lane is a pane, not a number of candle slots: it takes
    /// `chart_width × width_share` of screen whatever the candles are doing, so
    /// zooming the time axis changes how many bars fit beside the tape and
    /// never how much room the tape gets.
    #[must_use]
    pub fn resolved_width_px(&self, chart_width: f32) -> f32 {
        // The one gate the whole switch hangs off. A tape that is off reserves
        // no band, and everything downstream already reads a zero width as
        // "there is no lane": no divider, no lane rungs, no drawing boundary,
        // no projection asked for on the tape's account.
        if !self.enabled {
            return 0.0;
        }
        if !chart_width.is_finite() || chart_width <= 0.0 {
            return 0.0;
        }
        let share = if self.width_share.is_finite() {
            self.width_share
                .clamp(MIN_LIVE_LANE_SHARE, MAX_LIVE_LANE_SHARE)
        } else {
            DEFAULT_LIVE_LANE_SHARE
        };
        // The floor yields to the cap on a narrow window: half a small chart
        // beats a fixed band that would eat all of it.
        let floor = MIN_LIVE_LANE_WIDTH_PX.min(chart_width * MAX_LIVE_LANE_SHARE);
        (chart_width * share).max(floor)
    }

    /// Market time the lane shows, given the automatic reference window (the
    /// recent bars' typical duration).
    #[must_use]
    pub fn window_ms(&self, reference_ms: i64) -> i64 {
        self.window.resolve_ms(reference_ms)
    }

    /// Clustering window for prints inside the lane, given history's own and
    /// the automatic reference the lane's window is measured against.
    ///
    /// Scaled by the same factor as the window itself, so a cluster covers a
    /// constant *distance on screen* rather than a constant amount of time:
    /// a wider window gathers the crowd it creates into fewer, bigger bubbles
    /// instead of piling prints on top of each other. The factor comes from
    /// the resolved window rather than from the zoom, which is what lets a
    /// pinned two-minute tape cluster like the zoomed-out tape it is.
    #[must_use]
    pub fn effective_cluster_ms(&self, history_cluster_ms: i64, reference_ms: i64) -> i64 {
        let base = self.cluster_ms.unwrap_or(history_cluster_ms).max(0);
        if base == 0 {
            // One bubble per print, at every window: raw is a choice, not a
            // size.
            return 0;
        }
        let scaled = (base as f64 / self.window.cluster_scale(reference_ms)).round() as i64;
        scaled.clamp(1, MAX_BUBBLE_CLUSTER_MS)
    }

    /// Bubble radius range inside the lane, given the shared bubble style.
    #[must_use]
    pub fn scaled_radii(&self, bubbles: &BubbleStyle) -> (f32, f32) {
        let scale = if self.radius_scale.is_finite() {
            self.radius_scale
                .clamp(MIN_LIVE_LANE_RADIUS_SCALE, MAX_LIVE_LANE_RADIUS_SCALE)
        } else {
            DEFAULT_LIVE_LANE_RADIUS_SCALE
        };
        let min = (bubbles.min_radius * scale).clamp(0.0, MAX_BUBBLE_MIN_RADIUS);
        let max = (bubbles.max_radius * scale).clamp(MIN_BUBBLE_MAX_RADIUS, MAX_BUBBLE_MAX_RADIUS);
        (min.min(max), max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DEFAULT_BUBBLE_CLUSTER_MS;

    /// The lane is a pane of the chart, so its width is a share of the chart
    /// and nothing else — the candle zoom has no say in it at all.
    #[test]
    fn the_lane_takes_a_share_of_the_chart_and_never_more_than_half() {
        let lane = LiveLaneStyle::default();
        assert!((lane.resolved_width_px(1_000.0) - 350.0).abs() < 0.01);
        assert!((lane.resolved_width_px(600.0) - 210.0).abs() < 0.01);

        // A wider share takes more of the same chart; half is the ceiling.
        let wide = LiveLaneStyle {
            width_share: 0.9,
            ..LiveLaneStyle::default()
        };
        let mut capped = wide.clone();
        capped.sanitize();
        assert_eq!(capped.width_share, MAX_LIVE_LANE_SHARE);
        assert!((capped.resolved_width_px(1_000.0) - 500.0).abs() < 0.01);
        // Even unsanitized input cannot reach past half the chart.
        assert!(wide.resolved_width_px(1_000.0) <= 500.01);

        // Degenerate charts get no lane rather than a width the layout would
        // have to invent a meaning for.
        assert_eq!(lane.resolved_width_px(f32::NAN), 0.0);
        assert_eq!(lane.resolved_width_px(0.0), 0.0);
        // A tiny share stays a band rather than a hairline...
        let sliver = LiveLaneStyle {
            width_share: MIN_LIVE_LANE_SHARE,
            ..LiveLaneStyle::default()
        };
        assert!(sliver.resolved_width_px(200.0) >= MIN_LIVE_LANE_WIDTH_PX);
        // ...unless the chart itself is smaller than the floor, where the cap
        // wins and the tape still leaves history half the window.
        assert!((sliver.resolved_width_px(20.0) - 10.0).abs() < 0.01);
    }

    /// The lane's own zoom: same band, a different amount of market time in
    /// it. Zooming out has to aggregate as well as compress, or the crowd it
    /// creates is unreadable.
    #[test]
    fn the_lane_zoom_scales_the_window_and_the_clustering_with_it() {
        let reference = 8_000; // a typical bar of eight seconds
        let default = LiveLaneStyle {
            cluster_ms: Some(100),
            ..LiveLaneStyle::default()
        };
        assert_eq!(default.window_ms(reference), 8_000);
        assert_eq!(
            default.effective_cluster_ms(DEFAULT_BUBBLE_CLUSTER_MS, reference),
            100
        );

        // Zoomed out: four times the market time in the same band, and a
        // cluster four times as long — so a cluster keeps its width on screen.
        let out = LiveLaneStyle {
            window: LaneWindow::Auto { zoom: 0.25 },
            ..default.clone()
        };
        assert_eq!(out.window_ms(reference), 32_000);
        assert_eq!(
            out.effective_cluster_ms(DEFAULT_BUBBLE_CLUSTER_MS, reference),
            400
        );

        // Zoomed in: a quarter of the time, a quarter of the window.
        let into = LiveLaneStyle {
            window: LaneWindow::Auto { zoom: 4.0 },
            ..default.clone()
        };
        assert_eq!(into.window_ms(reference), 2_000);
        assert_eq!(
            into.effective_cluster_ms(DEFAULT_BUBBLE_CLUSTER_MS, reference),
            25
        );

        // Raw is a choice, not a size: it survives every zoom.
        let raw = LiveLaneStyle {
            cluster_ms: Some(0),
            window: LaneWindow::Auto {
                zoom: MIN_LIVE_LANE_ZOOM,
            },
            ..LiveLaneStyle::default()
        };
        assert_eq!(
            raw.effective_cluster_ms(DEFAULT_BUBBLE_CLUSTER_MS, reference),
            0
        );

        // Bounds hold whatever the bars did: a session-long reference cannot
        // turn the tape into a second chart, and a burst cannot make it an
        // instant wide.
        let out = LiveLaneStyle {
            window: LaneWindow::Auto {
                zoom: MIN_LIVE_LANE_ZOOM,
            },
            ..LiveLaneStyle::default()
        };
        assert_eq!(out.window_ms(i64::MAX), MAX_LIVE_LANE_WINDOW_MS);
        let into = LiveLaneStyle {
            window: LaneWindow::Auto {
                zoom: MAX_LIVE_LANE_ZOOM,
            },
            ..LiveLaneStyle::default()
        };
        assert_eq!(into.window_ms(1), MIN_LIVE_LANE_WINDOW_MS);
        assert_eq!(into.effective_cluster_ms(1, 8_000), 1);
    }

    /// A pinned window is a window like any other: it resolves whatever the
    /// bars do, and the clustering scales with it exactly as the zoom's does.
    /// Without that second half a two-minute tape draws two minutes of prints
    /// at a tape's density, which is a smear.
    #[test]
    fn a_pinned_window_ignores_the_bars_and_clusters_like_the_width_it_is() {
        let reference = 8_000; // a typical bar of eight seconds
        let pinned = LiveLaneStyle {
            window: LaneWindow::Fixed { ms: 120_000 },
            cluster_ms: Some(100),
            ..LiveLaneStyle::default()
        };
        // Two minutes is two minutes, whatever the bars are doing...
        assert_eq!(pinned.window_ms(reference), 120_000);
        assert_eq!(pinned.window_ms(1), 120_000);
        assert_eq!(pinned.window_ms(600_000), 120_000);
        // ...and fifteen times the automatic window gathers fifteen times the
        // market time into each mark.
        assert_eq!(
            pinned.effective_cluster_ms(DEFAULT_BUBBLE_CLUSTER_MS, reference),
            1_500
        );
        // Past the clustering ceiling the scaling stops rather than running
        // away: a mark may gather a crowd, never a whole tape.
        let widest = LiveLaneStyle {
            window: LaneWindow::Fixed {
                ms: MAX_LIVE_LANE_WINDOW_MS,
            },
            ..pinned.clone()
        };
        assert_eq!(
            widest.effective_cluster_ms(DEFAULT_BUBBLE_CLUSTER_MS, reference),
            MAX_BUBBLE_CLUSTER_MS
        );
        let modest = LiveLaneStyle {
            window: LaneWindow::Fixed { ms: 16_000 },
            ..pinned.clone()
        };
        assert_eq!(
            modest.effective_cluster_ms(DEFAULT_BUBBLE_CLUSTER_MS, reference),
            200,
            "twice the automatic window, twice the cluster"
        );
        // A narrower pinned window buys detail, the same way zooming in does.
        let narrow = LiveLaneStyle {
            window: LaneWindow::Fixed { ms: 4_000 },
            ..pinned.clone()
        };
        assert_eq!(
            narrow.effective_cluster_ms(DEFAULT_BUBBLE_CLUSTER_MS, reference),
            50
        );
        // A wider tape always gathers more than a narrower one — the property
        // the numbers above are examples of.
        assert!(
            modest.effective_cluster_ms(DEFAULT_BUBBLE_CLUSTER_MS, reference)
                > narrow.effective_cluster_ms(DEFAULT_BUBBLE_CLUSTER_MS, reference)
        );
        // Raw is a choice, not a size: it survives a pinned window too.
        let raw = LiveLaneStyle {
            cluster_ms: Some(0),
            ..pinned
        };
        assert_eq!(
            raw.effective_cluster_ms(DEFAULT_BUBBLE_CLUSTER_MS, reference),
            0
        );

        // Absurd durations are clamped to what the tape can draw, both ends.
        let mut wild = LiveLaneStyle {
            window: LaneWindow::Fixed { ms: i64::MAX },
            ..LiveLaneStyle::default()
        };
        wild.sanitize();
        assert_eq!(
            wild.window,
            LaneWindow::Fixed {
                ms: MAX_LIVE_LANE_WINDOW_MS
            }
        );
        let mut tiny = LiveLaneStyle {
            window: LaneWindow::Fixed { ms: 0 },
            ..LiveLaneStyle::default()
        };
        tiny.sanitize();
        assert_eq!(
            tiny.window,
            LaneWindow::Fixed {
                ms: MIN_LIVE_LANE_WINDOW_MS
            }
        );
    }

    /// The gesture over the tape edits whichever language the window is in,
    /// and never swaps the language underneath the trader.
    #[test]
    fn a_zoom_gesture_never_changes_which_mode_the_tape_is_in() {
        let mut following = LaneWindow::default();
        following.zoom_by(2.0);
        assert_eq!(following, LaneWindow::Auto { zoom: 2.0 });

        let mut pinned = LaneWindow::Fixed { ms: 60_000 };
        pinned.zoom_by(2.0);
        assert_eq!(
            pinned,
            LaneWindow::Fixed { ms: 30_000 },
            "zooming in halves the pinned window; it does not return to automatic"
        );
        pinned.zoom_by(0.5);
        assert_eq!(pinned, LaneWindow::Fixed { ms: 60_000 });

        // A pinned window cannot be dragged out of the drawable range.
        let mut floor = LaneWindow::Fixed {
            ms: MIN_LIVE_LANE_WINDOW_MS,
        };
        floor.zoom_by(1_000.0);
        assert_eq!(
            floor,
            LaneWindow::Fixed {
                ms: MIN_LIVE_LANE_WINDOW_MS
            }
        );

        // A meaningless gesture is refused rather than applied as garbage.
        let mut untouched = LaneWindow::Fixed { ms: 60_000 };
        untouched.zoom_by(f32::NAN);
        untouched.zoom_by(0.0);
        untouched.zoom_by(-1.0);
        assert_eq!(untouched, LaneWindow::Fixed { ms: 60_000 });
    }

    /// An empty tape has two possible meanings and only one of them is
    /// "nothing is trading". The axis has to separate them.
    ///
    /// The lane's right edge follows the newer of the book clock and the print
    /// clock; only prints draw bubbles. So a book running ahead of the tape
    /// puts every bubble left of the edge, and past the whole window puts none
    /// of them on the tape — while the prints themselves are still on the
    /// chart, in the slot of the bar they happened in. A scalper reading an
    /// empty tape as a still market is reading a delivery gap as a fact about
    /// the market, which is the expensive kind of wrong.
    #[test]
    fn the_axis_says_how_old_the_newest_mark_is() {
        let behind = |ms| Some(TapeAge::Behind(ms));
        // Current: nothing to say. Not "0 s behind" — a tape that is on the
        // edge must not carry a caption about being late.
        assert_eq!(lane_lag_label(30_000, None), None);
        // Under the threshold: an ordinary lull, and the axis stays quiet.
        assert_eq!(lane_lag_label(30_000, behind(3_000)), None);
        // Visibly behind, but the newest mark is still on the tape, so the
        // wording points at the mark the reader can find.
        assert_eq!(
            lane_lag_label(30_000, behind(6_000)),
            Some("last print 6 s back".to_owned())
        );
        // Exactly one window behind, the mark sits on the tape's leftmost
        // pixel — still there, so the wording must still point at it. Only
        // past the window does the tape genuinely hold nothing.
        assert_eq!(
            lane_lag_label(30_000, behind(30_000)),
            Some("last print 30 s back".to_owned())
        );
        assert_eq!(
            lane_lag_label(30_000, behind(30_001)),
            Some("no print for 30 s".to_owned())
        );

        // The floor follows the window, because "behind" is relative to what
        // the tape is showing: four seconds is most of a four-second tape and
        // invisible on a two-minute one.
        assert_eq!(
            lane_lag_label(4_000, behind(4_000)),
            Some("last print 4 s back".to_owned())
        );
        assert_eq!(lane_lag_label(120_000, behind(4_000)), None);
        assert_eq!(lane_lag_label(120_000, behind(17_000)), None);
        assert_eq!(
            lane_lag_label(120_000, behind(30_000)),
            Some("last print 30 s back".to_owned())
        );

        // …but never below a second, whatever the share works out to. The
        // tape zooms to MIN_LIVE_LANE_WINDOW_MS, where a sixth is 33 ms — less
        // than the gap between two ordinary prints, so a share-only floor
        // would light the caption permanently on a perfectly current tape.
        assert_eq!(lane_lag_label(MIN_LIVE_LANE_WINDOW_MS, behind(285)), None);
        assert_eq!(lane_lag_label(MIN_LIVE_LANE_WINDOW_MS, behind(900)), None);
        assert_eq!(
            lane_lag_label(MIN_LIVE_LANE_WINDOW_MS, behind(1_500)),
            Some("no print for 1 s".to_owned()),
            "a second and a half of silence on a 200 ms tape is a real hole"
        );

        // Nothing has printed at all. There is no mark to be late, so no
        // window threshold applies and the wording never sends the reader
        // looking for a bubble. This is the state every restart and every
        // symbol switch during a quiet stretch lands in — the chart opened at
        // lunch, which is the report this whole branch came from.
        assert_eq!(
            lane_lag_label(30_000, Some(TapeAge::NothingYet(4_000))),
            Some("no print for 4 s".to_owned())
        );
        assert_eq!(
            lane_lag_label(MIN_LIVE_LANE_WINDOW_MS, Some(TapeAge::NothingYet(90_000))),
            Some("no print for 1 min 30 s".to_owned()),
            "an empty tape is worth saying at any zoom"
        );

        // A degenerate window must not divide the label by zero or claim a
        // current tape is late.
        assert_eq!(lane_lag_label(0, None), None);
        assert_eq!(
            lane_lag_label(0, behind(5_000)),
            Some("no print for 5 s".to_owned())
        );
    }

    /// A menu that offers "auto" against "30 s" is asking the trader to
    /// compare durations, so the automatic entry has to state the one it
    /// currently amounts to.
    #[test]
    fn the_automatic_entry_states_the_duration_it_currently_works_out_to() {
        let following = LaneWindow::default();
        assert_eq!(lane_window_label(following, Some(8_000)), "auto (≈ 8 s)");
        assert_eq!(
            lane_window_label(following, Some(90_000)),
            "auto (≈ 1 min 30 s)"
        );
        // With no bars to measure, it names the policy rather than inventing
        // a number.
        assert_eq!(lane_window_label(following, None), "auto");
        // A zoomed automatic window reports what it shows, not the reference.
        let zoomed = LaneWindow::Auto { zoom: 4.0 };
        assert_eq!(lane_window_label(zoomed, Some(8_000)), "auto (≈ 2 s)");

        for (ms, expected) in [
            (250, "250 ms"),
            (15_000, "15 s"),
            (60_000, "1 min"),
            (120_000, "2 min"),
            (300_000, "5 min"),
        ] {
            assert_eq!(
                lane_window_label(LaneWindow::Fixed { ms }, Some(8_000)),
                expected
            );
        }
        // Every preset the menu offers reads as the round number it was
        // chosen as.
        for ms in LANE_WINDOW_PRESETS_MS {
            let label = lane_window_label(LaneWindow::Fixed { ms }, Some(8_000));
            assert!(!label.contains("ms"), "{label} is not how a trader says it");
        }

        // Selecting the automatic entry while already following the bars must
        // not reset a zoom the trader set; a preset only matches its own
        // duration.
        assert!(same_lane_window(zoomed, LaneWindow::default()));
        assert!(same_lane_window(
            LaneWindow::Fixed { ms: 60_000 },
            LaneWindow::Fixed { ms: 60_000 }
        ));
        assert!(!same_lane_window(
            LaneWindow::Fixed { ms: 60_000 },
            LaneWindow::Fixed { ms: 30_000 }
        ));
        assert!(!same_lane_window(zoomed, LaneWindow::Fixed { ms: 60_000 }));
    }

    /// Every preset written before the tape had a window mode says "follow the
    /// bars" by saying nothing, and has to keep drawing what it drew.
    #[test]
    fn a_file_from_before_the_mode_existed_opens_exactly_as_it_did() {
        let old: LiveLaneStyle = toml::from_str(
            "width_share = 0.4\ntime_zoom = 2.0\nradius_scale = 1.5\nshow_marks = false\n",
        )
        .unwrap();
        assert_eq!(old.window, LaneWindow::Auto { zoom: 2.0 });
        assert_eq!(
            old.window_ms(8_000),
            4_000,
            "the same window it always drew"
        );
        assert!((old.width_share - 0.4).abs() < 1e-6);
        assert!(!old.show_marks);
        // A file that never named the tape's layers gets the tape's defaults —
        // there is nothing to inherit from any more.
        assert!(
            old.enabled,
            "a file from before the switch opens with a tape"
        );
        assert!(old.show_depth && old.show_aggressions);

        // A file that said `false` said it on purpose, and is still obeyed —
        // the one thing the switch to a plain `bool` may not quietly discard.
        let chosen: LiveLaneStyle =
            toml::from_str("show_depth = false\nshow_aggressions = false\nenabled = false\n")
                .unwrap();
        assert!(!chosen.show_depth, "an explicit no stays a no");
        assert!(!chosen.show_aggressions);
        assert!(!chosen.enabled, "and a tape put away stays away");

        // A file this build writes round-trips, in both languages.
        for window in [
            LaneWindow::default(),
            LaneWindow::Auto { zoom: 0.5 },
            LaneWindow::Fixed { ms: 120_000 },
        ] {
            let style = LiveLaneStyle {
                window,
                enabled: false,
                show_depth: false,
                show_aggressions: true,
                ..LiveLaneStyle::default()
            };
            let text = toml::to_string(&style).unwrap();
            assert_eq!(toml::from_str::<LiveLaneStyle>(&text).unwrap(), style);
        }

        // Pinning parks the zoom at its default rather than hoarding the last
        // one: "automatic" means the bars decide, not that a multiplier is
        // waiting to reappear.
        let pinned = LiveLaneStyle {
            window: LaneWindow::Fixed { ms: 60_000 },
            ..LiveLaneStyle::default()
        };
        let text = toml::to_string(&pinned).unwrap();
        assert!(text.contains("window_ms = 60000"), "{text}");
        assert!(text.contains("time_zoom = 1.0"), "{text}");
    }

    #[test]
    fn the_lane_overrides_only_what_it_was_given() {
        let bubbles = BubbleStyle::default();
        let tuned = LiveLaneStyle {
            cluster_ms: Some(50),
            radius_scale: 2.0,
            ..LiveLaneStyle::default()
        };
        assert_eq!(
            tuned.effective_cluster_ms(DEFAULT_BUBBLE_CLUSTER_MS, 8_000),
            50
        );
        let (min, max) = tuned.scaled_radii(&bubbles);
        assert!((min - bubbles.min_radius * 2.0).abs() < 1e-4);
        assert!((max - bubbles.max_radius * 2.0).abs() < 1e-4);
        // Even an absurd scale stays inside the radius bounds the renderer
        // was built for, and never inverts the range.
        let huge = LiveLaneStyle {
            radius_scale: MAX_LIVE_LANE_RADIUS_SCALE,
            ..LiveLaneStyle::default()
        };
        let (min, max) = huge.scaled_radii(&BubbleStyle {
            min_radius: MAX_BUBBLE_MIN_RADIUS,
            max_radius: MAX_BUBBLE_MAX_RADIUS,
            ..BubbleStyle::default()
        });
        assert!(min <= max);
        assert!(max <= MAX_BUBBLE_MAX_RADIUS);
        assert!(min <= MAX_BUBBLE_MIN_RADIUS);
    }
}
