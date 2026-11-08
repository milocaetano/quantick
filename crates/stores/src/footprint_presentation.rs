//! Configured footprint presentation: layout policy and per-pane cache state.
mod pane;
pub use pane::PaneFootprint;

use crate::footprint_config::{CandleTreatment, FootprintStyle};
pub use quantick_chart::footprint_lod::candle_body_fade;
#[cfg(test)]
use quantick_chart::footprint_lod::*;
use quantick_chart::footprint_lod::{AXIS_CLEARANCE_PX, LevelMemory, detailed_entry_width};
use quantick_chart::footprint_projection::heat::HeatScale;
use quantick_chart::style::CandleStyle;
use quantick_engine::BarFootprint;
use rust_decimal::Decimal;

/// Clearance between a number and the bar's central axis, per side.
///
/// The floors below are budgets for text anchored *at* `xc`; the ladder
/// anchors at `xc ± this`, and for two releases the difference was simply
/// missing from the arithmetic — 3 px a side of quantity that the floor never
/// bought, so at the floor exactly the digits reached past the body they were
/// drawn in. Naming it is what keeps the two in step: the draw call and the
/// floor now read the same constant, and the test models it.
pub const CENTER_GUTTER_PX: f32 = AXIS_CLEARANCE_PX;

/// Quantities the sell|buy ladder writes across a row. Fixed, unlike the
/// cluster's, which the trader can switch between two and three.
#[cfg(test)]
const LADDER_QUANTITY_COLUMNS: f32 = 2.0;

/// The ladder's own Detailed floor. Kept as a named value because the
/// level tests reason about a single reference width; every *style* asks
/// [`detailed_min_width`] for its own.
#[cfg(test)]
fn ladder_detailed_min_width() -> f32 {
    detailed_min_width_for(FootprintStyle::Ladder, LADDER_QUANTITY_COLUMNS)
}

/// The candle width at which a style's deepest level fits what it writes, in
/// pixels — the typographic budget restated as arithmetic, so a retune has to
/// move a number that means something.
///
/// Two terms, and the second is the one that bites. The *text* term is how
/// many quantities the style writes; the *furniture* term is everything the
/// style puts around them. A floor that counts only the text is a floor that
/// promises room it does not have — the cluster spends 17 px per bar on its
/// candle lane, its box padding and its gutters before a digit is drawn, and
/// with those unbudgeted its columns overlap at exactly the width the floor
/// declares legible.
///
/// Moved by the trader's `detailed_from_px` ([`detailed_entry_width`]).
pub fn detailed_min_width(
    style: FootprintStyle,
    config: &crate::footprint_config::FootprintConfig,
) -> f32 {
    let typographic = detailed_min_width_for(style, style.detailed_quantity_columns(config));
    detailed_entry_width(typographic, config.detailed_from_px, config.detail_scale)
}

/// The same, for a column count already known.
fn detailed_min_width_for(style: FootprintStyle, columns: f32) -> f32 {
    let furniture = if style == FootprintStyle::Cluster {
        (columns - 1.0) * CLUSTER_GUTTER_PX
            + 2.0 * CLUSTER_BOX_PAD_PX
            + style.candle_treatment().content_inset()
    } else {
        columns * CENTER_GUTTER_PX
    };
    quantick_chart::footprint_lod::detailed_width(columns, furniture)
}

/// The row floors moved into config (`profile_row_px`, `number_row_px`,
/// same defaults); the constant stays as the tests' reference value.
#[cfg(test)]
const DEFAULT_FLOORS: RowFloors = RowFloors::new(4.0, DETAILED_MIN_ROW);

/// What a *painting* footprint layer does to the candles under it: the lane it
/// takes at the left of each slot, and the candle style it leaves behind
/// (`None` meaning "the chart's own, untouched").
///
/// One function because the two answers are one decision, and because the
/// decision has exactly one input that is easy to get wrong. `paints` is
/// whether the layer will actually *draw* this frame — not whether the
/// ladders are accumulating. The two diverge: a fixed-range volume profile
/// folds the same ladders with the layer hidden, and for a while that switch
/// was what reached here, so dropping a profile on a chart moved every candle
/// into a sidebar lane and faded its body to an outline for a layer that drew
/// nothing. The layer that does not paint does not dress.
#[must_use]
pub fn candle_dressing(
    paints: bool,
    treatment: CandleTreatment,
    candle_width: f32,
    fade_end: f32,
    base: CandleStyle,
) -> (f32, Option<CandleStyle>) {
    if !paints {
        return (0.0, None);
    }
    match treatment {
        // The candle cedes its interior to the ladder as the zoom crosses
        // into detail: the body fill fades out and the outline steps in — the
        // reference charts' outline-box candles.
        CandleTreatment::Fade => {
            let body = candle_body_fade(candle_width, fade_end);
            let dressed = (body < 1.0).then(|| {
                let mut faded = base;
                faded.fill_opacity *= body;
                faded.outline_opacity = faded.outline_opacity.max(1.0 - body);
                faded
            });
            (treatment.content_inset(), dressed)
        }
        // Beside the box, the candle is a solid sliver again: nothing is
        // behind anything, so there is nothing to fade *for*, and a 3 px
        // outline-only bar would read as a scratch.
        CandleTreatment::Sidebar => {
            let mut sidebar = base;
            sidebar.fill_opacity = 1.0;
            (treatment.content_inset(), Some(sidebar))
        }
    }
}

