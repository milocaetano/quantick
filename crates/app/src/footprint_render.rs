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
use quantick_engine::{BarFootprint, Extreme, FootprintLevel, Side, StackedZone};
use rust_decimal::Decimal;
use rust_decimal::prelude::{FromPrimitive as _, ToPrimitive as _};

use crate::chart::PriceScale;
use crate::footprint_config::{FootprintStyle, StylePlate};
use crate::theme;

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
/// How far an imbalanced cell sinks *below* its plate.
///
/// Below, never above: a light pill under text of the cell's own hue raises
/// the floor exactly beneath the digits it means to emphasise. Measured, the
/// old 0.35 pill left its number at 3.2:1 — the layer's most important row as
/// its least legible one. Sinking the cell and lightening the ink puts the
/// same row at 8.6:1.
const IMBALANCE_CELL_ALPHA: f32 = 0.16;
/// Width of the solid edge on the dominant column's outer border, in pixels.
/// The side is carried by *which* border it is, so the colour is redundancy.
const IMBALANCE_EDGE_PX: f32 = 2.0;

/// The ladder's own Detailed floor. Kept as a named value because the
/// hysteresis tests and `candle_body_fade` reason about a single reference
/// width; every *style* asks [`detailed_min_width`] for its own.
fn ladder_detailed_min_width() -> f32 {
    detailed_min_width(FootprintStyle::Ladder)
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
fn detailed_min_width(style: FootprintStyle) -> f32 {
    let columns = style.detailed_quantity_columns();
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

/// The split style's volume-profile silhouette: neutral light, after the
/// reference charts' white/gray histograms — color stays reserved for the
/// fight (the delta side) and the POC.
const PROFILE_COLOR: egui::Color32 = egui::Color32::from_gray(0xD8);

/// The least an imbalance chip spans, in pixels: enough to ring the number
/// on a short bar without swallowing the whole half.
const MIN_CHIP_PX: f32 = 14.0;

/// Gap between an extreme-ratio badge and the row it describes, in pixels —
/// just off the bar's end, never on the ladder itself.
const EXTREME_BADGE_GAP_PX: f32 = 3.0;

/// How far above the chart's bottom edge the per-bar delta totals sit —
/// clear of the legend line below them.
const TOTALS_STRIP_OFFSET_Y: f32 = 22.0;

/// How much of the canvas the split style's per-bar backdrop keeps: enough
/// that the footprint owns its interior over the heatmap, little enough
/// that the map stays visible between candles.
const BACKDROP_ALPHA: f32 = 0.65;

/// That backdrop, derived from the theme rather than hand-premultiplied —
/// a canvas color copied by hand goes stale the day the theme moves, with
/// no test to notice.
fn canvas_backdrop() -> egui::Color32 {
    theme::CANVAS.gamma_multiply(BACKDROP_ALPHA)
}

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
    let requested = frame.config.style;
    let level = lod.resolve(
        scaled_width,
        base_row_px,
        frame.config.profile_row_px,
        detailed_min_width(requested),
    );
    // A style that cannot pay for itself at this zoom hands over to the one it
    // names, rather than drawing a worse version of itself. The legend says
    // both names — a chart that quietly became a different chart is the same
    // defect as a layer that is on and invisible.
    let style = match requested.fallback() {
        Some(fallback) if level < DetailLevel::Detailed => fallback,
        _ => requested,
    };
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
    // The heat ramp's denominator: a high percentile of per-cell volume over
    // the newest closed bars. Folded here rather than per bar so the ramp
    // compares bars against each other — which is the reading alternative bars
    // exist to give — and only for the one style that draws it.
    let heat = if style == crate::footprint_config::FootprintStyle::Cluster {
        lod.heat_scale((start, end), frame.footprints.len(), k, || {
            heat_scale(visible_ladders().map(|(_, fp)| regroup(fp, k)))
        })
    } else {
        None
    };

    let mut cells_left = CELL_BUDGET;
    let mut aggregated_any = false;
    let mut zones: Vec<(usize, StackedZone)> = Vec::new();

    // Two passes over the visible ladders, and the split is not an
    // optimisation: a zone's wash has to land *under* the cells, not over
    // them. Painted last, it tinted the digits along with their background —
    // a row that was both POC and inside a zone read at ~3.9:1. The regrouped
    // rows are carried between the passes rather than folded twice, so the
    // second pass costs nothing but the walk.
    let mut regrouped: Vec<(usize, BTreeMap<i64, FootprintLevel>)> = Vec::new();
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

    for (slot, rows) in &regrouped {
        if level >= DetailLevel::Profile && cells_left > 0 {
            draw_bar(
                frame,
                level,
                style,
                rows,
                row_group_f,
                (frame.x_center)(*slot),
                ratio,
                min_qty,
                heat,
                &mut cells_left,
            );
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

/// Pixel band of display row `row` (rows are `row_group` of price tall).
///
/// Ordered on screen, not by price: on an inverted scale the row's high edge
/// is the *lower* pixel, and a band handed out as `(high_edge, low_edge)`
/// would give every rect a negative height.
fn row_band(frame: &LayerFrame<'_>, row: i64, row_group: f64) -> (f32, f32) {
    let low = row as f64 * row_group;
    frame.scale.band(low, low + row_group)
}

/// Where one display row lands on screen, and what the signals say about it.
///
/// Bundled because the per-style row painters all need the same seven facts
/// and none of them need anything else: passing the bundle keeps a new style
/// from reaching back into `draw_bar`'s locals, which is how the four
/// style conditions grew in the first place.
struct RowGeometry {
    row: i64,
    top: f32,
    bottom: f32,
    row_height: f32,
    is_poc: bool,
    buy_imbalance: bool,
    sell_imbalance: bool,
}

#[allow(clippy::too_many_arguments)]
fn draw_bar(
    frame: &LayerFrame<'_>,
    level: DetailLevel,
    style: crate::footprint_config::FootprintStyle,
    rows: &BTreeMap<i64, FootprintLevel>,
    row_group: f64,
    xc: f32,
    ratio: Decimal,
    min_qty: Decimal,
    heat: Option<HeatScale>,
    cells_left: &mut usize,
) {
    let painter = frame.painter;
    let poc = frame.config.show_poc.then(|| poc_of(rows)).flatten();
    let max_volume = rows
        .values()
        .map(|level| level.volume().to_f64().unwrap_or(0.0))
        .fold(0.0_f64, f64::max)
        .max(f64::EPSILON);
    let max_abs_delta = rows
        .values()
        .map(|level| level.delta().to_f64().unwrap_or(0.0).abs())
        .fold(0.0_f64, f64::max)
        .max(f64::EPSILON);
    // `bidask` mirrors two bars against one shared scale, and the scale has to
    // be the larger *side*, never the row total: halving the total would make
    // a one-sided row look like a balanced one at full width.
    let max_side_volume = rows
        .values()
        .map(|level| {
            level
                .buy
                .to_f64()
                .unwrap_or(0.0)
                .max(level.sell.to_f64().unwrap_or(0.0))
        })
        .fold(0.0_f64, f64::max)
        .max(f64::EPSILON);
    let dominates = |qty: Decimal, other: Decimal| -> bool {
        qty >= ratio.saturating_mul(other) && qty.saturating_sub(other) >= min_qty
    };
    let neighbour = |row: i64, side: Side| -> Decimal {
        rows.get(&row)
            .map(|l| match side {
                Side::Buy => l.buy,
                Side::Sell => l.sell,
            })
            .unwrap_or(Decimal::ZERO)
    };

    // The plate: what a style paints under its own content so the content has
    // a floor it controls.
    //
    // The floor matters exactly as much as the content is *digits*. A bar's
    // length reads the same over any background, so the shape styles ask for
    // the light backdrop and leave the map visible; a number does not degrade
    // gracefully, so the digit styles ask for the full casing. Until this was
    // a style's own answer, the ladder had no plate at all, and its contrast
    // floor was whatever the candle preset, the canvas switch and the bucket
    // arithmetic happened to leave behind — 2.2:1 on the `Classic` preset,
    // 4.1:1 on `Glass`.
    let plate = match style.plate() {
        StylePlate::Backdrop => canvas_backdrop(),
        StylePlate::Casing => theme::CASING,
    };
    if level >= DetailLevel::Profile
        && let (Some(&first), Some(&last)) = (rows.keys().next(), rows.keys().next_back())
    {
        // Composed from both rows' screen bands, not from the price names:
        // upside down the highest bucket renders lowest, and reading "top"
        // off it would hand the rect a negative height.
        let (first_top, first_bottom) = row_band(frame, first, row_group);
        let (last_top, last_bottom) = row_band(frame, last, row_group);
        let bar_top = first_top.min(last_top);
        let bar_bottom = first_bottom.max(last_bottom);
        let inset = style.candle_treatment().content_inset();
        let reach = (frame.half - 1.0).max(1.0);
        // One plate per bar, never one per cell: thirty rects instead of one,
        // and — worse — a hairline seam of whatever is behind between every
        // pair of rows.
        let box_rect = egui::Rect::from_min_max(
            egui::pos2(xc - reach + inset, bar_top),
            egui::pos2(xc + reach, bar_bottom),
        );
        painter.rect_filled(box_rect, egui::Rounding::same(2.0), plate);
        if inset > 0.0 {
            // A frame, so the box reads as one object rather than a dark
            // patch — the reference chart's boxed ladder. Border, never a
            // competitor: 2.3:1 against the casing.
            painter.rect_stroke(
                box_rect,
                egui::Rounding::same(2.0),
                egui::Stroke::new(1.0_f32, theme::BORDER),
            );
        }
        // A hairline spine at the central axis keeps the bar's midline
        // readable after the candle body fades to outline — the reference
        // charts' thin gray candle spine. On the ladder it does more: it is
        // the ruler between the two number columns, without which `123 456`
        // reads as one number, so it is drawn firmer there.
        let spine = match style {
            crate::footprint_config::FootprintStyle::Ladder => 0.55,
            _ => 0.3,
        };
        if inset == 0.0 {
            painter.line_segment(
                [egui::pos2(xc, bar_top), egui::pos2(xc, bar_bottom)],
                egui::Stroke::new(1.0_f32, theme::TEXT_FAINT.gamma_multiply(spine)),
            );
        }
    }

    for (&row, cell) in rows {
        if *cells_left == 0 {
            return;
        }
        let (top, bottom) = row_band(frame, row, row_group);
        if bottom < frame.chart_rect.top() || top > frame.chart_rect.bottom() {
            // Off-screen rows cost no budget: the cap exists to bound what
            // is *painted*, and spending it on invisible rows would starve
            // the visible bars of a tall ladder.
            continue;
        }
        *cells_left -= 1;
        let row_height = (bottom - top).max(1.0);
        let is_poc = poc == Some(row);
        let buy_imbalance = dominates(cell.buy, neighbour(row - 1, Side::Sell));
        let sell_imbalance = dominates(cell.sell, neighbour(row + 1, Side::Buy));

        // Styles that own their whole row paint it here and return; the two
        // that share the LOD ladder's generic cell fall through to the match
        // below. `draws_own_rows` is what decides, so a style added to the
        // registry declares which half it belongs to instead of being written
        // into a condition here.
        if style.draws_own_rows() && level >= DetailLevel::Profile {
            let geometry = RowGeometry {
                row,
                top,
                bottom,
                row_height,
                is_poc,
                buy_imbalance,
                sell_imbalance,
            };
            match style {
                crate::footprint_config::FootprintStyle::BidAsk => {
                    draw_bidask_row(frame, cell, &geometry, xc, max_side_volume, row_group);
                    continue;
                }
                crate::footprint_config::FootprintStyle::Cluster => {
                    draw_cluster_row(
                        frame, level, cell, &geometry, xc, heat, max_volume, row_group,
                    );
                    continue;
                }
                _ => {}
            }
            // The reference look, inside the candle: a central axis at the
            // candle's middle, the total-volume profile growing rightward in
            // neutral light (the exocharts silhouette), and a delta bar per
            // row growing leftward in the winner's color — "who won the
            // fight" readable at a glance, the volume shape behind it.
            let volume_frac = (cell.volume().to_f64().unwrap_or(0.0) / max_volume) as f32;
            let delta = cell.delta();
            let delta_frac = (delta.to_f64().unwrap_or(0.0).abs() / max_abs_delta) as f32;
            let reach = (frame.half - 1.0).max(1.0);
            let delta_text = fmt_delta(delta);
            // A row balanced at display resolution has no winner: neutral
            // sliver, no color, no number — a teal bar on a tie is a
            // wrong-side read (panel must-fix).
            let winner = delta_text.as_ref().map(|_| {
                if delta > Decimal::ZERO {
                    Side::Buy
                } else {
                    Side::Sell
                }
            });
            // The imbalance chip is colored by the side that IS imbalanced,
            // never by the delta winner: a sell-imbalanced row boxed in teal
            // because its own delta leans positive is the one thing a
            // footprint must never say (panel must-fix). Both sides at once
            // is contested — no chip rather than a coin flip.
            let chip_side = match (buy_imbalance, sell_imbalance) {
                (true, false) => Some(Side::Buy),
                (false, true) => Some(Side::Sell),
                _ => None,
            };
            // Right: the profile. The POC row is the brightest thing in the
            // silhouette even before its yellow line lands on it.
            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(xc, top + 0.5),
                    egui::pos2(xc + reach * volume_frac, bottom - 0.5),
                ),
                egui::Rounding::ZERO,
                PROFILE_COLOR.gamma_multiply(if is_poc { 0.95 } else { 0.60 }),
            );
            // Left: the fight.
            let (left_from, left_color) = match winner {
                Some(side) => (
                    xc - reach * delta_frac.max(0.04),
                    theme::side_color(side).gamma_multiply(if chip_side.is_some() {
                        0.55
                    } else {
                        0.5
                    }),
                ),
                None => (xc - 2.0, theme::TEXT_FAINT.gamma_multiply(0.35)),
            };
            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(left_from, top + 0.5),
                    egui::pos2(xc, bottom - 0.5),
                ),
                egui::Rounding::ZERO,
                left_color,
            );
            if let Some(side) = chip_side
                && row_height >= 4.0
            {
                // The chip hugs its own row's content — bar or number — and
                // is inset vertically so neighbouring chips never fuse into
                // one shouting rectangle (panel must-fix); the full-width
                // multi-row box stays reserved for the stacked-zone mark.
                let chip_reach = (reach * delta_frac).max(MIN_CHIP_PX).min(reach);
                painter.rect_stroke(
                    egui::Rect::from_min_max(
                        egui::pos2(xc - chip_reach - 2.0, top + 1.5),
                        egui::pos2(xc - 1.0, bottom - 1.5),
                    ),
                    egui::Rounding::ZERO,
                    egui::Stroke::new(1.0_f32, theme::side_color(side)),
                );
            }
            // Deep zoom: the delta number over the left half, side-colored
            // so the column scans without reading (panel must-fix), width-
            // clamped so it never bleeds out.
            if level == DetailLevel::Detailed
                && frame.config.show_numbers
                && let Some(text) = delta_text
            {
                let width_budget = (frame.half - 6.0) / 3.0;
                let font = egui::FontId::monospace(
                    (row_height - 2.0)
                        .min(width_budget)
                        .clamp(LADDER_MIN_FONT_PX, 13.0),
                );
                painter.text(
                    egui::pos2(xc - 3.0, (top + bottom) / 2.0),
                    egui::Align2::RIGHT_CENTER,
                    text,
                    font,
                    winner.map_or(theme::TEXT_MUTED, theme::side_color),
                );
            }
            if is_poc {
                // Right half only in the split style: a full-width line
                // would strike through the delta digits it shares the row
                // with; the profile half alone carries it unambiguously.
                draw_poc_line(frame, xc, xc + frame.half, row, row_group);
            }
            continue;
        }

        match level {
            DetailLevel::Profile => {
                // Textless histogram, anchored on the candle's left edge and
                // colored by the row's delta sign — neutral by design, so the
                // POC and the zone marks stay the only saturated things.
                let frac = (cell.volume().to_f64().unwrap_or(0.0) / max_volume) as f32;
                let width = (2.0 * frame.half - 1.0).max(1.0) * frac;
                let color = if cell.delta() >= Decimal::ZERO {
                    theme::BUY
                } else {
                    theme::SELL
                };
                painter.rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(xc - frame.half, top + 0.5),
                        egui::pos2(xc - frame.half + width, bottom - 0.5),
                    ),
                    egui::Rounding::ZERO,
                    color.gamma_multiply(if is_poc { 0.55 } else { 0.28 }),
                );
                if is_poc {
                    draw_poc_dot(frame, xc, row, row_group);
                }
            }
            DetailLevel::Compact | DetailLevel::Detailed => {
                // The font answers to the row height AND the cell width: a
                // five-glyph quantity ("58.1k") is ~3 em of monospace per
                // side, and a number that outgrows its candle bleeds into
                // the neighbour — worse than a smaller number.
                let width_budget = match level {
                    DetailLevel::Detailed => (frame.half - 6.0) / 3.0,
                    _ => (2.0 * frame.half - 6.0) / 3.0,
                };
                let font = egui::FontId::monospace(
                    (row_height - 2.0)
                        .min(width_budget)
                        .clamp(LADDER_MIN_FONT_PX, 13.0),
                );
                if is_poc {
                    // A ring around the row, not a wash under it. The old
                    // tint cost the row its contrast (6.8:1 → 4.4:1 — below
                    // AA on the one row the trader reads first) to say
                    // something an outline says louder, and the full-width
                    // line that came with it struck straight through both
                    // number columns. The split style already refuses that
                    // line for exactly this reason; the ladder had never been
                    // told. The ring also gives the POC a *shape*: the only
                    // framed row in the bar, readable without relying on hue.
                    painter.rect_stroke(
                        egui::Rect::from_min_max(
                            egui::pos2(xc - frame.half + 0.5, top + 0.5),
                            egui::pos2(xc + frame.half - 0.5, bottom - 0.5),
                        ),
                        egui::Rounding::ZERO,
                        egui::Stroke::new(1.5_f32, theme::POC),
                    );
                }
                let mid = (top + bottom) / 2.0;
                if level == DetailLevel::Compact {
                    if !frame.config.show_numbers {
                        continue;
                    }
                    let delta = cell.delta();
                    let side = if delta >= Decimal::ZERO {
                        Side::Buy
                    } else {
                        Side::Sell
                    };
                    painter.text(
                        egui::pos2(xc, mid),
                        egui::Align2::CENTER_CENTER,
                        fmt_qty(delta),
                        font,
                        theme::ink(side),
                    );
                } else {
                    // sell | buy, the tape's own left-to-right: taker-sells
                    // hit the bid printed on the left, taker-buys lift the
                    // ask on the right.
                    for (side, qty, imbalanced, align, x) in [
                        (
                            Side::Sell,
                            cell.sell,
                            sell_imbalance,
                            egui::Align2::RIGHT_CENTER,
                            xc - CENTER_GUTTER_PX,
                        ),
                        (
                            Side::Buy,
                            cell.buy,
                            buy_imbalance,
                            egui::Align2::LEFT_CENTER,
                            xc + CENTER_GUTTER_PX,
                        ),
                    ] {
                        if imbalanced {
                            let cell_rect = match side {
                                Side::Sell => egui::Rect::from_min_max(
                                    egui::pos2(xc - frame.half, top + 0.5),
                                    egui::pos2(xc - 1.0, bottom - 0.5),
                                ),
                                Side::Buy => egui::Rect::from_min_max(
                                    egui::pos2(xc + 1.0, top + 0.5),
                                    egui::pos2(xc + frame.half, bottom - 0.5),
                                ),
                            };
                            // The cell goes *deeper* than the plate, not
                            // brighter. A light pill under text of its own hue
                            // is arithmetically a trap — it raises the floor
                            // exactly beneath the digits it means to
                            // emphasise, and the row carrying the layer's most
                            // important signal ends up its least legible
                            // (3.2:1). Darker cell plus lighter ink inverts
                            // that: 8.6:1 on the same row.
                            painter.rect_filled(
                                cell_rect,
                                egui::Rounding::same(2.0),
                                theme::side_color(side).gamma_multiply(IMBALANCE_CELL_ALPHA),
                            );
                            // And an edge on the *outer* border of the column
                            // that dominated — sell on the body's left, buy on
                            // its right. Position carries the side on its own,
                            // so the colour is redundancy rather than the
                            // channel, and the mark survives colour blindness.
                            let edge = match side {
                                Side::Sell => egui::Rect::from_min_max(
                                    egui::pos2(cell_rect.left(), cell_rect.top()),
                                    egui::pos2(
                                        cell_rect.left() + IMBALANCE_EDGE_PX,
                                        cell_rect.bottom(),
                                    ),
                                ),
                                Side::Buy => egui::Rect::from_min_max(
                                    egui::pos2(
                                        cell_rect.right() - IMBALANCE_EDGE_PX,
                                        cell_rect.top(),
                                    ),
                                    egui::pos2(cell_rect.right(), cell_rect.bottom()),
                                ),
                            };
                            painter.rect_filled(
                                edge,
                                egui::Rounding::ZERO,
                                theme::side_color(side),
                            );
                        }
                        if !frame.config.show_numbers {
                            // Numbers off leaves the imbalance cells above:
                            // the shape of the fight without the digits.
                            continue;
                        }
                        // Over the plate, the ordinary number can afford the
                        // primary ink (14.2:1 instead of the muted grey's
                        // 6.8:1 on canvas — and the muted grey's 2.2:1 on a
                        // `Classic` candle body, which is the reading this
                        // plate exists to end).
                        let color = if imbalanced {
                            theme::ink(side)
                        } else {
                            theme::TEXT_PRIMARY
                        };
                        painter.text(egui::pos2(x, mid), align, fmt_qty(qty), font.clone(), color);
                    }
                }
            }
            DetailLevel::Marks | DetailLevel::Off => {}
        }
    }

    // The extreme ratio badges, Detailed only: the aggression ratio printed
    // beside the bar's low and high — the reference chart's "9.82 at the
    // low". One-sided extremes have no finite ratio and draw nothing rather
    // than an invented stand-in.
    // The badge describes the row the trader can *see*: the ratio is
    // computed on the display rows, not the finer capture grid — a "9.8x"
    // beside a merged row must be that row's own number. The "x" suffix
    // keeps it out of the price vocabulary.
    if level == DetailLevel::Detailed && frame.config.extreme_ratio_badge {
        for extreme in [Extreme::Low, Extreme::High] {
            let cell = match extreme {
                Extreme::Low => rows.iter().next(),
                Extreme::High => rows.iter().next_back(),
            };
            let Some((&row, cell)) = cell else { continue };
            let (dominant, other) = if cell.buy >= cell.sell {
                (cell.buy, cell.sell)
            } else {
                (cell.sell, cell.buy)
            };
            if other.is_zero() {
                continue;
            }
            let Some(ratio_value) = dominant.checked_div(other) else {
                continue;
            };
            // Below the threshold a badge is anti-signal ("1.0x" = nothing
            // happened); survivors get a chip anchored in the dominant
            // side's color, so the exhaustion cue registers as a marker
            // instead of a stray number (panel should-fix).
            if ratio_value < frame.config.badge_min_ratio {
                continue;
            }
            let dominant_side = if cell.buy >= cell.sell {
                Side::Buy
            } else {
                Side::Sell
            };
            // The badge sits *outside* the bar's extent — which end of the row
            // that is on screen follows the scale's orientation, so the chip
            // never lands on the ladder it is describing.
            let (band_top, band_bottom) = row_band(frame, row, row_group);
            let outward_down = match extreme {
                Extreme::Low => !frame.scale.is_inverted(),
                Extreme::High => frame.scale.is_inverted(),
            };
            let (align, y) = if outward_down {
                (egui::Align2::CENTER_TOP, band_bottom + EXTREME_BADGE_GAP_PX)
            } else {
                (egui::Align2::CENTER_BOTTOM, band_top - EXTREME_BADGE_GAP_PX)
            };
            let text = format!("{:.1}x", ratio_value.to_f64().unwrap_or(0.0));
            let galley =
                painter.layout_no_wrap(text, egui::FontId::monospace(10.0), theme::TEXT_PRIMARY);
            let anchor = align.anchor_size(egui::pos2(xc, y), galley.size());
            painter.rect_filled(
                anchor.expand(2.0),
                egui::Rounding::same(2.0),
                canvas_backdrop(),
            );
            painter.rect_stroke(
                anchor.expand(2.0),
                egui::Rounding::same(2.0),
                egui::Stroke::new(1.0_f32, theme::side_color(dominant_side)),
            );
            painter.galley(anchor.min, galley, theme::TEXT_PRIMARY);
        }
    }
}

