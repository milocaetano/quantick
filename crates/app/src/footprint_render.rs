//! Drawing the candle footprint layer: the LOD ladder, the display grouping
//! and the marks that survive zoom-out.
//!
//! The design (docs/footprint-design.md) in one paragraph: the effective
//! detail level is the *minimum* of what the candle width and the price-row
//! height allow, stepped discretely with a dead band so continuous zooming
//! never flickers between modes; before dropping a level the renderer first
//! coarsens the display grouping — an **integer** multiple of the capture
//! grid, so merging rows is exact arithmetic, and zero-anchored buckets keep
//! every bar's rows horizontally aligned. What survives zoom-out, in order:
//! stacked-imbalance zones, the POC, nothing else. No digits below
//! [`DetailLevel::Compact`], no fades anywhere: a half-transparent number is
//! illegible and present at the same time.
//!
//! Everything that decides (levels, grouping, zone coalescing, quantity
//! formatting) is a pure function tested without egui; only the painting
//! itself touches the frame.

use std::collections::BTreeMap;

use eframe::egui;
use quantick_engine::{BarFootprint, FootprintLevel, Side, StackedZone};
use rust_decimal::Decimal;
use rust_decimal::prelude::{FromPrimitive as _, ToPrimitive as _};

use crate::chart::PriceScale;
use crate::footprint_config::{CandleTreatment, FootprintStyle, StylePlate};
use crate::style::CandleStyle;
use crate::theme;

#[cfg(test)]
use bar::cluster_column_px_from;
use bar::{BarPaint, draw_bar, draw_poc_dot, draw_zone_mark};
use heat::{HeatScale, heat_scale};

mod bar;
mod heat;

/// How much detail the current zoom supports. Ordered: more detail is greater.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DetailLevel {
    /// Below every readable threshold: the layer paints nothing and the
    /// legend says why, because an on-but-invisible layer reads as broken.
    Off,
    /// POC dot and stacked-zone marks only. Nothing here is a number.
    Marks,
    /// Textless histogram per row, POC emphasized, zone ticks on the edge.
    Profile,
    /// One abbreviated delta number per row.
    Compact,
    /// The full sell × buy ladder with imbalance highlights and the extreme
    /// ratio badges.
    Detailed,
}

/// Smallest font the ladder draws its quantities at, in pixels.
///
/// Seven, down from eight. Monospace digits hold their shape a size below what
/// prose needs — they are a fixed, familiar alphabet of ten — and every text
/// floor below is measured from this number, so a pixel here is worth several
/// pixels of candle in how soon the numbers arrive.
const LADDER_MIN_FONT_PX: f32 = 7.0;

/// Advance width of a monospace glyph, as a fraction of the font size.
const GLYPH_EM: f32 = 0.6;

/// Glyphs in the widest quantity the ladder writes (`58.1k`).
const QUANTITY_GLYPHS: f32 = 5.0;

/// Width of that quantity at the smallest font, in pixels.
const QUANTITY_PX: f32 = QUANTITY_GLYPHS * GLYPH_EM * LADDER_MIN_FONT_PX;

/// Clearance kept around a quantity inside the body it is drawn in.
///
/// A pixel and a half a side, not the six the row layout reserves when it is
/// *sizing* the font: what a floor has to guarantee is that the digits do not
/// reach the next candle, and the body already sits inside a gap
/// ([`crate::style::DEFAULT_CANDLE_GAP`]) that keeps them apart.
const QUANTITY_PADDING_PX: f32 = 3.0;

/// The share of a slot a candle body takes at the default style. The numbers
/// are drawn inside the *body*, so this is what turns a text budget into a
/// candle width.
const TYPICAL_BODY_FRAC: f32 = 0.72;

/// Candle-width floors per level, in pixels — the typography budget of what
/// each level draws.
///
/// The two text levels are **derived, never chosen**: Compact fits one
/// quantity across the body, Detailed one per half of it. Writing them as
/// arithmetic is what keeps the retune honest — the floors moved because
/// [`LADDER_MIN_FONT_PX`] moved (8 px → 7 px), and anyone tightening them
/// further has to move a number that means something first.
///
/// That gap is much of why the layer read as *slow to arrive*: a trader zoomed
/// in for numbers, got marks, and had nothing saying how much further to go
/// (the legend now says it).
///
/// The two levels that draw no text answer to geometry instead, and had no
/// such excuse for waiting. Marks are a POC dot and a zone tick — visible from
/// a candle six pixels wide. The profile is a textless histogram whose *shape*
/// is the signal, readable at ten pixels where the old floor made it wait for
/// eighteen.
///
/// [`crate::footprint_config::FootprintConfig::detail_scale`] moves all four
/// together, for a trader who wants detail earlier still (and tighter) or
/// later and roomier.
/// Clearance between a number and the bar's central axis, per side.
///
/// The floors below are budgets for text anchored *at* `xc`; the ladder
/// anchors at `xc ± this`, and for two releases the difference was simply
/// missing from the arithmetic — 3 px a side of quantity that the floor never
/// bought, so at the floor exactly the digits reached past the body they were
/// drawn in. Naming it is what keeps the two in step: the draw call and the
/// floor now read the same constant, and the test models it.
const CENTER_GUTTER_PX: f32 = 2.0;

/// Quantities the sell|buy ladder writes across a row. Fixed, unlike the
/// cluster's, which the trader can switch between two and three.
const LADDER_QUANTITY_COLUMNS: f32 = 2.0;

/// The ladder's own Detailed floor. Kept as a named value because the
/// hysteresis tests and `candle_body_fade` reason about a single reference
/// width; every *style* asks [`detailed_min_width`] for its own.
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
fn detailed_min_width(
    style: FootprintStyle,
    config: &crate::footprint_config::FootprintConfig,
) -> f32 {
    detailed_min_width_for(style, style.detailed_quantity_columns(config))
}

/// The same, for a column count already known. Split out because the ladder's
/// count is fixed, and the fade curve below needs its floor without having a
/// config to hand.
fn detailed_min_width_for(style: FootprintStyle, columns: f32) -> f32 {
    let text = columns * (QUANTITY_PX + QUANTITY_PADDING_PX / 2.0);
    let furniture = match style {
        FootprintStyle::Cluster => {
            (columns - 1.0) * CLUSTER_GUTTER_PX
                + 2.0 * CLUSTER_BOX_PAD_PX
                + style.candle_treatment().content_inset()
        }
        // The in-candle styles keep a clearance either side of the axis.
        _ => columns * CENTER_GUTTER_PX,
    };
    (text + furniture) / TYPICAL_BODY_FRAC
}

const COMPACT_MIN_WIDTH: f32 = (QUANTITY_PX + QUANTITY_PADDING_PX) / TYPICAL_BODY_FRAC;

const PROFILE_MIN_WIDTH: f32 = 10.0;

const MARKS_MIN_WIDTH: f32 = 6.0;

/// Row-height floors per level. Profile rows survive down to hairline bands;
/// text rows need a legible line.
const DETAILED_MIN_ROW: f32 = 12.0;

const COMPACT_MIN_ROW: f32 = 11.0;

/// The Profile floor moved into config (`profile_row_px`, same default);
/// the constant stays as the tests' reference value for that default.
#[cfg(test)]
const PROFILE_MIN_ROW: f32 = 4.0;

/// The dead band on level *downgrades*: the current level survives until the
/// zoom is 15% past its floor, so a trackpad hovering on a boundary cannot
/// blink the chart between modes mid-gesture.
const LEVEL_HYSTERESIS: f32 = 1.15;

/// Display-grouping multiples, smallest first. Integer multiples of the
/// capture grid keep row merges exact; round values keep the effective
/// grouping a number a trader can say out loud. The ladder runs to 10 000×
/// deliberately: a feed that never reports its tick leaves the capture grid
/// on the 0.01 fallback, and an index future at 180 000 needs a 200–500×
/// merge before a row is even one visible pixel — capping at 100× silently
/// locked those charts in Marks at every zoom.
const GROUP_SNAP: [i64; 16] = [
    1, 2, 5, 10, 20, 25, 50, 100, 200, 250, 500, 1000, 2000, 2500, 5000, 10_000,
];

/// Hard cap of painted cells per frame, the heatmap's own budget: beyond it
/// the layer stops and the legend says "capped" instead of eating the frame.
const CELL_BUDGET: usize = 12_000;

/// Most stacked-zone marks on one screen. Beyond it only the tallest stacks
/// survive and the legend discloses the filter — a mark that always appears
/// has stopped being a signal.
const MAX_ZONE_MARKS: usize = 24;

/// How far above the chart's bottom edge the per-bar delta totals sit —
/// clear of the legend line below them.
const TOTALS_STRIP_OFFSET_Y: f32 = 22.0;