/// The bar slots a footprint has a ladder to draw in: the only candles it
/// may dress.
///
/// The layer fades a body so its numbers can be read inside it. A bar with
/// no ladder — the venue's history prefix, which carries no tape, or a bar
/// that traded nothing the layer saw — has nothing to read there, and
/// fading it anyway washed a whole chart of venue history out to pastels as
/// the trader zoomed in, under a layer that drew nothing on any of it.
#[derive(Debug, Clone, Copy)]
pub struct LadderSlots<'a> {
    first_state_slot: usize,
    closed: &'a [BarFootprint],
    partial_slot: Option<usize>,
}

impl<'a> LadderSlots<'a> {
    /// Closed ladders indexed from `first_state_slot` (the prefix length),
    /// and the forming bar's ladder at its slot, when it is on screen.
    #[must_use]
    pub fn new(
        first_state_slot: usize,
        closed: &'a [BarFootprint],
        partial: Option<(usize, &BarFootprint)>,
    ) -> Self {
        Self {
            first_state_slot,
            closed,
            partial_slot: partial
                .filter(|(_, ladder)| !ladder.levels().is_empty())
                .map(|(slot, _)| slot),
        }
    }

    /// Whether the bar at `slot` has a ladder with at least one level.
    #[must_use]
    pub fn carries(&self, slot: usize) -> bool {
        self.partial_slot == Some(slot)
            || slot
                .checked_sub(self.first_state_slot)
                .and_then(|index| self.closed.get(index))
                .is_some_and(|ladder| !ladder.levels().is_empty())
    }
}

/// The styles one frame's candles paint in: the chart's own, and the one a
/// footprint dresses the bars it has a ladder for in.
#[derive(Debug, Clone, Copy)]
pub struct CandleStyles<'a> {
    base: &'a CandleStyle,
    dressed: Option<(&'a CandleStyle, LadderSlots<'a>)>,
}

impl<'a> CandleStyles<'a> {
    /// `dressed` is the footprint's style this frame, if it dresses at all.
    #[must_use]
    pub fn new(
        base: &'a CandleStyle,
        dressed: Option<&'a CandleStyle>,
        ladders: LadderSlots<'a>,
    ) -> Self {
        Self {
            base,
            dressed: dressed.map(|style| (style, ladders)),
        }
    }

    /// The style the frame's candle layout follows: the dressed one while
    /// the footprint dresses, so every bar keeps one width.
    #[must_use]
    pub fn frame(&self) -> &'a CandleStyle {
        self.dressed.map_or(self.base, |(style, _)| style)
    }

    /// The style the bar at `slot` paints in.
    #[must_use]
    pub fn of(&self, slot: usize) -> &'a CandleStyle {
        match self.dressed {
            Some((style, ladders)) if ladders.carries(slot) => style,
            _ => self.base,
        }
    }
}

/// The smallest detail level — and the row multiple — with hysteresis
/// applied, per pane.
#[derive(Debug, Default)]
pub struct FootprintLod {
    /// The detail level and row multiple, each held through its dead band.
    pub levels: LevelMemory,
    /// The adaptive imbalance floor and the state it was computed from:
    /// `(closed bar count, capture group)`. See [`Self::adaptive_floor`].
    floor: Option<(usize, Decimal, Decimal)>,
    /// The heat ramp's cuts and the state they were computed from:
    /// `(first slot, last slot, closed bar count, display multiple)`. See
    /// [`Self::heat_scale`].
    heat: Option<(usize, usize, usize, i64, Option<HeatScale>)>,
    /// The style the last painted frame actually drew, after any handover.
    ///
    /// Published because the *candle* has to be laid out before the layer
    /// paints, and its layout depends on which style is really drawing: a
    /// boxed style moves the candle into a lane beside it, and a style that
    /// handed over does not. Reading the requested style instead squeezed the
    /// candle into a lane that the style which actually drew then painted
    /// straight over.
    ///
    /// One frame behind, and that is the whole cost: width carries no dead
    /// band, so a zoom across a style's floor lays the candle out for the old
    /// style for exactly one frame, and the next frame reads the new one.
    pub drawn_style: Option<crate::footprint_config::FootprintStyle>,
    /// Candles per aggression-summary mark, held through a steady zoom.
    pub candle_groups: quantick_orderflow::projection::CandleGroupMemory,
}

impl FootprintLod {
    /// The adaptive imbalance floor, recomputed only when the closed-bar
    /// count or the capture grid changes.
    ///
    /// The value is a fact about the newest closed bars, not about the
    /// frame — its own doc says so — but computing it per frame walked
    /// every row of 50 ladders and sorted them, at 60 Hz, for a number that
    /// changes once per bar. The cache key is what the answer depends on;
    /// `bars` growing is exactly "a bar closed".
    pub fn adaptive_floor(
        &mut self,
        bars: usize,
        group: Decimal,
        compute: impl FnOnce() -> Decimal,
    ) -> Decimal {
        if let Some((cached_bars, cached_group, floor)) = self.floor
            && cached_bars == bars
            && cached_group == group
        {
            return floor;
        }
        let floor = compute();
        self.floor = Some((bars, group, floor));
        floor
    }