/// The POC line over `[x_from, x_to]`, with a background under-stroke so it
/// registers against the candle outline it crosses at every level.
fn draw_poc_line(frame: &LayerFrame<'_>, x_from: f32, x_to: f32, row: i64, row_group: f64) {
    let (top, bottom) = row_band(frame, row, row_group);
    let y = (top + bottom) / 2.0;
    frame.painter.line_segment(
        [egui::pos2(x_from, y), egui::pos2(x_to, y)],
        egui::Stroke::new(3.5_f32, canvas_backdrop()),
    );
    frame.painter.line_segment(
        [egui::pos2(x_from, y), egui::pos2(x_to, y)],
        egui::Stroke::new(1.5_f32, theme::POC),
    );
}

/// How many steps the heat ramp has.
///
/// Quantised, never a gradient. Three reasons, in order: rounding a float into
/// a colour every frame is how a pixel moves between two identical frames; the
/// depth map already owns the "continuous gradient" channel on this same
/// screen; and steps can be counted, which a gradient cannot.
const HEAT_STEP_COUNT: usize = 6;
/// The cuts and the colours describe the same ramp from two sides, and only
/// agree by construction: every cut opens a step, and the floor below the
/// first cut is free. Adding a colour without a cut leaves one unreachable.
const _: () = assert!(
    HEAT_PERCENTILES.len() + 1 == HEAT_STEP_COUNT,
    "the heat ramp needs exactly one more colour than it has cuts"
);