/// How much of the candle body's fill survives at `candle_width`, `1.0`
/// (untouched) through `0.0` (outline only).
///
/// The reference charts draw the candle as an outline box with the footprint
/// living inside it. Rather than a hard switch, the body fades as the zoom
/// crosses from "candles are the chart" (Marks) into "the inside is the
/// chart" (Profile → Detailed): fully opaque up to the Profile floor, gone
/// by the Detailed floor. Only applied while the layer is on — off, the
/// candles are untouched at any zoom.
pub fn candle_body_fade(candle_width: f32) -> f32 {
    let span = ladder_detailed_min_width() - PROFILE_MIN_WIDTH;
    (1.0 - (candle_width - PROFILE_MIN_WIDTH) / span).clamp(0.0, 1.0)
}

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
            let body = candle_body_fade(candle_width);
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

/// The smallest detail level — and the row multiple — with hysteresis
/// applied, per pane.
#[derive(Debug, Default)]
pub struct FootprintLod {
    level: Option<DetailLevel>,
    k: Option<i64>,
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
    /// One frame behind, and that is the whole cost: the level is sticky with
    /// its own dead band, so the boundary is crossed once and the stale answer
    /// survives a single frame of a gesture.
    drawn_style: Option<crate::footprint_config::FootprintStyle>,
}

impl FootprintLod {
    /// The level this zoom supports, sticky in BOTH directions (see
    /// [`LEVEL_HYSTERESIS`]). `profile_row_px` is the configured Profile
    /// floor — the "how fine may the bands get" knob.
    ///
    /// The dead band is two-sided on purpose: the price auto-fit breathes
    /// with every pan and print, so the row height crosses a floor and
    /// crosses back with a centimetre of mouse travel. With instant
    /// upgrades against banded downgrades, the boundary blinks — up at
    /// once, down 15% later, up at once again. A change in either
    /// direction now has to clear the floor with 15% to spare before the
    /// level moves; only the very first frame takes the strict answer.
    pub fn resolve(
        &mut self,
        candle_width: f32,
        base_row_px: f32,
        profile_row_px: f32,
        detailed_min: f32,
    ) -> DetailLevel {
        let strict = level_for(candle_width, base_row_px, profile_row_px, detailed_min);
        let level = match self.level {
            // The dead band defends exactly ONE step of boundary jitter.
            // Further than that, the sticky state is not jitter — it is a
            // leftover from another zoom era (the first frames' wild
            // auto-fit spans) — and holding it is how "rows 100.00" wedges
            // on a chart whose strict answer is Detailed.
            Some(current) if (strict as i8 - current as i8).abs() > 1 => strict,
            Some(current) if strict < current => {
                let relaxed = level_for(
                    candle_width * LEVEL_HYSTERESIS,
                    base_row_px * LEVEL_HYSTERESIS,
                    profile_row_px,
                    detailed_min,
                );
                if relaxed < current { strict } else { current }
            }
            Some(current) if strict > current => {
                let confirmed = level_for(
                    candle_width / LEVEL_HYSTERESIS,
                    base_row_px / LEVEL_HYSTERESIS,
                    profile_row_px,
                    detailed_min,
                );
                if confirmed >= strict { strict } else { current }
            }
            _ => strict,
        };
        self.level = Some(level);
        level
    }

    /// The adaptive imbalance floor, recomputed only when the closed-bar
    /// count or the capture grid changes.
    ///
    /// The value is a fact about the newest closed bars, not about the
    /// frame — its own doc says so — but computing it per frame walked
    /// every row of 50 ladders and sorted them, at 60 Hz, for a number that
    /// changes once per bar. The cache key is what the answer depends on;
    /// `bars` growing is exactly "a bar closed".
    fn adaptive_floor(
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
    fn heat_scale(
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

    /// The display multiple, with the same dead band the level has: the
    /// price auto-fit breathes with every new high of the live bar, and a
    /// ladder that restructures from 2-tick to 5-tick rows on one print and
    /// back on the next is unreadable. The current `k` survives until it is
    /// 15% past failing its floor, and a finer one is adopted only once it
    /// clears the floor with 15% to spare.
    fn resolve_multiple(&mut self, base_row_px: f32, min_row_px: f32) -> Option<i64> {
        let strict = display_multiple(base_row_px, min_row_px);
        let snap_position = |k: i64| GROUP_SNAP.iter().position(|snap| *snap == k);
        let k = match (self.k, strict) {
            (Some(current), Some(strict_k)) if current != strict_k => {
                // Same one-step rule as the level: the dead band defends
                // boundary jitter, never a multiple wedged eras away (the
                // snap quantization can leave the strict answer exactly on
                // its floor, where the 15% adoption margin is unreachable —
                // without this, a stale 10 000× from the first frames'
                // auto-fit span holds forever).
                let one_step_apart = matches!(
                    (snap_position(current), snap_position(strict_k)),
                    (Some(a), Some(b)) if a.abs_diff(b) <= 1
                );
                if !one_step_apart {
                    strict_k
                } else if strict_k > current {
                    if base_row_px * current as f32 >= min_row_px / LEVEL_HYSTERESIS {
                        current
                    } else {
                        strict_k
                    }
                } else if base_row_px * strict_k as f32 >= min_row_px * LEVEL_HYSTERESIS {
                    strict_k
                } else {
                    current
                }
            }
            (_, strict) => strict?,
        };
        self.k = Some(k);
        Some(k)
    }
}

/// What `candle_width` and the *achievable* row height allow. A thin base row
/// is not a refusal — the display grouping can merge up to [`GROUP_SNAP`]'s
/// largest multiple — so each level asks whether some multiple reaches its
/// row floor.
fn level_for(
    candle_width: f32,
    base_row_px: f32,
    profile_row_px: f32,
    detailed_min: f32,
) -> DetailLevel {
    let row_reachable = |min_row: f32| display_multiple(base_row_px, min_row).is_some();
    if candle_width >= detailed_min && row_reachable(DETAILED_MIN_ROW) {
        DetailLevel::Detailed
    } else if candle_width >= COMPACT_MIN_WIDTH && row_reachable(COMPACT_MIN_ROW) {
        DetailLevel::Compact
    } else if candle_width >= PROFILE_MIN_WIDTH && row_reachable(profile_row_px) {
        DetailLevel::Profile
    } else if candle_width >= MARKS_MIN_WIDTH {
        DetailLevel::Marks
    } else {
        DetailLevel::Off
    }
}

/// The smallest snap multiple whose rows reach `min_row_px`, or `None` when
/// even the coarsest is too thin (a chart zoomed so far out that one snap row
/// is still under the floor).
fn display_multiple(base_row_px: f32, min_row_px: f32) -> Option<i64> {
    GROUP_SNAP
        .into_iter()
        .find(|k| base_row_px * (*k as f32) >= min_row_px)
}

/// Fold a ladder onto rows `k` buckets tall. `k = 1` is the identity; the
/// merge is exact because display buckets are integer multiples of capture
/// buckets sharing the zero anchor.
fn regroup(fp: &BarFootprint, k: i64) -> BTreeMap<i64, FootprintLevel> {
    let mut rows: BTreeMap<i64, FootprintLevel> = BTreeMap::new();
    for (&bucket, level) in fp.levels() {
        let row = rows.entry(bucket.div_euclid(k)).or_default();
        row.buy = row.buy.saturating_add(level.buy);
        row.sell = row.sell.saturating_add(level.sell);
        row.trade_count += level.trade_count;
    }
    rows
}

/// Abbreviate a quantity for a fixed-width cell: `58.1k`, `1.2M`, `736`,
/// `0.523`. Three decimals below 1 (a 1-minute BTC row's delta usually
/// lives there), two up to 100, so a dense ladder's cells stay the same
/// visual weight.
fn fmt_qty(qty: Decimal) -> String {
    let value = qty.to_f64().unwrap_or(0.0);
    let magnitude = value.abs();
    // Suffix thresholds sit at the value that *rounds* to the next unit:
    // 999.96k would print "1000.0k" — seven glyphs where the cell budget
    // assumes five — so it rolls to "1.0M" instead.
    if magnitude >= 999_950.0 {
        format!("{:.1}M", value / 1_000_000.0)
    } else if magnitude >= 999.95 {
        format!("{:.1}k", value / 1_000.0)
    } else if magnitude >= 100.0 {
        format!("{value:.0}")
    } else if value == value.trunc() {
        // A whole number of contracts is written as one. "92.00" spends two
        // fifths of a cell on characters that carry nothing, and in a ladder
        // that width is not free — it is taken out of the font size every
        // other number is drawn at. Instruments that trade in fractions still
        // get their decimals below.
        format!("{value:.0}")
    } else if magnitude >= 1.0 {
        format!("{value:.2}")
    } else {
        format!("{value:.3}")
    }
}

/// A delta for display: a value that *rounds* to zero prints as an unsigned
/// `"0"` — "-0.00" reads as broken software, and the sign on nothing is a
/// wrong-side whisper. Returns `None` exactly when the row is balanced at
/// display resolution, so callers can also skip the winner color.
fn fmt_delta(delta: Decimal) -> Option<String> {
    let text = fmt_qty(delta);
    if text
        .trim_start_matches('-')
        .chars()
        .all(|c| c == '0' || c == '.')
    {
        return None;
    }
    Some(text)
}

/// A bar's whole-ladder delta: who won the bar. Saturating, like every
/// other quantity fold here — a corrupt feed must not panic the paint.
fn bar_delta(fp: &BarFootprint) -> Decimal {
    fp.levels()
        .values()
        .fold(Decimal::ZERO, |sum, cell| sum.saturating_add(cell.delta()))
}

/// One stacked zone spanning one or more adjacent bars, in display buckets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZoneMark {
    pub first_slot: usize,
    pub last_slot: usize,
    pub low_bucket: i64,
    pub high_bucket: i64,
    pub side: Side,
}

/// Coalesce per-bar stacked zones across adjacent bars: a zone continuing at
/// an overlapping price range in the next bar is one market fact, not two
/// marks. Then keep at most `cap`, tallest stacks first — and report whether
/// anything was dropped, because a silently thinned signal reads as absence.
fn coalesce_zones(mut zones: Vec<(usize, StackedZone)>, cap: usize) -> (Vec<ZoneMark>, bool) {
    zones.sort_by_key(|(slot, zone)| (*slot, zone.low_bucket, zone.side == Side::Sell));
    let mut marks: Vec<ZoneMark> = Vec::new();
    for (slot, zone) in zones {
        let merged = marks.iter_mut().find(|mark| {
            mark.side == zone.side
                && slot > mark.first_slot
                && slot <= mark.last_slot + 1
                && zone.low_bucket <= mark.high_bucket
                && zone.high_bucket >= mark.low_bucket
        });
        match merged {
            Some(mark) => {
                mark.last_slot = mark.last_slot.max(slot);
                mark.low_bucket = mark.low_bucket.min(zone.low_bucket);
                mark.high_bucket = mark.high_bucket.max(zone.high_bucket);
            }
            None => marks.push(ZoneMark {
                first_slot: slot,
                last_slot: slot,
                low_bucket: zone.low_bucket,
                high_bucket: zone.high_bucket,
                side: zone.side,
            }),
        }
    }
    let dropped = marks.len() > cap;
    if dropped {
        // Tallest stacks carry the most memory; ties resolve by place so the
        // pick is deterministic frame over frame.
        marks.sort_by_key(|mark| {
            (
                std::cmp::Reverse(mark.high_bucket - mark.low_bucket),
                mark.first_slot,
                mark.low_bucket,
            )
        });
        marks.truncate(cap);
        marks.sort_by_key(|mark| (mark.first_slot, mark.low_bucket));
    }
    (marks, dropped)
}

/// How many of the newest *closed* bars feed the adaptive imbalance floor.
const ADAPTIVE_FLOOR_BARS: usize = 50;

/// The adaptive imbalance quantity floor: the 60th percentile of per-row
/// total volume over the newest closed bars. One fixed number cannot serve
/// WIN contracts and BTC fractions at once (20 is right on one and absurd on
/// the other); a percentile of what is actually printing adapts to the
/// instrument and the regime. Closed bars only, independent of what is on
/// screen: a floor that moved with every live print or every pan would
/// rewrite the highlights of history while the trader reads them. The
/// config surface adds a manual override on top.
fn adaptive_min_qty<'a>(ladders: impl Iterator<Item = &'a BarFootprint>) -> Decimal {
    let mut volumes: Vec<f64> = ladders
        .flat_map(|fp| fp.levels().values())
        .map(|level| level.volume().to_f64().unwrap_or(0.0))
        .collect();
    if volumes.is_empty() {
        return Decimal::ZERO;
    }
    // Only the p60 is read, so partition around it instead of ordering the
    // whole vector: linear rather than n log n over up to a few thousand
    // rows. See `FootprintLod::adaptive_floor` for why this runs rarely.
    let index = (volumes.len().saturating_sub(1)) * 60 / 100;
    let (_, p60, _) = volumes.select_nth_unstable_by(index, f64::total_cmp);
    Decimal::from_f64(*p60).unwrap_or(Decimal::ZERO)
}