    /// The style the previous painted frame drew, or `requested` before there
    /// has been one. See [`Self::drawn_style`].
    #[must_use]
    pub fn effective_style(
        &self,
        requested: crate::footprint_config::FootprintStyle,
    ) -> crate::footprint_config::FootprintStyle {
        use crate::footprint_config::FootprintStyle;
        match self.drawn_style {
            // `auto` is a question, not a look: whatever concrete style the
            // last frame answered with is the one the candle must be laid out
            // for.
            Some(drawn) if requested == FootprintStyle::Auto => drawn,
            // A handover from a concrete style only ever goes one way, so a
            // remembered style that is not this one is meaningful only while
            // it is this one's fallback.
            Some(drawn) if requested.fallback() == Some(drawn) => drawn,
            _ => requested,
        }
    }

    /// The heat ramp's cuts, recomputed only when the window they describe
    /// moves.
    ///
    /// The cuts are a fact about the ladders on screen, not about the frame.
    /// Computing them per frame means allocating and sorting every visible
    /// cell at 60 Hz for an answer that changes when the trader pans, zooms or
    /// a bar closes — the same trade [`Self::adaptive_floor`] makes, and for
    /// the same reason.
    ///
    /// The key is everything the answer depends on, and the fourth part is the
    /// one that is easy to miss: the cuts are measured on *display* rows, so
    /// they move when the display multiple does — and that multiple answers to
    /// the **price** zoom, not the time zoom. Dragging the price gutter
    /// regroups every row without touching which slots are visible or how many
    /// bars have closed, so a key made only of those three would hand back
    /// cuts for a grid that no longer exists.
    pub fn heat_scale(
        &mut self,
        visible: (usize, usize),
        bars: usize,
        k: i64,
        compute: impl FnOnce() -> Option<HeatScale>,
    ) -> Option<HeatScale> {
        if let Some((first, last, cached_bars, cached_k, scale)) = self.heat
            && first == visible.0
            && last == visible.1
            && cached_bars == bars
            && cached_k == k
        {
            return scale;
        }
        let scale = compute();
        self.heat = Some((visible.0, visible.1, bars, k, scale));
        scale
    }
}

/// Inset of the cluster's columns from its box, and the gutter between them.
pub const CLUSTER_BOX_PAD_PX: f32 = 2.0;

pub const CLUSTER_GUTTER_PX: f32 = 3.0;

/// The same, from a candle width rather than a body width — what the floor's
/// own test asks.
#[cfg(test)]
fn cluster_column_px(candle_width: f32) -> f32 {
    cluster_column_px_from(
        candle_width * TYPICAL_BODY_FRAC,
        FootprintStyle::Cluster
            .detailed_quantity_columns(&crate::footprint_config::FootprintConfig::default()),
    )
}