/// The heat ramp, one colour per step, darkest first.
///
/// **Derived, then written down.** Each entry was resolved in CIELCh — a
/// chosen lightness, a chosen hue, and the most chroma that lightness and hue
/// can hold inside sRGB — and pasted here as a literal. Three things follow
/// from doing it that way rather than mixing toward black and white at paint
/// time:
///
/// - **Chroma is an input.** A linear mix toward a neutral is a mix with grey,
///   so it launders the colour out at both ends: the old top sell step had
///   thrown away 68% of its base hue's chroma, and that vividness is exactly
///   what the reference charts get their heat from. Here the top step carries
///   more chroma than the token it came from.
/// - **The hue is free to travel.** Heat reads as a drift toward orange, and a
///   mix toward white cannot drift. The sell ramp walks 24° to 52°, ending on
///   an orange still 41° away from [`crate::theme::AMBER`], which stays
///   reserved for provenance. Yellow is never reached: at this lightness it
///   would land on AMBER's own hue.
/// - **No arithmetic at paint time.** No binary search, no float compared per
///   cell, and the ramp is bit-exact for ever — a search in `f32` can walk an
///   8-bit level the day anything upstream of it moves.
///
/// Lightness is the ladder the ink rule reads, so it is the one axis chosen
/// rather than maximised: 16, 22, 32, 40, 56, 78 in L*. Step 1 sits at 22
/// because [`theme::TEXT_MUTED`] stops clearing 4.5:1 above L* 23, and the two
/// quiet steps keep the muted ink.
///
/// The top step takes nearly all the chroma its lightness allows, and that is
/// deliberate rather than incidental: a first pass at this table held it back
/// and landed on C* 22 — within half a unit of the washed-out colour the whole
/// rewrite was meant to replace. The defect had survived its own fix, at the
/// one step that carries the heat. Lightness is untouched by the correction,
/// so every ink ratio is identical.
const HEAT_SELL: [egui::Color32; HEAT_STEP_COUNT] = [
    egui::Color32::from_rgb(0x51, 0x0E, 0x16),
    egui::Color32::from_rgb(0x6A, 0x11, 0x1A),
    egui::Color32::from_rgb(0x95, 0x15, 0x20),
    egui::Color32::from_rgb(0xBD, 0x12, 0x17),
    egui::Color32::from_rgb(0xFE, 0x38, 0x00),
    egui::Color32::from_rgb(0xFD, 0xAF, 0x89),
];
/// See [`HEAT_SELL`]. The buy ramp walks 191° to 170°, away from the sell hue
/// at every step so the two can never converge.
const HEAT_BUY: [egui::Color32; HEAT_STEP_COUNT] = [
    egui::Color32::from_rgb(0x0A, 0x2D, 0x2B),
    egui::Color32::from_rgb(0x0D, 0x3B, 0x38),
    egui::Color32::from_rgb(0x11, 0x55, 0x4E),
    egui::Color32::from_rgb(0x0D, 0x6A, 0x5F),
    egui::Color32::from_rgb(0x00, 0x98, 0x80),
    egui::Color32::from_rgb(0x29, 0xD9, 0xAE),
];