/// Everything one frame of the layer needs, borrowed from the pane's draw.
pub struct LayerFrame<'a> {
    pub painter: &'a egui::Painter,
    pub chart_rect: egui::Rect,
    pub scale: &'a PriceScale,
    /// Closed ladders, indexed by state-bar index (global slot minus the
    /// venue prefix — prefix candles have no tape and draw no footprint).
    pub footprints: &'a [BarFootprint],
    /// Global slot of state bar 0 (= the venue prefix length).
    pub first_state_slot: usize,
    /// Visible global slots `[start, end)`.
    pub visible: (usize, usize),
    /// The forming bar's ladder (already throttle-snapshotted) and its slot.
    pub partial: Option<&'a BarFootprint>,
    pub partial_slot: usize,
    pub x_center: &'a dyn Fn(usize) -> f32,
    pub half: f32,
    pub candle_width: f32,
    /// Whether this feed *infers* the aggressor side (MT5 tick rule, replays
    /// of it). A layer whose entire content is buyer-vs-seller carries the
    /// label itself; the status bar's note is not enough here.
    pub side_inferred: bool,
    /// Whether the depth map is on underneath. The plate covers it inside the
    /// bars, and a map with holes in it that nothing explains reads as a map
    /// that lost data.
    pub depth_visible: bool,
    /// Device pixels per egui point, for the one thing that must land on a
    /// whole device pixel. Carried rather than asked for per cell: reading it
    /// from the context takes an exclusive lock.
    pub pixels_per_point: f32,
    /// The signal tunables (ratio, min-qty override, stack length, POC and
    /// badge switches).
    pub config: &'a crate::footprint_config::FootprintConfig,
}