pub fn cluster_column_px_from(body_width: f32, columns: f32) -> f32 {
    let inner = body_width
        - crate::footprint_config::CANDLE_LANE_PX
        - 2.0 * CLUSTER_BOX_PAD_PX
        - (columns - 1.0) * CLUSTER_GUTTER_PX;
    (inner / columns).max(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr as _;
    fn dec(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }
    /// The zoom ceiling must reach every style's uncapped text floor, or the
    /// entry cap at the deepest zoom would draw digits that do not fit — the
    /// reason MAX_CANDLE_WIDTH rose. A cross-constant guard on purpose: it
    /// fires when either drifts under the other.
    #[test]
    fn every_style_is_reachable_at_some_zoom_and_every_detail_scale() {
        use crate::footprint_config::{FootprintConfig, FootprintStyle};
        // Past the deepest zoom a detail scale allows, the entry is answered
        // at that zoom (`detailed_entry_width`), which on screen is the
        // ceiling itself. That is only honest while the text the style writes
        // fits there — measured uncapped, every column count it can take.
        for style in FootprintStyle::ALL {
            for cluster_show_total in [true, false] {
                let config = FootprintConfig {
                    cluster_show_total,
                    ..FootprintConfig::default()
                };
                let columns = style.detailed_quantity_columns(&config);
                let floor = detailed_min_width_for(style, columns);
                assert!(
                    floor <= quantick_chart::viewport::MAX_CANDLE_WIDTH,
                    "{} ({columns} columns): text floor {floor:.1} px is past the {:.1} px \
                     zoom ceiling",
                    style.id(),
                    quantick_chart::viewport::MAX_CANDLE_WIDTH
                );
            }
        }
    }

    /// Turning the third column off is meant to buy a shallower zoom, and the
    /// file and the panel both say so. It has to actually move the floor.
    #[test]
    fn dropping_the_total_column_lowers_the_clusters_floor() {
        use crate::footprint_config::{FootprintConfig, FootprintStyle};
        let with = FootprintConfig::default();
        let without = FootprintConfig {
            cluster_show_total: false,
            ..FootprintConfig::default()
        };
        let three = detailed_min_width(FootprintStyle::Cluster, &with);
        let two = detailed_min_width(FootprintStyle::Cluster, &without);
        assert!(
            two < three,
            "two columns {two:.1} px vs three {three:.1} px"
        );
        // And the styles that do not own that switch are unmoved by it.
        for style in [FootprintStyle::Split, FootprintStyle::Ladder] {
            assert!(
                (detailed_min_width(style, &with) - detailed_min_width(style, &without)).abs()
                    < f32::EPSILON,
                "{} moved with a knob it does not own",
                style.id()
            );
        }
    }

    #[test]
    fn levels_need_both_width_and_a_reachable_row_height() {
        // Wide candle, healthy rows: full detail.
        assert_eq!(
            level_for(
                100.0,
                12.0,
                DEFAULT_FLOORS,
                COMPACT_MIN_WIDTH,
                ladder_detailed_min_width()
            ),
            DetailLevel::Detailed
        );
        // Wide candle, hairline base rows: grouping x100 still reaches 12px.
        assert_eq!(
            level_for(
                100.0,
                0.2,
                DEFAULT_FLOORS,
                COMPACT_MIN_WIDTH,
                ladder_detailed_min_width()
            ),
            DetailLevel::Detailed
        );
        // Wide candle, sub-hairline rows: the extended snap ladder rescues
        // detail far deeper than 100× (an index future on the 0.01 fallback
        // grid), so only truly hopeless rows drop to profile, then marks.
        assert_eq!(
            level_for(
                100.0,
                0.05,
                DEFAULT_FLOORS,
                COMPACT_MIN_WIDTH,
                ladder_detailed_min_width()
            ),
            DetailLevel::Detailed
        );
        assert_eq!(
            level_for(
                100.0,
                0.0006,
                DEFAULT_FLOORS,
                COMPACT_MIN_WIDTH,
                ladder_detailed_min_width()
            ),
            DetailLevel::Profile
        );
        assert_eq!(
            level_for(
                100.0,
                0.0003,
                DEFAULT_FLOORS,
                COMPACT_MIN_WIDTH,
                ladder_detailed_min_width()
            ),
            DetailLevel::Marks
        );
        // Width floors gate exactly — stated against the floors themselves, so
        // retuning one moves its own test rather than breaking four others.
        for (width, expected) in [
            (ladder_detailed_min_width(), DetailLevel::Detailed),
            (ladder_detailed_min_width() - 1.0, DetailLevel::Compact),
            (COMPACT_MIN_WIDTH, DetailLevel::Compact),
            (COMPACT_MIN_WIDTH - 1.0, DetailLevel::Profile),
            (PROFILE_MIN_WIDTH, DetailLevel::Profile),
            (PROFILE_MIN_WIDTH - 1.0, DetailLevel::Marks),
            (MARKS_MIN_WIDTH, DetailLevel::Marks),
            (MARKS_MIN_WIDTH - 1.0, DetailLevel::Off),
        ] {
            assert_eq!(
                level_for(
                    width,
                    12.0,
                    DEFAULT_FLOORS,
                    COMPACT_MIN_WIDTH,
                    ladder_detailed_min_width()
                ),
                expected,
                "at {width} px per candle"
            );
        }
    }

    /// `auto` walks the chain richest-first and always lands somewhere.
    ///
    /// The point of the style is that one wheel does the whole ladder, so the
    /// two things worth pinning are that the walk is *monotonic* — zooming in
    /// never gives a poorer reading — and that it has no floor of its own to
    /// fall through: at one pixel a candle it still answers with a style that
    /// needs no digits.
    #[test]
    fn auto_walks_the_chain_and_never_falls_through() {
        use crate::footprint_config::{FootprintConfig, FootprintStyle};
        let config = FootprintConfig::default();
        let at = |width: f32| {
            FootprintStyle::Auto.resolve_auto(|style| width >= detailed_min_width(style, &config))
        };
        // Richest first, and each link is reached at its own floor.
        assert_eq!(
            at(detailed_min_width(FootprintStyle::Cluster, &config)),
            FootprintStyle::Cluster
        );
        assert_eq!(
            at(detailed_min_width(FootprintStyle::Ladder, &config)),
            FootprintStyle::Ladder
        );
        // Below every floor it still draws: the chain ends on a shape.
        let last = at(quantick_chart::viewport::MIN_PX_PER_BAR);
        assert!(
            !last.draws_own_rows() || last == FootprintStyle::Split,
            "auto fell through to {last:?}"
        );
        assert_ne!(last, FootprintStyle::Auto, "auto resolved to itself");

        // Monotonic: widening the candle never buys a poorer reading. The
        // chain's own order is the ranking, so a walk up the widths must never
        // move backwards through it.
        let rank = |style: FootprintStyle| {
            FootprintStyle::AUTO_CHAIN
                .iter()
                .position(|link| *link == style)
                .expect("auto resolves inside its own chain")
        };
        let mut previous = rank(at(quantick_chart::viewport::MIN_PX_PER_BAR));
        let mut width = quantick_chart::viewport::MIN_PX_PER_BAR;
        while width <= quantick_chart::viewport::MAX_CANDLE_WIDTH {
            let here = rank(at(width));
            assert!(here <= previous, "the walk went backwards at {width} px");
            previous = here;
            width += 1.0;
        }
    }

    /// A style that cannot pay for its own detail hands over to the one it
    /// names, and never to itself.
    #[test]
    fn cluster_hands_over_below_its_own_floor() {
        use crate::footprint_config::FootprintStyle;
        assert_eq!(
            FootprintStyle::Cluster.fallback(),
            Some(FootprintStyle::BidAsk)
        );
        // The handover target must itself be drawable where the handover
        // happens, or the fallback is a blank chart.
        assert!(FootprintStyle::BidAsk.draws_own_rows());
        assert!(FootprintStyle::BidAsk.fallback().is_none());
        // And the cluster's floor is genuinely higher, or the handover never
        // fires and the whole mechanism is decoration.
        let defaults = crate::footprint_config::FootprintConfig::default();
        let cluster = detailed_min_width(FootprintStyle::Cluster, &defaults);
        let ladder = detailed_min_width(FootprintStyle::Ladder, &defaults);
        assert!(cluster > ladder, "cluster {cluster} vs ladder {ladder}");
    }

    /// Every style is reachable by the token the hook and the TOML speak, and
    /// no two share one. A style the registry cannot name is a style the
    /// second operator cannot pick.
    #[test]
    fn every_style_round_trips_through_its_id() {
        use crate::footprint_config::FootprintStyle;
        let mut seen = std::collections::BTreeSet::new();
        for style in FootprintStyle::ALL {
            assert!(seen.insert(style.id()), "duplicate id {}", style.id());
            assert_eq!(FootprintStyle::from_id(style.id()), Some(style));
            assert!(!style.label().is_empty());
            assert!(!style.hover().is_empty());
            assert!(!style.detailed_legend().is_empty());
        }
        assert_eq!(FootprintStyle::from_id("no-such-style"), None);
    }

    #[test]
    fn every_text_floor_still_fits_the_text_it_draws() {
        let quantity_px = QUANTITY_GLYPHS * GLYPH_EM * LADDER_MIN_FONT_PX;
        // Compact writes one quantity across the whole body.
        let compact_body = COMPACT_MIN_WIDTH * TYPICAL_BODY_FRAC;
        assert!(
            compact_body >= quantity_px,
            "compact: {compact_body} px of body for {quantity_px} px of text"
        );
        // Detailed writes one per half of it — and the halves do not start at
        // the axis. The ladder anchors its columns at `xc +- CENTER_GUTTER_PX`,
        // and for two releases that clearance was missing from the floor: at
        // the floor exactly, the digits reached past the body they were drawn
        // in. Modelling the gutter here is what stops the two drifting apart
        // again.
        let detailed_half =
            ladder_detailed_min_width() * TYPICAL_BODY_FRAC / 2.0 - CENTER_GUTTER_PX;
        assert!(
            detailed_half >= quantity_px,
            "detailed: {detailed_half} px per half (gutter removed) for {quantity_px} px of text"
        );
        // The same arithmetic has to hold for a style that writes three
        // quantities across the row rather than two, which is the whole reason
        // the floor became a function of the column count.
        let defaults = crate::footprint_config::FootprintConfig::default();
        let cluster_column =
            cluster_column_px(detailed_min_width(FootprintStyle::Cluster, &defaults));
        assert!(
            cluster_column >= quantity_px,
            "cluster: {cluster_column} px per column for {quantity_px} px of text"
        );
        // And the ordering that makes them levels at all.
        assert!(ladder_detailed_min_width() > COMPACT_MIN_WIDTH);
        const {
            assert!(COMPACT_MIN_WIDTH > PROFILE_MIN_WIDTH);
            assert!(PROFILE_MIN_WIDTH > MARKS_MIN_WIDTH);
        }
    }

    /// Detail arrives sooner than it used to at every level — the point of the
    /// retune — and a `detail_scale` below one moves the whole ladder together
    /// without reordering it.
    #[test]
    fn detail_arrives_earlier_than_the_old_floors_and_scales_as_one() {
        // What each floor was when the ladder's font floor was 8 px.
        for (now, before) in [
            (ladder_detailed_min_width(), 72.0),
            (COMPACT_MIN_WIDTH, 40.0),
            (PROFILE_MIN_WIDTH, 18.0),
            (MARKS_MIN_WIDTH, 8.0),
        ] {
            assert!(now < before, "{now} is no earlier than {before}");
        }
        // A scale below one says the same thing as narrower floors: at a given
        // candle width the level can only improve, never reorder.
        let tight = *crate::footprint_config::DETAIL_SCALE_RANGE.start();
        assert!(tight < 1.0);
        for width in [6.0_f32, 10.0, 20.0, 35.0, 63.0, 120.0] {
            let plain = level_for(
                width,
                12.0,
                DEFAULT_FLOORS,
                COMPACT_MIN_WIDTH,
                ladder_detailed_min_width(),
            );
            let scaled = level_for(
                width / tight,
                12.0,
                DEFAULT_FLOORS,
                COMPACT_MIN_WIDTH,
                ladder_detailed_min_width(),
            );
            assert!(scaled >= plain, "at {width} px");
        }
    }

    /// Full body up to the Profile floor, outline-only by the Detailed
    /// floor, monotonic in between — and never outside [0, 1].
    #[test]
    fn candle_body_fade_spans_profile_to_detailed() {
        let defaults = crate::footprint_config::FootprintConfig::default();
        let end = defaults.body_fade_end_px();
        assert!((end - DETAILED_FROM_PX).abs() < 1e-3, "{end}");
        let fade = |width: f32| candle_body_fade(width, end);
        assert_eq!(fade(8.0), 1.0);
        assert_eq!(fade(PROFILE_MIN_WIDTH), 1.0);
        assert_eq!(fade(end), 0.0);
        assert_eq!(fade(160.0), 0.0);
        let mid = fade((PROFILE_MIN_WIDTH + end) / 2.0);
        assert!(mid > 0.0 && mid < 1.0);
        assert!(fade(30.0) > fade(50.0));
        // The fade ends where the trader's Detailed starts, not at the floor.
        let later = crate::footprint_config::FootprintConfig {
            detailed_from_px: 120.0,
            ..defaults
        };
        let end = later.body_fade_end_px();
        assert!((end - 120.0).abs() < 1e-3, "{end}");
        assert!(candle_body_fade(100.0, end) > 0.0);
        assert_eq!(candle_body_fade(120.0, end), 0.0);
    }

    #[test]
    fn width_changes_the_level_at_the_floor_in_both_directions() {
        // Written as multiples of the floor rather than as pixels, so the
        // symmetry is tested wherever the floor is tuned to.
        let floor = ladder_detailed_min_width();
        let mut lod = FootprintLod::default();
        let mut at = |width: f32| {
            lod.levels
                .resolve(width, 12.0, DEFAULT_FLOORS, COMPACT_MIN_WIDTH, floor)
        };
        // Width carries no dead band: the level a width shows does not
        // depend on the direction it was reached from.
        for (width, expected) in [
            (floor * 1.2, DetailLevel::Detailed),
            (floor * 1.01, DetailLevel::Detailed),
            (floor * 0.99, DetailLevel::Compact),
            (floor * 1.01, DetailLevel::Detailed),
            (COMPACT_MIN_WIDTH * 0.99, DetailLevel::Profile),
            (COMPACT_MIN_WIDTH * 1.01, DetailLevel::Compact),
        ] {
            assert_eq!(at(width), expected, "width {width}");
        }
    }

    /// The row side keeps its dead band: the auto-fit breathing a few
    /// percent across a row floor with every print must not blink the level.
    #[test]
    fn row_jitter_inside_the_dead_band_holds_the_level() {
        let mut lod = FootprintLod::default();
        // Base rows so thin that only the coarsest snap reaches a text row.
        let coarsest = *GROUP_SNAP.last().unwrap() as f32;
        let row = |share: f32| DETAILED_MIN_ROW / coarsest * share;
        let mut at = |share: f32| {
            lod.levels.resolve(
                100.0,
                row(share),
                DEFAULT_FLOORS,
                COMPACT_MIN_WIDTH,
                ladder_detailed_min_width(),
            )
        };
        assert_eq!(at(1.02), DetailLevel::Detailed);
        for share in [0.98, 1.02, 0.95, 1.01, 0.97] {
            assert_eq!(
                at(share),
                DetailLevel::Detailed,
                "row share {share} blinked"
            );
        }
    }

    /// The annotated zoom sequence enters Profile after the fifth inward
    /// wheel step and leaves it on the first step back. The legend's 4.2x and
    /// 2.9x readings give the candle widths without relying on screenshot DPI.
    #[test]
    fn profile_enters_on_fifth_step_and_leaves_on_first_reverse() {
        let opening_width = COMPACT_MIN_WIDTH / 4.2;
        let wheel_factor = (4.2_f32 / 2.9).powf(0.25);
        let mut lod = FootprintLod::default();
        let mut at = |width: f32| {
            lod.levels.resolve(
                width,
                12.0,
                DEFAULT_FLOORS,
                COMPACT_MIN_WIDTH,
                ladder_detailed_min_width(),
            )
        };
        for step in 0..=5 {
            let width = opening_width * wheel_factor.powi(step);
            let expected = if step == 5 {
                DetailLevel::Profile
            } else {
                DetailLevel::Marks
            };
            assert_eq!(
                at(width),
                expected,
                "inward wheel step {step} at {width:.1} px"
            );
        }
        assert_eq!(
            at(opening_width * wheel_factor.powi(4)),
            DetailLevel::Marks,
            "the first outward step must restore Marks"
        );
        // Strict on both sides of the floor, whichever way it is crossed.
        assert_eq!(at(PROFILE_MIN_WIDTH * 1.01), DetailLevel::Profile);
        assert_eq!(at(PROFILE_MIN_WIDTH * 0.99), DetailLevel::Marks);
        assert_eq!(at(PROFILE_MIN_WIDTH * 1.01), DetailLevel::Profile);
    }

    /// The trader's two entry widths move the levels, and the defaults are
    /// the old zoom-in points: 15% past each typographic floor.
    #[test]
    fn the_configured_entry_widths_gate_both_directions() {
        let close = |a: f32, b: f32| (a - b).abs() < 1e-3;
        assert!(close(ladder_detailed_min_width(), DETAILED_MIN_WIDTH));
        let defaults = crate::footprint_config::FootprintConfig::default();
        assert!(close(defaults.compact_from_px, COMPACT_FROM_PX));
        assert!(close(
            detailed_min_width(FootprintStyle::Ladder, &defaults),
            ladder_detailed_min_width() * LEVEL_ENTRY_MARGIN
        ));
        let config = crate::footprint_config::FootprintConfig {
            compact_from_px: 50.0,
            detailed_from_px: 120.0,
            ..defaults
        };
        assert!(close(
            detailed_min_width(FootprintStyle::Ladder, &config),
            120.0
        ));
        // A wider style keeps its ratio to the ladder.
        let ratio = |config| {
            detailed_min_width(FootprintStyle::Cluster, config)
                / detailed_min_width(FootprintStyle::Ladder, config)
        };
        assert!((ratio(&config) - ratio(&defaults)).abs() < 1e-4);
        let mut lod = FootprintLod::default();
        let detailed = detailed_min_width(FootprintStyle::Ladder, &config);
        let mut at = |width: f32| {
            lod.levels.resolve(
                width,
                12.0,
                DEFAULT_FLOORS,
                config.compact_entry_width(),
                detailed,
            )
        };
        for (width, expected) in [
            (49.0, DetailLevel::Profile),
            (50.0, DetailLevel::Compact),
            (119.0, DetailLevel::Compact),
            (120.0, DetailLevel::Detailed),
            (119.0, DetailLevel::Compact),
            (49.0, DetailLevel::Profile),
        ] {
            assert_eq!(at(width), expected, "width {width}");
        }
    }

    /// The dead band defends one step of jitter, never a wedged state: a
    /// level locked in the first frames' wild auto-fit span must snap to
    /// the strict answer the moment it is more than one step away.
    #[test]
    fn a_wedged_level_or_multiple_snaps_back_to_strict() {
        let mut lod = FootprintLod::default();
        // Locked at Marks by a startup-era span (rows unreachable)...
        assert_eq!(
            lod.levels.resolve(
                100.0,
                0.0001,
                DEFAULT_FLOORS,
                COMPACT_MIN_WIDTH,
                ladder_detailed_min_width()
            ),
            DetailLevel::Marks
        );
        // ...then the real span arrives: two steps away, no band, snap.
        assert_eq!(
            lod.levels.resolve(
                100.0,
                12.0,
                DEFAULT_FLOORS,
                COMPACT_MIN_WIDTH,
                ladder_detailed_min_width()
            ),
            DetailLevel::Detailed
        );

        // Same for the row multiple: a 10 000x from a wild span must not
        // hold once the strict answer is orders of magnitude finer.
        let mut lod = FootprintLod::default();
        assert_eq!(lod.levels.resolve_multiple(0.0005, 4.0), Some(10_000));
        assert_eq!(lod.levels.resolve_multiple(0.09, 4.0), Some(50));
    }

    /// The floor is a fact about the closed bars, so it is computed once and
    /// reused until a bar closes or the capture grid moves — the per-frame
    /// version walked every row of 50 ladders at 60 Hz for a number that
    /// changes once per bar.
    #[test]
    fn the_adaptive_floor_is_computed_once_per_bar_not_per_frame() {
        let mut lod = FootprintLod::default();
        let calls = std::cell::Cell::new(0);
        let floor = |lod: &mut FootprintLod, bars: usize, group: Decimal| {
            lod.adaptive_floor(bars, group, || {
                calls.set(calls.get() + 1);
                dec("7")
            })
        };
        assert_eq!(floor(&mut lod, 50, dec("0.5")), dec("7"));
        assert_eq!(floor(&mut lod, 50, dec("0.5")), dec("7"));
        assert_eq!(floor(&mut lod, 50, dec("0.5")), dec("7"));
        assert_eq!(calls.get(), 1, "frames must not recompute the floor");
        floor(&mut lod, 51, dec("0.5"));
        assert_eq!(calls.get(), 2, "a bar closed");
        floor(&mut lod, 51, dec("5"));
        assert_eq!(calls.get(), 3, "the capture grid moved");
    }

    /// The heat cuts describe the window, so a frame that changes nothing
    /// about the window must not pay for them again.
    ///
    /// Per frame this walks every visible cell, allocates and sorts — cheap
    /// once, at 60 Hz a waste, and the same reason the imbalance floor is
    /// cached beside it. Panning, zooming or closing a bar are the three
    /// things that genuinely move the answer.
    #[test]
    fn the_heat_cuts_are_computed_once_per_window_not_per_frame() {
        let mut lod = FootprintLod::default();
        let calls = std::cell::Cell::new(0);
        let cuts: HeatScale = [1.0, 2.0, 3.0, 4.0, 5.0];
        let scale = |lod: &mut FootprintLod, visible: (usize, usize), bars: usize, k: i64| {
            lod.heat_scale(visible, bars, k, || {
                calls.set(calls.get() + 1);
                Some(cuts)
            })
        };
        assert_eq!(scale(&mut lod, (10, 40), 100, 1), Some(cuts));
        assert_eq!(scale(&mut lod, (10, 40), 100, 1), Some(cuts));
        assert_eq!(scale(&mut lod, (10, 40), 100, 1), Some(cuts));
        assert_eq!(calls.get(), 1, "frames must not recompute the cuts");
        scale(&mut lod, (11, 41), 100, 1);
        assert_eq!(calls.get(), 2, "the window panned");
        scale(&mut lod, (11, 60), 100, 1);
        assert_eq!(calls.get(), 3, "the window zoomed in time");
        scale(&mut lod, (11, 60), 101, 1);
        assert_eq!(calls.get(), 4, "a bar closed under it");
        // The one that is easy to miss: the price zoom regroups the rows the
        // cuts are measured on without moving a single slot.
        scale(&mut lod, (11, 60), 101, 5);
        assert_eq!(calls.get(), 5, "the price zoom regrouped the rows");
    }

    /// A layer that does not paint does not dress. This is the whole reason
    /// [`candle_dressing`] takes `paints` instead of reading the switch that
    /// turns the ladders on: a fixed-range volume profile turns those on
    /// while the layer stays hidden, and for a while that was what reached
    /// here — so dropping a profile on a chart moved every candle into the
    /// sidebar lane and faded its body to an outline, under nothing.
    #[test]
    fn a_hidden_footprint_leaves_the_candles_exactly_as_they_were() {
        let base = quantick_chart::style::ChartStyle::default().candles;
        for treatment in [CandleTreatment::Fade, CandleTreatment::Sidebar] {
            // Wide enough that `Fade` would really fade and `Sidebar` would
            // really claim its lane, so this cannot pass by accident.
            for candle_width in [4.0_f32, 40.0, 400.0] {
                assert_eq!(
                    candle_dressing(false, treatment, candle_width, DETAILED_FROM_PX, base),
                    (0.0, None),
                    "{treatment:?} at {candle_width} px, layer hidden"
                );
            }
        }
    }

    /// Venue history has no tape, so no ladder: the layer has nothing to
    /// show in those bodies and leaves them in the chart's own colours. A
    /// traded bar beside them is still dressed.
    #[test]
    fn only_bars_with_a_ladder_are_dressed() {
        let group = Decimal::ONE;
        let mut builder = quantick_engine::FootprintBuilder::new(group, 64);
        builder.push(&quantick_engine::Trade {
            agg_id: 1,
            timestamp_ms: 1_000,
            price: Decimal::from(100),
            quantity: Decimal::ONE,
            side: quantick_engine::Side::Buy,
        });
        let traded = builder.close().expect("a traded bar has a ladder");
        let empty = quantick_engine::FootprintBuilder::new(group, 64).close_or_empty();
        assert!(empty.levels().is_empty());
        let closed = [traded.clone(), empty];
        // Slots 0..3 are the venue prefix; state bars start at slot 3.
        let ladders = LadderSlots::new(3, &closed, None);
        assert!(!ladders.carries(0), "venue history carries no ladder");
        assert!(ladders.carries(3), "a traded bar does");
        assert!(!ladders.carries(4), "an empty ladder draws nothing");
        assert!(!ladders.carries(5), "past the closed bars");

        let forming = LadderSlots::new(3, &closed, Some((5, &traded)));
        assert!(forming.carries(5), "the forming bar's ladder counts");
        let empty_forming = quantick_engine::FootprintBuilder::new(group, 64).close_or_empty();
        assert!(!LadderSlots::new(3, &closed, Some((5, &empty_forming))).carries(5));
    }

    /// And the other half of the same law: with the layer painting, the
    /// dressing is still there. A fix that simply stopped dressing candles
    /// would pass the test above and break the footprint.
    #[test]
    fn a_painting_footprint_still_dresses_the_candles() {
        let base = quantick_chart::style::ChartStyle::default().candles;
        let wide = ladder_detailed_min_width().max(PROFILE_MIN_WIDTH) + 1.0;

        let (lane, dressed) =
            candle_dressing(true, CandleTreatment::Sidebar, wide, DETAILED_FROM_PX, base);
        assert!(lane > 0.0, "the sidebar candle keeps a lane of its own");
        assert_eq!(
            dressed.expect("a sidebar candle is restyled").fill_opacity,
            1.0,
            "beside the box the candle is solid again"
        );

        // The fade runs from Profile (untouched) to Detailed (outline only),
        // so it is halfway across that span that the body is really fading.
        let halfway = f32::midpoint(PROFILE_MIN_WIDTH, DETAILED_FROM_PX);
        let (lane, dressed) =
            candle_dressing(true, CandleTreatment::Fade, halfway, DETAILED_FROM_PX, base);
        assert_eq!(lane, 0.0, "a fading candle stays where it was");
        let faded = dressed.expect("the body fades once the zoom crosses Profile");
        assert!(faded.fill_opacity < base.fill_opacity);
        assert!(faded.outline_opacity >= base.outline_opacity);

        // At the Profile floor itself nothing has faded yet, and "nothing to
        // do" is reported as the chart's own style rather than a copy of it.
        assert_eq!(
            candle_dressing(
                true,
                CandleTreatment::Fade,
                PROFILE_MIN_WIDTH,
                DETAILED_FROM_PX,
                base
            ),
            (0.0, None),
            "the body is whole until the zoom crosses Profile"
        );
    }
}