/// The step at and above which the ink turns dark. See [`HEAT_LUMINANCE`].
const HEAT_INK_FLIP_STEP: usize = 4;
/// Below this step the ink is muted rather than primary.
///
/// The ramp builds a hierarchy and a two-valued ink erases half of it: a cell
/// on the floor and a cell three steps up read with the same weight of text,
/// so the eye has to decode the background to know which one matters. Letting
/// the quiet cells keep quiet numbers means the digits agree with the colour
/// instead of arguing with it.
const HEAT_INK_MUTED_BELOW_STEP: usize = 2;

/// How far the cluster's grey silhouette is allowed to lighten its column.
///
/// Hard ceiling: past ~0.42 the silhouette pushes the column into the
/// forbidden luminance band and the total column would need a flip rule of its
/// own. Staying under it is what buys the column a single ink.
const CLUSTER_TOTAL_SILHOUETTE_ALPHA: f32 = 0.35;
/// Inset of the cluster's columns from its box, and the gutter between them.
const CLUSTER_BOX_PAD_PX: f32 = 2.0;
const CLUSTER_GUTTER_PX: f32 = 3.0;
/// Alpha of a `bidask` bar. Low enough that a POC line crosses it readably,
/// high enough that the two sides separate from the plate at a glance.
const BIDASK_BAR_ALPHA: f32 = 0.62;