/// Paint the layer and its legend. `lod` is the pane's sticky level state.
pub fn draw_layer(frame: &LayerFrame<'_>, lod: &mut FootprintLod) {
    let group = frame
        .footprints
        .first()
        .or(frame.partial)
        .map_or(Decimal::ONE, |fp| fp.group());
    let group_f = group.to_f64().unwrap_or(0.01).max(f64::EPSILON);
    // From the scale's own f64 density — never y(0) - y(group), which is
    // f32 rounding noise at index-future prices (see PriceScale::px_per_price).
    let base_row_px = (frame.scale.px_per_price() * group_f) as f32;
    // The zoom this layer answers to, in the units its floors are written in:
    // the candle's own width, stretched by the trader's `detail_scale` (a
    // scale below one asks for detail at narrower candles, which is the same
    // statement as lowering every floor by it).
    let scaled_width = if frame.config.detail_scale > 0.0 {
        frame.candle_width / frame.config.detail_scale
    } else {
        frame.candle_width
    };
    // `auto` is answered before anything is measured: it is a question about
    // the zoom, and every floor below is measured against a concrete style.
    // The chain is walked richest-first and the first link the candle can pay
    // for wins, so one wheel walks three columns → two → a shape.
    let requested = frame
        .config
        .style
        .resolve_auto(|style| scaled_width >= detailed_min_width(style, frame.config));
    let level = lod.resolve(
        scaled_width,
        base_row_px,
        frame.config.profile_row_px,
        detailed_min_width(requested, frame.config),
    );
    // A style that cannot pay for itself at this zoom hands over to the one it
    // names, rather than drawing a worse version of itself. The legend says
    // both names — a chart that quietly became a different chart is the same
    // defect as a layer that is on and invisible.
    let style = match requested.fallback() {
        Some(fallback) if level < DetailLevel::Detailed => fallback,
        _ => requested,
    };
    // Published for the next frame's candle layout, which has to run before
    // this one paints.
    lod.drawn_style = Some(style);
    // QUANTICK_FOOTPRINT_DEBUG=1 appends the level inputs to the legend —
    // the boundary bugs so far were all states the eye could not explain
    // from the outside (wedged k, stale group), and the chart telling its
    // own numbers beats a screenshot guessing game.
    let debug = {
        static DEBUG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        (*DEBUG.get_or_init(|| {
            std::env::var("QUANTICK_FOOTPRINT_DEBUG").is_ok_and(|value| value == "1")
        }))
        .then(|| {
            format!(
                " · [w{:.0} row{:.3} g{} lvl{:?} n{}]",
                frame.candle_width,
                base_row_px,
                group,
                level,
                frame.footprints.len(),
            )
        })
    };

    if level == DetailLevel::Off {
        // Nothing to compute and nothing to draw — but the legend still
        // explains the silence, or an enabled layer reads as broken.
        draw_legend(
            frame,
            level,
            style,
            group,
            1,
            false,
            false,
            false,
            Decimal::ZERO,
            debug,
        );
        return;
    }

    let min_row = match level {
        DetailLevel::Detailed => DETAILED_MIN_ROW,
        DetailLevel::Compact => COMPACT_MIN_ROW,
        // The configured band fineness: the boss's "more, thinner rows".
        _ => frame.config.profile_row_px,
    };
    let k = lod
        .resolve_multiple(base_row_px, min_row)
        .unwrap_or(GROUP_SNAP[GROUP_SNAP.len() - 1]);
    let row_group_f = group_f * k as f64;

    // The ladders on screen, each beside its global slot.
    let (start, end) = frame.visible;
    let visible_ladders = || {
        (start.max(frame.first_state_slot)..end)
            .filter_map(|slot| {
                let fp = frame.footprints.get(slot - frame.first_state_slot)?;
                Some((slot, fp))
            })
            .chain(
                frame
                    .partial
                    .filter(|_| frame.partial_slot >= start && frame.partial_slot < end)
                    .map(|fp| (frame.partial_slot, fp)),
            )
    };

    let min_qty = match frame.config.imbalance_min_qty {
        Some(pinned) => pinned,
        None => lod.adaptive_floor(frame.footprints.len(), group, || {
            adaptive_min_qty(frame.footprints.iter().rev().take(ADAPTIVE_FLOOR_BARS))
        }),
    };
    let ratio = frame.config.imbalance_ratio;
    let mut cells_left = CELL_BUDGET;
    let mut aggregated_any = false;
    let mut zones: Vec<(usize, StackedZone)> = Vec::new();

    // Two passes over the visible ladders, and the split is not an
    // optimisation: a zone's wash has to land *under* the cells, not over
    // them. Painted last, it tinted the digits along with their background —
    // a row that was both POC and inside a zone read at ~3.9:1. The regrouped
    // rows are carried between the passes rather than folded twice, so the
    // second pass costs nothing but the walk.
    let mut regrouped: Vec<(usize, BTreeMap<i64, FootprintLevel>)> =
        Vec::with_capacity(end.saturating_sub(start));
    if level >= DetailLevel::Marks {
        for (slot, fp) in visible_ladders() {
            if fp.is_aggregated() {
                // A cap-coarsened ladder lives on a doubled grid; drawing it
                // with the frame's row geometry would put its rows at the
                // wrong prices. Hiding it and saying so is the honest v1
                // (the cap only trips on pathological bars).
                aggregated_any = true;
                continue;
            }
            // Zones and the POC are computed on the display rows the eye
            // compares — the same rows the cells draw.
            let rows = regroup(fp, k);
            for zone in zones_of(&rows, ratio, min_qty, frame.config.stacked_count) {
                zones.push((slot, zone));
            }
            regrouped.push((slot, rows));
        }
    }

    let (marks, zones_dropped) = coalesce_zones(zones, MAX_ZONE_MARKS);
    for mark in &marks {
        draw_zone_mark(frame, mark, row_group_f);
    }

    // The heat ramp's cuts: percentiles of the distribution the visible ladders
    // hold, on the display grid they are drawn on. Read from the maps the pass
    // above already built rather than folding them a second time — the cache
    // key moves with the visible window, so during a drag every frame is a
    // miss, and a second fold there was a full regroup of every visible bar at
    // frame rate.
    let heat = if style == crate::footprint_config::FootprintStyle::Cluster {
        lod.heat_scale((start, end), frame.footprints.len(), k, || {
            heat_scale(regrouped.iter().map(|(_, rows)| rows))
        })
    } else {
        None
    };

    let paint = BarPaint {
        frame,
        level,
        style,
        row_group: row_group_f,
        ratio,
        min_qty,
        heat,
    };
    for (slot, rows) in &regrouped {
        if level >= DetailLevel::Profile && cells_left > 0 {
            draw_bar(&paint, rows, (frame.x_center)(*slot), &mut cells_left);
        } else if frame.config.show_poc
            && let Some(poc) = poc_of(rows)
        {
            draw_poc_dot(frame, (frame.x_center)(*slot), poc, row_group_f);
        }
    }

    // The per-bar delta totals strip at the chart's bottom — the reference
    // charts' footer chips: one signed, side-colored number per bar saying
    // who won it overall. From Compact up: at Profile widths the chips
    // would overlap into noise. `bar_delta` is the tested fold.
    //
    // Every style, not just the split. Who won the bar is a reading of the
    // bar, orthogonal to how its rows are drawn; withholding it from the
    // ladder made that style strictly poorer than its sibling rather than a
    // different way of seeing the same thing.
    if level >= DetailLevel::Compact && frame.config.show_delta_totals {
        for (slot, fp) in visible_ladders() {
            let delta = bar_delta(fp);
            let Some(text) = fmt_delta(delta) else {
                continue;
            };
            let side = if delta > Decimal::ZERO {
                Side::Buy
            } else {
                Side::Sell
            };
            let galley = frame.painter.layout_no_wrap(
                text,
                egui::FontId::monospace(10.0),
                egui::Color32::WHITE,
            );
            let center = egui::pos2(
                (frame.x_center)(slot),
                frame.chart_rect.bottom() - TOTALS_STRIP_OFFSET_Y,
            );
            let rect = egui::Rect::from_center_size(center, galley.size() + egui::vec2(6.0, 3.0));
            frame.painter.rect_filled(
                rect,
                egui::Rounding::same(2.0),
                theme::side_color(side).gamma_multiply(0.8),
            );
            frame.painter.galley(
                rect.min + egui::vec2(3.0, 1.5),
                galley,
                egui::Color32::WHITE,
            );
        }
    }

    draw_legend(
        frame,
        level,
        style,
        group,
        k,
        aggregated_any,
        cells_left == 0,
        zones_dropped,
        min_qty,
        debug,
    );
}

/// POC of already-regrouped rows: highest volume, ties to the lowest row —
/// the engine's own rule, restated on display rows.
fn poc_of(rows: &BTreeMap<i64, FootprintLevel>) -> Option<i64> {
    let mut best: Option<(i64, Decimal)> = None;
    for (&row, level) in rows {
        let volume = level.volume();
        match best {
            Some((_, best_volume)) if volume <= best_volume => {}
            _ => best = Some((row, volume)),
        }
    }
    best.map(|(row, _)| row)
}

/// Diagonal stacked zones on display rows: same rule the engine applies to
/// capture buckets, run over the rows the eye actually compares.
fn zones_of(
    rows: &BTreeMap<i64, FootprintLevel>,
    ratio: Decimal,
    min_qty: Decimal,
    min_run: usize,
) -> Vec<StackedZone> {
    let side_qty = |row: i64, side: Side| -> Decimal {
        rows.get(&row)
            .map(|level| match side {
                Side::Buy => level.buy,
                Side::Sell => level.sell,
            })
            .unwrap_or(Decimal::ZERO)
    };
    let dominates = |qty: Decimal, other: Decimal| -> bool {
        qty >= ratio.saturating_mul(other) && qty.saturating_sub(other) >= min_qty
    };
    let mut zones = Vec::new();
    for side in [Side::Buy, Side::Sell] {
        let buckets: Vec<i64> = rows
            .iter()
            .filter(|&(&row, level)| match side {
                Side::Buy => dominates(level.buy, side_qty(row - 1, Side::Sell)),
                Side::Sell => dominates(level.sell, side_qty(row + 1, Side::Buy)),
            })
            .map(|(&row, _)| row)
            .collect();
        let mut run_start = 0usize;
        for i in 0..buckets.len() {
            let run_breaks = i + 1 == buckets.len() || buckets[i + 1] != buckets[i] + 1;
            if run_breaks {
                if i + 1 - run_start >= min_run.max(1) {
                    zones.push(StackedZone {
                        low_bucket: buckets[run_start],
                        high_bucket: buckets[i],
                        side,
                    });
                }
                run_start = i + 1;
            }
        }
    }
    zones
}

/// Inset of the cluster's columns from its box, and the gutter between them.
const CLUSTER_BOX_PAD_PX: f32 = 2.0;

const CLUSTER_GUTTER_PX: f32 = 3.0;

/// WCAG contrast ratio between two opaque colours. Used by the tests that pin
/// every number the layer draws against the background it is drawn on.
///
/// The luminance under it comes from `theme`, the module that owns colour:
/// one spelling of that formula is what keeps this guard and `theme::ink_on`
/// grading on the same scale.
#[cfg(test)]
fn contrast_ratio(a: egui::Color32, b: egui::Color32) -> f32 {
    let (high, low) = {
        let (x, y) = (theme::relative_luminance(a), theme::relative_luminance(b));
        (x.max(y), x.min(y))
    };
    (high + 0.05) / (low + 0.05)
}

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