/// The bevel's two faces.
///
/// Complementary by construction, and the asymmetry is the design rather than
/// a compromise: on a dark cell the white edge does all the work and the
/// shadow has nowhere to go, on a light cell the reverse. Measured in L*, the
/// highlight buys +19 on the floor step and +0.4 on the top one; the shadow
/// buys +5 on the floor and +29 on the top. So only the face that can be seen
/// is drawn — painting both always meant painting one for nothing.
///
/// Raising the alpha does not rescue the losing face: even at full opacity,
/// white over the top step is worth +17 L* — the cell simply turns white.
/// Which is why this is a choice of face, not a choice of number.
const BEVEL_HIGHLIGHT: egui::Color32 = egui::Color32::from_rgba_premultiplied(56, 56, 56, 56);
const BEVEL_SHADOW: egui::Color32 = egui::Color32::from_rgba_premultiplied(0, 0, 0, 120);
/// The step at and above which the shadow carries the relief instead of the
/// highlight. Shares the ink flip's boundary because both answer the same
/// question: is this cell light or dark.
const BEVEL_SHADOW_FROM_STEP: usize = HEAT_INK_FLIP_STEP;
/// Thickness of a bevel face, in pixels.
///
/// Two, not one. A one-pixel rect lands between two device pixels wherever the
/// row band falls on a fraction — which is always, the band being a
/// price-to-y projection — and antialiasing then spreads it until nothing is
/// left: measured, the top step's highlight arrived at +0.26 L* against a
/// theoretical +2.98, well under a just-noticeable difference of ~2.3.
const BEVEL_PX: f32 = 2.0;
/// Under this cell width the bevel is more edge than cell.
const BEVEL_MIN_CELL_PX: f32 = 16.0;

const BEVEL_MIN_ROW_PX: f32 = 10.0;

/// The heat ramp's scale: where each step's boundary falls, in quantity, for
/// the ladders currently on screen.
///
/// **Ranks, not ratios.** Dividing a cell by a fixed reference sounds right
/// and is not: per-cell volume is heavily skewed and the shape of that skew
/// changes with the market, so one denominator paints every cell on the floor
/// in a quiet stretch and saturates half of them in a busy one. Measured on a
/// real capture, ratio-to-p95 put **47% of cells in the top step** — the
/// brightest colour on screen was also the most common one, which leaves
/// nothing for it to stand out against.
///
/// Cutting the visible distribution at fixed *percentiles* fixes both ends by
/// construction: the busiest cells are always the top step and the quiet ones
/// always the floor, whatever the regime. The cuts are uneven on purpose —
/// most rows are ordinary, so the ramp spends its bright steps on the tail
/// that is worth seeing.
///
/// Visible ladders, not the newest N of the series: the denominator has to
/// describe what the trader is looking at. Reading the series instead made
/// the colours depend on where the replay's live edge happened to be.
const HEAT_PERCENTILES: [usize; 5] = [45, 68, 83, 93, 98];

/// One step's lower bound in quantity, ascending.  when there is
/// nothing on screen to measure — a ramp with an invented scale is a colour
/// key that means whatever it likes.
type HeatScale = [f64; 5];

fn heat_scale(rows: impl Iterator<Item = BTreeMap<i64, FootprintLevel>>) -> Option<HeatScale> {
    let mut sides: Vec<f64> = rows
        .flat_map(|rows| rows.into_values().collect::<Vec<_>>())
        .flat_map(|level| {
            [
                level.buy.to_f64().unwrap_or(0.0),
                level.sell.to_f64().unwrap_or(0.0),
            ]
        })
        .filter(|volume| *volume > 0.0)
        .collect();
    // Display rows, not capture buckets. Cutting the raw grid and colouring
    // the merged one is a scale for a different chart: a drawn cell is the sum
    // of `k` buckets, so the same cut lands at a different place in the
    // distribution at every zoom and every instrument tick.
    if sides.is_empty() {
        return None;
    }
    sides.sort_by(f64::total_cmp);
    let last = sides.len() - 1;
    Some(HEAT_PERCENTILES.map(|pct| sides[last * pct / 100]))
}

/// Which heat step a quantity falls in. `reference` absent (no closed bars
/// yet) puts everything on the floor rather than inventing a scale.
fn heat_step(qty: Decimal, scale: Option<HeatScale>) -> usize {
    let Some(scale) = scale else { return 0 };
    let value = qty.to_f64().unwrap_or(0.0);
    // One past the last boundary it clears: below every cut is the floor.
    scale.iter().filter(|cut| value >= **cut).count()
}

/// The fill for a step: a table lookup, and deliberately nothing more.
fn heat_fill(side: Side, step: usize) -> egui::Color32 {
    let ramp = match side {
        Side::Buy => HEAT_BUY,
        Side::Sell => HEAT_SELL,
    };
    ramp[step.min(ramp.len() - 1)]
}

/// The ink for a step. A function of the *step*, never of the colour: no
/// luminance arithmetic at paint time, no float compared per frame.
fn heat_ink(step: usize) -> egui::Color32 {
    if step >= HEAT_INK_FLIP_STEP {
        theme::CHIP_INK
    } else if step < HEAT_INK_MUTED_BELOW_STEP {
        theme::TEXT_MUTED
    } else {
        theme::TEXT_PRIMARY
    }
}