#[allow(clippy::too_many_arguments)]
fn draw_legend(
    frame: &LayerFrame<'_>,
    level: DetailLevel,
    style: crate::footprint_config::FootprintStyle,
    group: Decimal,
    k: i64,
    aggregated_any: bool,
    capped: bool,
    zones_dropped: bool,
    min_qty: Decimal,
    debug: Option<String>,
) {
    let mut text = String::from("footprint");
    // A style that handed over says so, naming both: the trader asked for one
    // reading and is looking at another, and a chart that quietly became a
    // different chart is the same defect as a layer that is on and invisible.
    if style != frame.config.style {
        text.push_str(" · ");
        text.push_str(frame.config.style.id());
        text.push_str(" → ");
        text.push_str(style.id());
    }
    match level {
        DetailLevel::Off => text.push_str(" · zoom in for detail"),
        DetailLevel::Marks => text.push_str(" · marks"),
        DetailLevel::Profile => text.push_str(" · profile"),
        DetailLevel::Compact => text.push_str(" · delta"),
        // The legend names what the columns actually are — "sell|buy" over
        // a delta ladder would misread every number (data honesty).
        DetailLevel::Detailed => {
            text.push_str(" · ");
            text.push_str(style.detailed_legend());
        }
    }
    // How much further to zoom, in the only unit the gesture has. "zoom in for
    // numbers" with no number is why this layer read as slow to arrive: a
    // trader could not tell a nudge from a different chart entirely.
    if level < DetailLevel::Compact && frame.candle_width > 0.0 {
        let further = COMPACT_MIN_WIDTH * frame.config.detail_scale / frame.candle_width;
        if further > 1.05 {
            text.push_str(&format!(" · numbers at {further:.1}× this zoom"));
        }
    }
    // The effective grouping is always spoken: the number a row stands for
    // must never change meaning silently (data honesty).
    let effective = group.saturating_mul(Decimal::from(k));
    text.push_str(&format!(" · rows {effective}"));
    // The imbalance floor in force, spoken like the rows are: a highlight
    // whose threshold is secret reads as arbitrary.
    if level > DetailLevel::Off && !min_qty.is_zero() {
        text.push_str(&format!(" · min qty {}", fmt_qty(min_qty)));
    }
    // What the cell colours mean. A six-step scale with no key is a chart
    // asking to be guessed at: bright could be "a lot" or "imbalanced", and
    // the two lead to opposite trades. Same rule as the rows and the floor —
    // a mark whose meaning is secret reads as arbitrary.
    if style == crate::footprint_config::FootprintStyle::Cluster && level >= DetailLevel::Detailed {
        text.push_str(" · heat: cell volume vs the screen");
    }
    if aggregated_any {
        text.push_str(" · coarsened bars hidden");
    }
    if frame.side_inferred {
        text.push_str(" · side inferred");
    }
    // The plate is opaque by design — that is what gives the digits a floor
    // they control — so where the map used to show through, it no longer
    // does. Said out loud for the same reason the effective row size is: a
    // trader reading the liquidity map must never wonder whether the gaps are
    // the market or the chart. And only where a plate is actually painted:
    // claiming occlusion that is not there undercuts the same guarantee from
    // the other side.
    if frame.depth_visible && style.plate() == StylePlate::Casing && level >= DetailLevel::Profile {
        text.push_str(" · map hidden behind the bars");
    }
    if capped {
        text.push_str(" · capped");
    }
    if zones_dropped {
        text.push_str(&format!(" · strongest {MAX_ZONE_MARKS} zones"));
    }
    if let Some(debug) = debug {
        text.push_str(&debug);
    }
    // Bottom-left: the top-left is the bubbles legend's home, and that panel
    // paints an opaque background *after* this layer — a legend carrying the
    // rows' meaning must not live under someone else's paint.
    frame.painter.text(
        egui::pos2(
            frame.chart_rect.left() + 6.0,
            frame.chart_rect.bottom() - 6.0,
        ),
        egui::Align2::LEFT_BOTTOM,
        text,
        egui::FontId::proportional(10.5),
        theme::TEXT_MUTED,
    );
}

crate::hooks::declare_hooks!["QUANTICK_FOOTPRINT_DEBUG"];

#[cfg(test)]
mod tests {
    use super::bar::*;
    use super::heat::*;
    use super::*;
    use quantick_engine::{DEFAULT_LEVEL_CAP, FootprintBuilder, Trade};
    use std::str::FromStr as _;