/// WCAG relative luminance of an opaque colour. Test-only now that the
/// ramp is a table: what it guards is the contract, not the construction.
#[cfg(test)]
fn relative_luminance(color: egui::Color32) -> f32 {
    let linear = |channel: u8| -> f32 {
        let value = f32::from(channel) / 255.0;
        if value <= 0.03928 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(color.r()) + 0.7152 * linear(color.g()) + 0.0722 * linear(color.b())
}

/// WCAG contrast ratio between two opaque colours. Used by the tests that pin
/// every number the layer draws against the background it is drawn on.
#[cfg(test)]
fn contrast_ratio(a: egui::Color32, b: egui::Color32) -> f32 {
    let (high, low) = {
        let (x, y) = (relative_luminance(a), relative_luminance(b));
        (x.max(y), x.min(y))
    };
    (high + 0.05) / (low + 0.05)
}

/// A light top edge and a dark bottom edge — the cheap bevel.
///
/// Two rects rather than four: the left and right faces are the least
/// informative of the four in a field of cells that already touch sideways,
/// and they cost 57% more. Rects rather than strokes, because a stroke goes
/// through the tessellator's feathering, and feathering is exactly what blurs
/// a one-pixel edge into nothing.
fn paint_bevel(painter: &egui::Painter, rect: egui::Rect, row_height: f32, step: usize) {
    if row_height < BEVEL_MIN_ROW_PX || rect.width() < BEVEL_MIN_CELL_PX {
        return;
    }
    // Snapped to whole *device* pixels before anything is drawn. The rect
    // arrives on a fraction — the row band is a price-to-y projection — and a
    // bevel is the one thing that cannot survive being antialiased across two
    // rows, because the edge carrying the relief is exactly as wide as the
    // blur would be.
    //
    // Device pixels, not egui points: at 125% or 150% display scaling a whole
    // point is 1.25 or 1.5 physical pixels, so rounding before the scale is
    // applied lands the edge back on a fraction — which is the very thing this
    // is here to avoid, and it would only show on the machines that scale.
    let scale = painter.ctx().pixels_per_point().max(f32::EPSILON);
    let snap = |v: f32| (v * scale).round() / scale;
    let rect = egui::Rect::from_min_max(
        egui::pos2(snap(rect.left()), snap(rect.top())),
        egui::pos2(snap(rect.right()), snap(rect.bottom())),
    );
    let (face, lit_top) = if step >= BEVEL_SHADOW_FROM_STEP {
        (BEVEL_SHADOW, false)
    } else {
        (BEVEL_HIGHLIGHT, true)
    };
    // Two edges meeting at a corner, not two opposite bars: relief is read at
    // the corner, and a top and a bottom with no sides read as a rule.
    let (horizontal, vertical) = if lit_top {
        (
            egui::Rect::from_min_max(rect.min, egui::pos2(rect.right(), rect.top() + BEVEL_PX)),
            egui::Rect::from_min_max(rect.min, egui::pos2(rect.left() + BEVEL_PX, rect.bottom())),
        )
    } else {
        (
            egui::Rect::from_min_max(egui::pos2(rect.left(), rect.bottom() - BEVEL_PX), rect.max),
            egui::Rect::from_min_max(egui::pos2(rect.right() - BEVEL_PX, rect.top()), rect.max),
        )
    };
    painter.rect_filled(horizontal, egui::Rounding::ZERO, face);
    painter.rect_filled(vertical, egui::Rounding::ZERO, face);
}

/// How wide one cluster column is, given the body width it shares.
///
/// The painter and the floor read the *same* function. They used to compute it
/// apart, which is how the floor came to declare a width legible that the
/// painter then drew three overlapping numbers into.
fn cluster_column_px_from(body_width: f32, columns: f32) -> f32 {
    let inner = body_width
        - crate::footprint_config::CANDLE_LANE_PX
        - 2.0 * CLUSTER_BOX_PAD_PX
        - (columns - 1.0) * CLUSTER_GUTTER_PX;
    (inner / columns).max(1.0)
}

/// The same, from a candle width rather than a body width — what the floor's
/// own test asks.
#[cfg(test)]
fn cluster_column_px(candle_width: f32) -> f32 {
    cluster_column_px_from(
        candle_width * TYPICAL_BODY_FRAC,
        FootprintStyle::Cluster.detailed_quantity_columns(),
    )
}

/// How many number columns the cluster draws: three with the total, two
/// without. The knob exists because the third column costs ~33 px of candle
/// width, and a trader who would rather see more bars than one more number
/// should not have to leave the style to get them.
fn cluster_columns(config: &crate::footprint_config::FootprintConfig) -> f32 {
    if config.cluster_show_total { 3.0 } else { 2.0 }
}

/// One row of the `bidask` style: both sides at their real size, mirrored
/// around the bar's axis on one shared scale.
///
/// The split answers "who won, and how much traded"; this answers "how big was
/// each side" — a question the split's single delta bar cannot, because 400×380
/// and 40×20 share a delta and are not the same market. Two mirrored lengths
/// on one scale make that difference the first thing the eye gets, with no
/// digit involved, which is why this style survives down to Profile where the
/// number styles cannot go.
fn draw_bidask_row(
    frame: &LayerFrame<'_>,
    cell: &FootprintLevel,
    geometry: &RowGeometry,
    xc: f32,
    max_side_volume: f64,
    row_group: f64,
) {
    let painter = frame.painter;
    let reach = (frame.half - 1.0).max(1.0);
    let top = geometry.top + 0.5;
    let bottom = geometry.bottom - 0.5;
    for (side, qty, imbalanced) in [
        (Side::Sell, cell.sell, geometry.sell_imbalance),
        (Side::Buy, cell.buy, geometry.buy_imbalance),
    ] {
        let frac = (qty.to_f64().unwrap_or(0.0) / max_side_volume) as f32;
        let span = reach * frac.clamp(0.0, 1.0);
        if span <= 0.0 {
            continue;
        }
        // Sell grows left, buy grows right: the tape's own left-to-right, the
        // same one the ladder's columns keep.
        let bar = match side {
            Side::Sell => {
                egui::Rect::from_min_max(egui::pos2(xc - span, top), egui::pos2(xc, bottom))
            }
            Side::Buy => {
                egui::Rect::from_min_max(egui::pos2(xc, top), egui::pos2(xc + span, bottom))
            }
        };
        painter.rect_filled(
            bar,
            egui::Rounding::ZERO,
            theme::side_color(side).gamma_multiply(BIDASK_BAR_ALPHA),
        );
        if imbalanced {
            // A cap on the growing end, in the side's own ink. It reads as a
            // tipped bar rather than a coloured one — form first, hue as
            // backup — and it lands where the eye already is, at the end of
            // the longest bar in the row.
            let cap = match side {
                Side::Sell => egui::Rect::from_min_max(
                    egui::pos2(bar.left(), top),
                    egui::pos2(bar.left() + IMBALANCE_EDGE_PX, bottom),
                ),
                Side::Buy => egui::Rect::from_min_max(
                    egui::pos2(bar.right() - IMBALANCE_EDGE_PX, top),
                    egui::pos2(bar.right(), bottom),
                ),
            };
            painter.rect_filled(cap, egui::Rounding::ZERO, theme::ink(side));
        }
    }
    if geometry.is_poc {
        draw_poc_dot(frame, xc, geometry.row, row_group);
    }
}

/// One row of the `cluster` style: the reference chart's boxed ladder — bid,
/// ask and the row total, each cell shaded by how much volume it holds.
#[allow(clippy::too_many_arguments)]
fn draw_cluster_row(
    frame: &LayerFrame<'_>,
    level: DetailLevel,
    cell: &FootprintLevel,
    geometry: &RowGeometry,
    xc: f32,
    heat: Option<HeatScale>,
    max_volume: f64,
    row_group: f64,
) {
    let painter = frame.painter;
    let reach = (frame.half - 1.0).max(1.0);
    let inset = crate::footprint_config::CANDLE_LANE_PX;
    // No inset: a half pixel a side became a four-pixel seam in a sixteen-pixel
    // row once antialiasing had spread both edges — a quarter of the row given
    // to gaps, which reads as a black grid rather than as raised cells. The
    // bevel is what separates one row from the next here.
    let top = geometry.top;
    let bottom = geometry.bottom;
    let columns = cluster_columns(frame.config);
    let column_width = cluster_column_px_from(2.0 * reach, columns);
    let bevel = frame.config.cluster_bevel && level == DetailLevel::Detailed;
    // The width budget is a *font size*, not a width: a five-glyph quantity is
    // `QUANTITY_GLYPHS * GLYPH_EM` ≈ 3 em of monospace, so the room a column
    // has buys a third of that in point size. Handing the column's raw width
    // to the font instead is how three columns of numbers end up written over
    // each other — which is exactly what the first capture of this style
    // showed. Same arithmetic the ladder's own budget uses.
    let width_budget = (column_width - 2.0) / (QUANTITY_GLYPHS * GLYPH_EM);
    let font = egui::FontId::monospace(
        (geometry.row_height - 2.0)
            .min(width_budget)
            .clamp(LADDER_MIN_FONT_PX, 13.0),
    );

    let mut x = xc - reach + inset + CLUSTER_BOX_PAD_PX;
    let mut column_rect = |painter: &egui::Painter| -> egui::Rect {
        let rect =
            egui::Rect::from_min_max(egui::pos2(x, top), egui::pos2(x + column_width, bottom));
        let _ = painter;
        x += column_width + CLUSTER_GUTTER_PX;
        rect
    };

    // Bid then ask, in that order and never the other: the ramp is
    // isoluminant between the two sides, so under deuteranopia the hues
    // collapse and *position* is what still says which side a number is.
    // That is a deliberate trade — luminance carries the ordinal reading,
    // which works for everyone — and it only holds while the columns stay put.
    for (side, qty, imbalanced) in [
        (Side::Sell, cell.sell, geometry.sell_imbalance),
        (Side::Buy, cell.buy, geometry.buy_imbalance),
    ] {
        let rect = column_rect(painter);
        let step = heat_step(qty, heat);
        painter.rect_filled(rect, egui::Rounding::ZERO, heat_fill(side, step));
        if bevel {
            paint_bevel(painter, rect, geometry.row_height, step);
        }
        if imbalanced {
            // The outline flips with the ink, and for the same reason. A
            // side's lightened ink is *darker* than the ramp's top steps —
            // measured, 1.27:1 — so the mark that says "look here" was
            // vanishing into the cell it was meant to ring, precisely on the
            // busiest rows.
            let outline = if step >= HEAT_INK_FLIP_STEP {
                theme::CHIP_INK
            } else {
                theme::ink(side)
            };
            painter.rect_stroke(
                rect.shrink(0.5),
                egui::Rounding::ZERO,
                egui::Stroke::new(1.5_f32, outline),
            );
        }
        if frame.config.show_numbers && !qty.is_zero() {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                fmt_qty(qty),
                font.clone(),
                heat_ink(step),
            );
        }
    }

    // The total column carries a silhouette, not a heat step. The grey bar
    // answers "where did volume concentrate" on an axis — length — that does
    // not compete with the digit for contrast, so the whole column lives on a
    // single ink with no flip rule. It is also the same silhouette the split
    // style draws, which is the point: one visual idea, two places.
    if columns > 2.0 {
        let rect = column_rect(painter);
        let volume = cell.volume();
        // Against the bar's own busiest row, not the screen-wide side scale.
        // A row total is structurally about twice one side, so measuring it
        // with a per-side scale pinned nine rows in ten at full width —
        // wallpaper with a number on it rather than a histogram. Per bar is
        // also the right question here: "which row of *this* bar held the
        // volume", which is what the split style's silhouette answers too.
        let frac = if max_volume > 0.0 {
            (volume.to_f64().unwrap_or(0.0) / max_volume).clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        painter.rect_filled(
            egui::Rect::from_min_max(
                rect.min,
                egui::pos2(rect.left() + rect.width() * frac, rect.bottom()),
            ),
            egui::Rounding::ZERO,
            PROFILE_COLOR.gamma_multiply(CLUSTER_TOTAL_SILHOUETTE_ALPHA),
        );
        if bevel {
            paint_bevel(painter, rect, geometry.row_height, 0);
        }
        if frame.config.show_numbers {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                fmt_qty(volume),
                font,
                theme::TEXT_PRIMARY,
            );
        }
    }

    if geometry.is_poc {
        // A ring, with the casing under-stroke the ramp makes mandatory:
        // POC yellow over the ramp's brightest step is 1.1:1, and a signal
        // that vanishes on the busiest rows is worse than no signal.
        let ring = egui::Rect::from_min_max(
            egui::pos2(xc - reach + inset, geometry.top + 0.5),
            egui::pos2(xc + reach, geometry.bottom - 0.5),
        );
        painter.rect_stroke(
            ring,
            egui::Rounding::ZERO,
            egui::Stroke::new(1.5 + theme::CASING_EXTRA_PX, theme::CASING),
        );
        painter.rect_stroke(
            ring,
            egui::Rounding::ZERO,
            egui::Stroke::new(1.5_f32, theme::POC),
        );
    }
    let _ = row_group;
}

/// Full-candle POC line, the non-split styles' and Marks level's shape.
fn draw_poc_dot(frame: &LayerFrame<'_>, xc: f32, row: i64, row_group: f64) {
    draw_poc_line(frame, xc - frame.half, xc + frame.half, row, row_group);
}

fn draw_zone_mark(frame: &LayerFrame<'_>, mark: &ZoneMark, row_group: f64) {
    // Composed from both rows' screen bands: upside down the high bucket
    // renders below the low one, and reading "top" off it would hand both
    // rects a negative height (see the Split backdrop above).
    let (high_top, high_bottom) = row_band(frame, mark.high_bucket, row_group);
    let (low_top, low_bottom) = row_band(frame, mark.low_bucket, row_group);
    let top = high_top.min(low_top);
    let bottom = high_bottom.max(low_bottom);
    let left = (frame.x_center)(mark.first_slot) - frame.half;
    let right = (frame.x_center)(mark.last_slot) + frame.half;
    // A zone marks the bars that formed it with a wash, and closes on a
    // firmer band at the right — the side that dominated, one bar past the
    // last one that re-formed it.
    //
    // It does *not* yet outlive those bars. A zone's trading value is memory
    // — a level to watch for a retest — and memory needs a life after its
    // origin plus a rule for when it dies (a stacked imbalance dies on a
    // print through the far edge; absorption dies on a *close* through it,
    // because a wick that pierces and returns is the defender holding). That
    // is a level-memory of its own, shared with naked POCs and absorption,
    // and it is not this change.
    let color = theme::side_color(mark.side);
    frame.painter.rect_filled(
        egui::Rect::from_min_max(egui::pos2(left, top), egui::pos2(right, bottom)),
        egui::Rounding::ZERO,
        color.gamma_multiply(0.10),
    );
    let edge_x = right + 1.0;
    frame.painter.rect_filled(
        egui::Rect::from_min_max(egui::pos2(edge_x, top), egui::pos2(edge_x + 2.0, bottom)),
        egui::Rounding::ZERO,
        color.gamma_multiply(0.7),
    );
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
    // the market or the chart.
    if frame.depth_visible && style.plate() == StylePlate::Casing {
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

#[cfg(test)]
mod tests {
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
    #[allow(clippy::assertions_on_constants)]
    fn the_zoom_ceiling_reaches_the_detailed_level() {
        assert!(crate::viewport::MAX_CANDLE_WIDTH >= ladder_detailed_min_width());
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
        let scale = heat_scale(std::iter::once(regroup(&ladder, 1))).expect("a scale");

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
        let scale = heat_scale(std::iter::once(regroup(&ladder, 1))).expect("a scale");
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
                let previous = relative_luminance(heat_fill(side, step - 1));
                let current = relative_luminance(heat_fill(side, step));
                assert!(
                    current > previous,
                    "{side:?} step {step} is not brighter than {}",
                    step - 1
                );
            }
        }
        for step in 0..HEAT_STEP_COUNT {
            let buy = relative_luminance(heat_fill(Side::Buy, step));
            let sell = relative_luminance(heat_fill(Side::Sell, step));
            assert!(
                (buy - sell).abs() < 0.02,
                "step {step}: buy L={buy:.3} vs sell L={sell:.3} — the sides must weigh the same"
            );
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
        let cluster = detailed_min_width(FootprintStyle::Cluster);
        let ladder = detailed_min_width(FootprintStyle::Ladder);
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
        let cluster_column = cluster_column_px(detailed_min_width(FootprintStyle::Cluster));
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
}