    fn dec(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    /// The zoom ceiling must reach the Detailed budget, or the most detailed
    /// level exists only in the code — the reason MAX_CANDLE_WIDTH rose.
    /// A cross-constant guard on purpose: it fires when either constant
    /// drifts under the other.
    #[test]
    fn every_style_is_reachable_at_some_zoom_and_every_detail_scale() {
        use crate::footprint_config::{DETAIL_SCALE_RANGE, FootprintConfig, FootprintStyle};
        // The zoom the trader can actually reach is the ceiling divided by the
        // detail scale, and the scale goes to the top of its own range. A
        // style whose floor is above that is a style the registry offers and
        // the chart can never draw — the legend would say `cluster → bidask`
        // for ever, and the slider that caused it is in another window.
        let reachable = crate::viewport::MAX_CANDLE_WIDTH / DETAIL_SCALE_RANGE.end();
        for style in FootprintStyle::ALL {
            for config in [
                FootprintConfig::default(),
                FootprintConfig {
                    cluster_show_total: false,
                    ..FootprintConfig::default()
                },
            ] {
                let floor = detailed_min_width(style, &config);
                assert!(
                    reachable >= floor,
                    "{}: floor {floor:.1} px is past the {reachable:.1} px the zoom can reach",
                    style.id()
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
            level_for(100.0, 12.0, PROFILE_MIN_ROW, ladder_detailed_min_width()),
            DetailLevel::Detailed
        );
        // Wide candle, hairline base rows: grouping x100 still reaches 12px.
        assert_eq!(
            level_for(100.0, 0.2, PROFILE_MIN_ROW, ladder_detailed_min_width()),
            DetailLevel::Detailed
        );
        // Wide candle, sub-hairline rows: the extended snap ladder rescues
        // detail far deeper than 100× (an index future on the 0.01 fallback
        // grid), so only truly hopeless rows drop to profile, then marks.
        assert_eq!(
            level_for(100.0, 0.05, PROFILE_MIN_ROW, ladder_detailed_min_width()),
            DetailLevel::Detailed
        );
        assert_eq!(
            level_for(100.0, 0.0006, PROFILE_MIN_ROW, ladder_detailed_min_width()),
            DetailLevel::Profile
        );
        assert_eq!(
            level_for(100.0, 0.0003, PROFILE_MIN_ROW, ladder_detailed_min_width()),
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
                level_for(width, 12.0, PROFILE_MIN_ROW, ladder_detailed_min_width()),
                expected,
                "at {width} px per candle"
            );
        }
    }

    /// The text floors are a typography budget and must be re-derived, never
    /// nudged: a floor under what its own text measures draws digits across
    /// the neighbouring candle, which is worse than making the trader zoom.
    /// This is that derivation, run against whatever the constants say today.
    /// Compose a translucent colour over an opaque one, the way the painter
    /// does — so a test measures the pixel the trader sees, not the token.
    fn over(fg: egui::Color32, bg: egui::Color32) -> egui::Color32 {
        let alpha = f32::from(fg.a()) / 255.0;
        // `Color32` is premultiplied, so the source term is already scaled.
        let channel = |f: u8, b: u8| -> u8 {
            (f32::from(f) + f32::from(b) * (1.0 - alpha))
                .round()
                .min(255.0) as u8
        };
        egui::Color32::from_rgb(
            channel(fg.r(), bg.r()),
            channel(fg.g(), bg.g()),
            channel(fg.b(), bg.b()),
        )
    }

    /// The floor WCAG calls readable for body text. Every number this layer
    /// draws is measured against it — no exemptions, because a number a trader
    /// cannot read is a number that is not there.
    const AA: f32 = 4.5;

    /// The plate is what makes the ladder's contrast a constant. Proven
    /// against every background the layer can actually sit on: the canvas, a
    /// candle body at each of the four appearance presets, and the depth map's
    /// brightest bands.
    ///
    /// Two of those presets used to fail — `Glass` at 4.07:1 and `Classic` at
    /// 2.18:1 — because the ladder had no plate and inherited whatever the
    /// trader's taste in candles left behind. That is the defect this test
    /// exists to keep out.
    #[test]
    fn the_ladder_plate_makes_every_background_readable() {
        let candle_at = |fill: f32, side: egui::Color32| -> egui::Color32 {
            over(side.gamma_multiply(fill), theme::CANVAS)
        };
        let backgrounds = [
            ("canvas", theme::CANVAS),
            ("orderflow buy", candle_at(0.20, theme::BUY)),
            ("orderflow sell", candle_at(0.20, theme::SELL)),
            ("glass buy", candle_at(0.35, theme::BUY)),
            ("glass sell", candle_at(0.35, theme::SELL)),
            ("classic buy", candle_at(1.0, theme::BUY)),
            ("classic sell", candle_at(1.0, theme::SELL)),
            ("heat cyan", egui::Color32::from_rgb(0x00, 0xC2, 0xC4)),
            ("heat amber", egui::Color32::from_rgb(0xFA, 0x9E, 0x2C)),
            ("heat peak", egui::Color32::from_rgb(0xFF, 0xFA, 0xE8)),
        ];
        for (name, background) in backgrounds {
            let plate = over(theme::CASING, background);
            for (ink_name, ink) in [
                ("ordinary", theme::TEXT_PRIMARY),
                ("buy", theme::ink(Side::Buy)),
                ("sell", theme::ink(Side::Sell)),
            ] {
                let ratio = contrast_ratio(ink, plate);
                assert!(
                    ratio >= AA,
                    "{ink_name} ink over the plate on {name}: {ratio:.2}:1"
                );
            }
            // And on an imbalanced row, where the cell sinks below the plate.
            for side in [Side::Buy, Side::Sell] {
                let cell = over(
                    theme::side_color(side).gamma_multiply(IMBALANCE_CELL_ALPHA),
                    plate,
                );
                let ratio = contrast_ratio(theme::ink(side), cell);
                assert!(
                    ratio >= AA,
                    "{side:?} ink over its imbalanced cell on {name}: {ratio:.2}:1"
                );
            }
        }
    }

    /// One closed ladder holding `quantities`, one per price level — the
    /// shape the heat scale reads, built through the engine rather than by
    /// hand so the test cannot drift from what the app actually folds.
    fn ladder_of(quantities: &[Decimal]) -> quantick_engine::BarFootprint {
        let mut builder = FootprintBuilder::new(Decimal::ONE, DEFAULT_LEVEL_CAP);
        for (i, qty) in quantities.iter().enumerate() {
            builder.push(&Trade {
                agg_id: i as u64,
                timestamp_ms: i as i64,
                price: Decimal::from(1_000 + i as i64),
                quantity: *qty,
                side: Side::Buy,
            });
        }
        builder.close().expect("a closed ladder")
    }

    /// The ramp spreads its steps over whatever distribution is on screen.
    ///
    /// This is the claim ranks buy over ratios, and it is worth an assertion
    /// because both failure modes shipped once. Measured on a capture, a fixed
    /// denominator put 47% of cells in the top step — the brightest colour on
    /// screen was also the most common, so nothing stood out against anything
    /// — and an earlier reference put nearly all of them on the floor.
    ///
    /// Per-cell volume is heavily skewed, so the test feeds a skewed
    /// distribution rather than a flat one: a ramp that only behaves on
    /// uniform data would prove nothing about a tape.
    #[test]
    fn the_heat_ramp_spreads_over_a_skewed_distribution() {
        // A long tail: most rows ordinary, a handful enormous.
        let quantities: Vec<Decimal> = (1..=400)
            .map(|i| {
                let skewed = (f64::from(i) / 400.0).powf(4.0) * 5_000.0 + 1.0;
                Decimal::from_f64(skewed).unwrap_or(Decimal::ONE)
            })
            .collect();
        let ladder = ladder_of(&quantities);
        let scale = heat_scale(std::iter::once(&regroup(&ladder, 1))).expect("a scale");

        let mut population = [0_usize; HEAT_STEP_COUNT];
        for qty in &quantities {
            population[heat_step(*qty, Some(scale))] += 1;
        }
        let total: usize = population.iter().sum();
        assert_eq!(total, quantities.len());

        // Every step is used, and none of them swallows the screen. The top
        // step is deliberately the rarest — it is the one that has to mean
        // something when it appears.
        for (step, count) in population.iter().enumerate() {
            assert!(*count > 0, "step {step} is unreachable: {population:?}");
            let share = 100 * count / total;
            assert!(
                share <= 55,
                "step {step} holds {share}% of the screen: {population:?}"
            );
        }
        assert!(
            population[HEAT_STEP_COUNT - 1] < population[0],
            "the brightest step must be rarer than the floor: {population:?}"
        );
    }

    /// The cuts rise, so a bigger quantity never lands on a colder colour.
    #[test]
    fn the_heat_scale_is_monotonic() {
        let quantities: Vec<Decimal> = (1..=200).map(Decimal::from).collect();
        let ladder = ladder_of(&quantities);
        let scale = heat_scale(std::iter::once(&regroup(&ladder, 1))).expect("a scale");
        for pair in scale.windows(2) {
            assert!(pair[1] >= pair[0], "cuts not ascending: {scale:?}");
        }
        let mut previous = 0;
        for qty in &quantities {
            let step = heat_step(*qty, Some(scale));
            assert!(
                step >= previous,
                "step fell at {qty}: {step} after {previous}"
            );
            previous = step;
        }
    }

    /// Nothing on screen means no scale, and no scale means the floor — never
    /// a colour key invented from an empty set.
    #[test]
    fn an_empty_screen_has_no_heat_scale() {
        assert!(heat_scale(std::iter::empty()).is_none());
        assert_eq!(heat_step(Decimal::from(1_000), None), 0);
    }

    /// Every step of the heat ramp is readable with the ink that step selects.
    ///
    /// This is the assertion the ramp was *designed backwards from*: the ink
    /// flip sits where it does because between the two inks lies a band of
    /// luminance neither can serve.
    #[test]
    fn every_heat_step_is_readable_with_the_ink_it_picks() {
        for step in 0..HEAT_STEP_COUNT {
            for side in [Side::Buy, Side::Sell] {
                let fill = heat_fill(side, step);
                let ratio = contrast_ratio(heat_ink(step), fill);
                assert!(
                    ratio >= AA,
                    "step {step} on {side:?} ({fill:?}) with its ink: {ratio:.2}:1"
                );
            }
        }
    }

    /// Both ink boundaries are forced, not chosen.
    ///
    /// The ink rule is three-valued — muted, primary, dark — and boundaries in
    /// a rule are only honest when something makes them fall where they do.
    /// Maximum contrast is deliberately *not* the goal: a quiet cell keeps a
    /// quiet number, so the ramp and the digits agree instead of arguing. What
    /// has to hold is that neither boundary could move without breaking AA,
    /// which is what makes them arithmetic rather than taste.
    #[test]
    fn both_ink_boundaries_are_forced_by_contrast() {
        for side in [Side::Buy, Side::Sell] {
            // Dark ink is unusable below its flip, and the only usable one at
            // and above it — so the boundary sits exactly where it must.
            for step in 0..HEAT_INK_FLIP_STEP {
                let fill = heat_fill(side, step);
                assert!(
                    contrast_ratio(theme::CHIP_INK, fill) < AA,
                    "step {step} on {side:?}: dark ink would already work, so the flip is late"
                );
            }
            for step in HEAT_INK_FLIP_STEP..HEAT_STEP_COUNT {
                let fill = heat_fill(side, step);
                assert!(
                    contrast_ratio(theme::TEXT_PRIMARY, fill) < AA,
                    "step {step} on {side:?}: light ink still works, so the flip is early"
                );
            }
            // And muted ink runs out exactly where the ramp stops using it:
            // that is why step 1 sits at L* 22 rather than anywhere brighter.
            let last_muted = heat_fill(side, HEAT_INK_MUTED_BELOW_STEP - 1);
            let first_primary = heat_fill(side, HEAT_INK_MUTED_BELOW_STEP);
            assert!(
                contrast_ratio(theme::TEXT_MUTED, last_muted) >= AA,
                "{side:?}: the last muted step is already unreadable"
            );
            assert!(
                contrast_ratio(theme::TEXT_MUTED, first_primary) < AA,
                "{side:?}: muted ink would still work one step further up"
            );
        }
    }

    /// The heat ramp never reaches the hues the app reserves.
    ///
    /// [`theme::AMBER`] means \"not live\" and [`theme::POC`] is a line inside
    /// these very candles. The ramp heats toward orange deliberately — that is
    /// where the reference charts get their warmth — and orange is on the far
    /// side of red, not next door to yellow. This pins the distance so a later
    /// \"make the top a bit warmer\" cannot quietly collide with either.
    #[test]
    fn the_heat_ramp_stays_clear_of_the_reserved_hues() {
        let hue = |color: egui::Color32| -> f32 {
            let (r, g, b) = (
                f32::from(color.r()) / 255.0,
                f32::from(color.g()) / 255.0,
                f32::from(color.b()) / 255.0,
            );
            let max = r.max(g).max(b);
            let min = r.min(g).min(b);
            let span = max - min;
            if span <= f32::EPSILON {
                return 0.0;
            }
            let h = if max == r {
                60.0 * (((g - b) / span) % 6.0)
            } else if max == g {
                60.0 * ((b - r) / span + 2.0)
            } else {
                60.0 * ((r - g) / span + 4.0)
            };
            (h + 360.0) % 360.0
        };
        let separation = |a: f32, b: f32| -> f32 {
            let d = (a - b).abs() % 360.0;
            d.min(360.0 - d)
        };
        const MIN_SEPARATION_DEG: f32 = 25.0;
        for reserved in [theme::AMBER, theme::POC] {
            let reserved_hue = hue(reserved);
            for step in 0..HEAT_STEP_COUNT {
                for side in [Side::Buy, Side::Sell] {
                    let fill = heat_fill(side, step);
                    let gap = separation(hue(fill), reserved_hue);
                    assert!(
                        gap >= MIN_SEPARATION_DEG,
                        "step {step} on {side:?} sits {gap:.0}° from a reserved hue"
                    );
                }
            }
        }
    }

    /// The cluster's total column carries a silhouette instead of a heat step,
    /// and that is what buys it one ink with no flip rule. The ceiling is
    /// load-bearing: past it the column re-enters the unreadable band.
    #[test]
    fn the_total_columns_silhouette_stays_under_its_ceiling() {
        let plate = over(theme::CASING, theme::CANVAS);
        let filled = over(
            PROFILE_COLOR.gamma_multiply(CLUSTER_TOTAL_SILHOUETTE_ALPHA),
            plate,
        );
        let ratio = contrast_ratio(theme::TEXT_PRIMARY, filled);
        assert!(
            ratio >= AA,
            "primary ink over the total column's silhouette: {ratio:.2}:1"
        );
        const {
            assert!(
                CLUSTER_TOTAL_SILHOUETTE_ALPHA <= 0.42,
                "past 0.42 the silhouette needs a flip rule of its own"
            );
        }
    }

    /// The heat ramp is ordered, and its two sides are isoluminant.
    ///
    /// Isoluminance is deliberate: under deuteranopia the two hues collapse,
    /// and what still separates bid from ask is the *position* of the column.
    /// That trade only holds while luminance carries the ordinal reading — so
    /// the ordering is the assertion, and the columns may never swap places.
    #[test]
    fn the_heat_ramp_is_ordered_and_isoluminant() {
        for side in [Side::Buy, Side::Sell] {
            for step in 1..HEAT_STEP_COUNT {
                let previous = theme::relative_luminance(heat_fill(side, step - 1));
                let current = theme::relative_luminance(heat_fill(side, step));
                assert!(
                    current > previous,
                    "{side:?} step {step} is not brighter than {}",
                    step - 1
                );
            }
        }
        for step in 0..HEAT_STEP_COUNT {
            let buy = theme::relative_luminance(heat_fill(Side::Buy, step));
            let sell = theme::relative_luminance(heat_fill(Side::Sell, step));
            assert!(
                (buy - sell).abs() < 0.02,
                "step {step}: buy L={buy:.3} vs sell L={sell:.3} — the sides must weigh the same"
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
        let last = at(crate::viewport::MIN_PX_PER_BAR);
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
        let mut previous = rank(at(crate::viewport::MIN_PX_PER_BAR));
        let mut width = crate::viewport::MIN_PX_PER_BAR;
        while width <= crate::viewport::MAX_CANDLE_WIDTH {
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
    /// retune — and `detail_scale` moves the whole ladder together without
    /// reordering it.
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
            let plain = level_for(width, 12.0, PROFILE_MIN_ROW, ladder_detailed_min_width());
            let scaled = level_for(
                width / tight,
                12.0,
                PROFILE_MIN_ROW,
                ladder_detailed_min_width(),
            );
            assert!(scaled >= plain, "at {width} px");
        }
    }

    /// Full body up to the Profile floor, outline-only by the Detailed
    /// floor, monotonic in between — and never outside [0, 1].
    #[test]
    fn candle_body_fade_spans_profile_to_detailed() {
        assert_eq!(candle_body_fade(8.0), 1.0);
        assert_eq!(candle_body_fade(PROFILE_MIN_WIDTH), 1.0);
        assert_eq!(candle_body_fade(ladder_detailed_min_width()), 0.0);
        assert_eq!(candle_body_fade(160.0), 0.0);
        let mid = candle_body_fade((PROFILE_MIN_WIDTH + ladder_detailed_min_width()) / 2.0);
        assert!(mid > 0.0 && mid < 1.0);
        assert!(candle_body_fade(30.0) > candle_body_fade(50.0));
    }

    #[test]
    fn lod_changes_only_past_the_dead_band_in_both_directions() {
        // Written as multiples of the floor rather than as pixels, so the dead
        // band is tested wherever the floor is tuned to.
        let floor = ladder_detailed_min_width();
        let mut lod = FootprintLod::default();
        // The first frame takes the strict answer.
        assert_eq!(
            lod.resolve(
                floor * 1.2,
                12.0,
                PROFILE_MIN_ROW,
                ladder_detailed_min_width()
            ),
            DetailLevel::Detailed
        );
        // Just under the floor: inside the 15% band, the level holds.
        assert_eq!(
            lod.resolve(
                floor * 0.95,
                12.0,
                PROFILE_MIN_ROW,
                ladder_detailed_min_width()
            ),
            DetailLevel::Detailed
        );
        // 15% past the floor: the downgrade happens.
        assert_eq!(
            lod.resolve(
                floor * 0.83,
                12.0,
                PROFILE_MIN_ROW,
                ladder_detailed_min_width()
            ),
            DetailLevel::Compact
        );
        // Upgrades need the same clearance: over the floor but not 15% over,
        // so the level holds — an instant upgrade against a banded downgrade
        // is a blinker at the boundary.
        assert_eq!(
            lod.resolve(
                floor * 1.1,
                12.0,
                PROFILE_MIN_ROW,
                ladder_detailed_min_width()
            ),
            DetailLevel::Compact
        );
        assert_eq!(
            lod.resolve(
                floor * 1.2,
                12.0,
                PROFILE_MIN_ROW,
                ladder_detailed_min_width()
            ),
            DetailLevel::Detailed
        );
        // The blinker scenario itself: oscillating across the floor by a
        // hair must not change the level once settled.
        for width in [floor * 1.02, floor * 0.98, floor * 1.02, floor * 0.98] {
            assert_eq!(
                lod.resolve(width, 12.0, PROFILE_MIN_ROW, ladder_detailed_min_width()),
                DetailLevel::Detailed,
                "width {width} blinked"
            );
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
            lod.resolve(100.0, 0.0001, PROFILE_MIN_ROW, ladder_detailed_min_width()),
            DetailLevel::Marks
        );
        // ...then the real span arrives: two steps away, no band, snap.
        assert_eq!(
            lod.resolve(100.0, 12.0, PROFILE_MIN_ROW, ladder_detailed_min_width()),
            DetailLevel::Detailed
        );

        // Same for the row multiple: a 10 000x from a wild span must not
        // hold once the strict answer is orders of magnitude finer.
        let mut lod = FootprintLod::default();
        assert_eq!(lod.resolve_multiple(0.0005, 4.0), Some(10_000));
        assert_eq!(lod.resolve_multiple(0.09, 4.0), Some(50));
    }

    #[test]
    fn display_multiple_snaps_to_round_row_groups() {
        assert_eq!(display_multiple(12.0, 11.0), Some(1));
        assert_eq!(display_multiple(6.0, 11.0), Some(2));
        assert_eq!(display_multiple(1.0, 11.0), Some(20));
        assert_eq!(display_multiple(0.1, 11.0), Some(200));
        assert_eq!(display_multiple(0.1, 4.0), Some(50));
        // The fallback-grid regression: a 0.01 capture grid on an index
        // future leaves base rows at ~0.026 px — the ladder must still
        // reach a drawable row instead of locking the level at Marks.
        assert_eq!(display_multiple(0.026, 4.0), Some(200));
        assert_eq!(display_multiple(0.026, 12.0), Some(500));
        assert_eq!(display_multiple(0.0001, 12.0), None);
    }

    #[test]
    fn regrouping_by_integer_multiples_is_exact() {
        let mut builder = FootprintBuilder::new(dec("0.5"), DEFAULT_LEVEL_CAP);
        for (i, price) in ["100.0", "100.5", "101.0", "101.5"].iter().enumerate() {
            builder.push(&Trade {
                agg_id: i as u64,
                timestamp_ms: i as i64,
                price: dec(price),
                quantity: dec("1"),
                side: Side::Buy,
            });
        }
        let fp = builder.close().unwrap();
        let rows = regroup(&fp, 2);
        // Buckets 200..=203 halve into rows 100 and 101, two units each.
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[&100].buy, dec("2"));
        assert_eq!(rows[&101].buy, dec("2"));
        // k = 1 is the identity.
        assert_eq!(&regroup(&fp, 1), fp.levels());
    }

    #[test]
    fn quantities_abbreviate_into_fixed_weight_cells() {
        assert_eq!(fmt_qty(dec("58100")), "58.1k");
        assert_eq!(fmt_qty(dec("1230000")), "1.2M");
        assert_eq!(fmt_qty(dec("736")), "736");
        assert_eq!(fmt_qty(dec("0.5234")), "0.523");
        assert_eq!(fmt_qty(dec("12.345")), "12.35");
        assert_eq!(fmt_qty(dec("-1500")), "-1.5k");
        // A value that rounds past its suffix rolls to the next one: never
        // "1000.0k" — seven glyphs where the cell budget assumes five.
        assert_eq!(fmt_qty(dec("999960")), "1.0M");
        assert_eq!(fmt_qty(dec("999.96")), "1.0k");
    }

    /// A delta that rounds to zero at display resolution has no sign and no
    /// text at all: "-0.00" reads as broken software, and the minus on
    /// nothing is a wrong-side whisper (panel must-fix).
    #[test]
    fn display_zero_deltas_are_never_signed() {
        assert_eq!(fmt_delta(dec("-0.0004")), None);
        assert_eq!(fmt_delta(dec("0.0003")), None);
        assert_eq!(fmt_delta(dec("0")), None);
        assert_eq!(fmt_delta(dec("-0.43")).as_deref(), Some("-0.430"));
        assert_eq!(fmt_delta(dec("58100")).as_deref(), Some("58.1k"));
    }

    #[test]
    fn adjacent_bars_at_one_price_coalesce_into_one_zone_mark() {
        let zone = |low: i64, high: i64| StackedZone {
            low_bucket: low,
            high_bucket: high,
            side: Side::Buy,
        };
        let (marks, dropped) = coalesce_zones(
            vec![
                (10, zone(100, 103)),
                (11, zone(101, 104)),
                (14, zone(100, 103)),
            ],
            MAX_ZONE_MARKS,
        );
        assert!(!dropped);
        assert_eq!(
            marks,
            vec![
                ZoneMark {
                    first_slot: 10,
                    last_slot: 11,
                    low_bucket: 100,
                    high_bucket: 104,
                    side: Side::Buy,
                },
                // Slot 14 does not touch slot 11: a separate market fact.
                ZoneMark {
                    first_slot: 14,
                    last_slot: 14,
                    low_bucket: 100,
                    high_bucket: 103,
                    side: Side::Buy,
                },
            ]
        );
    }

    #[test]
    fn the_zone_cap_keeps_the_tallest_stacks_and_says_it_dropped_some() {
        let zones: Vec<(usize, StackedZone)> = (0..40)
            .map(|i| {
                (
                    i * 2, // gaps, so nothing coalesces
                    StackedZone {
                        low_bucket: 1000 + i as i64 * 10,
                        high_bucket: 1000 + i as i64 * 10 + (i as i64 % 7),
                        side: Side::Sell,
                    },
                )
            })
            .collect();
        let (marks, dropped) = coalesce_zones(zones, 5);
        assert!(dropped);
        assert_eq!(marks.len(), 5);
        // Every survivor is at least as tall as the tallest loser would be.
        assert!(
            marks
                .iter()
                .all(|mark| mark.high_bucket - mark.low_bucket >= 5)
        );
    }

    #[test]
    fn the_adaptive_floor_reads_the_screens_own_percentile() {
        let mut builder = FootprintBuilder::new(dec("1"), DEFAULT_LEVEL_CAP);
        for (i, qty) in ["1", "2", "3", "4", "100"].iter().enumerate() {
            builder.push(&Trade {
                agg_id: i as u64,
                timestamp_ms: i as i64,
                price: Decimal::from(100 + i as i64),
                quantity: dec(qty),
                side: Side::Buy,
            });
        }
        let fp = builder.close().unwrap();
        let floor = adaptive_min_qty(std::iter::once(&fp));
        // Five levels, p60 lands on the third-smallest volume: one big
        // print does not drag the floor up to itself.
        assert_eq!(floor, dec("3"));
        assert_eq!(adaptive_min_qty(std::iter::empty()), Decimal::ZERO);
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

    /// The bar's delta is the sum of its rows', and a bar balanced at
    /// display resolution prints no chip at all.
    #[test]
    fn a_bars_delta_is_the_sum_of_its_rows() {
        let mut builder = FootprintBuilder::new(dec("1"), DEFAULT_LEVEL_CAP);
        for (i, (price, qty, side)) in [
            ("100", "3", Side::Buy),
            ("101", "1", Side::Sell),
            ("102", "0.5", Side::Buy),
        ]
        .into_iter()
        .enumerate()
        {
            builder.push(&Trade {
                agg_id: i as u64,
                timestamp_ms: i as i64,
                price: dec(price),
                quantity: dec(qty),
                side,
            });
        }
        assert_eq!(bar_delta(&builder.close().unwrap()), dec("2.5"));

        let mut builder = FootprintBuilder::new(dec("1"), DEFAULT_LEVEL_CAP);
        for (i, side) in [Side::Buy, Side::Sell].into_iter().enumerate() {
            builder.push(&Trade {
                agg_id: i as u64,
                timestamp_ms: i as i64,
                price: dec("100"),
                quantity: dec("2"),
                side,
            });
        }
        let flat = builder.close().unwrap();
        assert_eq!(bar_delta(&flat), Decimal::ZERO);
        assert_eq!(fmt_delta(bar_delta(&flat)), None, "no winner, no chip");
    }

    /// A layer that does not paint does not dress. This is the whole reason
    /// [`candle_dressing`] takes `paints` instead of reading the switch that
    /// turns the ladders on: a fixed-range volume profile turns those on
    /// while the layer stays hidden, and for a while that was what reached
    /// here — so dropping a profile on a chart moved every candle into the
    /// sidebar lane and faded its body to an outline, under nothing.
    #[test]
    fn a_hidden_footprint_leaves_the_candles_exactly_as_they_were() {
        let base = crate::style::ChartStyle::default().candles;
        for treatment in [CandleTreatment::Fade, CandleTreatment::Sidebar] {
            // Wide enough that `Fade` would really fade and `Sidebar` would
            // really claim its lane, so this cannot pass by accident.
            for candle_width in [4.0_f32, 40.0, 400.0] {
                assert_eq!(
                    candle_dressing(false, treatment, candle_width, base),
                    (0.0, None),
                    "{treatment:?} at {candle_width} px, layer hidden"
                );
            }
        }
    }

    /// And the other half of the same law: with the layer painting, the
    /// dressing is still there. A fix that simply stopped dressing candles
    /// would pass the test above and break the footprint.
    #[test]
    fn a_painting_footprint_still_dresses_the_candles() {
        let base = crate::style::ChartStyle::default().candles;
        let wide = ladder_detailed_min_width().max(PROFILE_MIN_WIDTH) + 1.0;

        let (lane, dressed) = candle_dressing(true, CandleTreatment::Sidebar, wide, base);
        assert!(lane > 0.0, "the sidebar candle keeps a lane of its own");
        assert_eq!(
            dressed.expect("a sidebar candle is restyled").fill_opacity,
            1.0,
            "beside the box the candle is solid again"
        );

        // The fade runs from Profile (untouched) to Detailed (outline only),
        // so it is halfway across that span that the body is really fading.
        let halfway = f32::midpoint(PROFILE_MIN_WIDTH, ladder_detailed_min_width());
        let (lane, dressed) = candle_dressing(true, CandleTreatment::Fade, halfway, base);
        assert_eq!(lane, 0.0, "a fading candle stays where it was");
        let faded = dressed.expect("the body fades once the zoom crosses Profile");
        assert!(faded.fill_opacity < base.fill_opacity);
        assert!(faded.outline_opacity >= base.outline_opacity);

        // At the Profile floor itself nothing has faded yet, and "nothing to
        // do" is reported as the chart's own style rather than a copy of it.
        assert_eq!(
            candle_dressing(true, CandleTreatment::Fade, PROFILE_MIN_WIDTH, base),
            (0.0, None),
            "the body is whole until the zoom crosses Profile"
        );
    }
}
