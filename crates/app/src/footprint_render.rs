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

/// Candle-width floors per level, in pixels — the typography budget of the
/// numbers each level draws (a `1234 x 5678` cell needs ~72 px before it is a
/// number and not a smear).
const DETAILED_MIN_WIDTH: f32 = 72.0;
const COMPACT_MIN_WIDTH: f32 = 40.0;
const PROFILE_MIN_WIDTH: f32 = 18.0;
const MARKS_MIN_WIDTH: f32 = 8.0;
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

/// How far above the chart's bottom edge the per-bar delta totals sit —
/// clear of the legend line below them.
const TOTALS_STRIP_OFFSET_Y: f32 = 22.0;

/// The split style's per-bar backdrop: the canvas color at ~65% alpha, so
/// the footprint owns its interior over the heatmap while the map stays
/// fully visible between candles.
const CANVAS_BACKDROP: egui::Color32 = egui::Color32::from_rgba_premultiplied(12, 15, 22, 166);

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
    let span = DETAILED_MIN_WIDTH - PROFILE_MIN_WIDTH;
    (1.0 - (candle_width - PROFILE_MIN_WIDTH) / span).clamp(0.0, 1.0)
}

/// The smallest detail level — and the row multiple — with hysteresis
/// applied, per pane.
#[derive(Debug, Default)]
pub struct FootprintLod {
    level: Option<DetailLevel>,
    k: Option<i64>,
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
    ) -> DetailLevel {
        let strict = level_for(candle_width, base_row_px, profile_row_px);
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
                );
                if relaxed < current { strict } else { current }
            }
            Some(current) if strict > current => {
                let confirmed = level_for(
                    candle_width / LEVEL_HYSTERESIS,
                    base_row_px / LEVEL_HYSTERESIS,
                    profile_row_px,
                );
                if confirmed >= strict { strict } else { current }
            }
            _ => strict,
        };
        self.level = Some(level);
        level
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
fn level_for(candle_width: f32, base_row_px: f32, profile_row_px: f32) -> DetailLevel {
    let row_reachable = |min_row: f32| display_multiple(base_row_px, min_row).is_some();
    if candle_width >= DETAILED_MIN_WIDTH && row_reachable(DETAILED_MIN_ROW) {
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
    volumes.sort_by(f64::total_cmp);
    let p60 = volumes[(volumes.len().saturating_sub(1)) * 60 / 100];
    Decimal::from_f64(p60).unwrap_or(Decimal::ZERO)
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
    let base_row_px = (frame.scale.y(0.0) - frame.scale.y(group_f)).abs();
    let level = lod.resolve(frame.candle_width, base_row_px, frame.config.profile_row_px);

    if level == DetailLevel::Off {
        // Nothing to compute and nothing to draw — but the legend still
        // explains the silence, or an enabled layer reads as broken.
        draw_legend(frame, level, group, 1, false, false, false, Decimal::ZERO);
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

    let min_qty = frame.config.imbalance_min_qty.unwrap_or_else(|| {
        adaptive_min_qty(frame.footprints.iter().rev().take(ADAPTIVE_FLOOR_BARS))
    });
    let ratio = frame.config.imbalance_ratio;

    let mut cells_left = CELL_BUDGET;
    let mut aggregated_any = false;
    let mut zones: Vec<(usize, StackedZone)> = Vec::new();

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
            if level >= DetailLevel::Profile && cells_left > 0 {
                draw_bar(
                    frame,
                    level,
                    &rows,
                    row_group_f,
                    (frame.x_center)(slot),
                    ratio,
                    min_qty,
                    &mut cells_left,
                );
            } else if frame.config.show_poc
                && let Some(poc) = poc_of(&rows)
            {
                draw_poc_dot(frame, (frame.x_center)(slot), poc, row_group_f);
            }
        }
    }

    let (marks, zones_dropped) = coalesce_zones(zones, MAX_ZONE_MARKS);
    for mark in &marks {
        draw_zone_mark(frame, mark, row_group_f);
    }

    // The per-bar delta totals strip at the chart's bottom — the reference
    // charts' footer chips: one signed, side-colored number per bar saying
    // who won it overall. From Compact up: at Profile widths the chips
    // would overlap into noise.
    if level >= DetailLevel::Compact
        && frame.config.style == crate::footprint_config::FootprintStyle::Split
    {
        for (slot, fp) in visible_ladders() {
            let delta: Decimal = fp
                .levels()
                .values()
                .fold(Decimal::ZERO, |sum, cell| sum.saturating_add(cell.delta()));
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
                side_color(side).gamma_multiply(0.8),
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
        group,
        k,
        aggregated_any,
        cells_left == 0,
        zones_dropped,
        min_qty,
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
fn row_band(frame: &LayerFrame<'_>, row: i64, row_group: f64) -> (f32, f32) {
    let low = row as f64 * row_group;
    let top = frame.scale.y(low + row_group);
    let bottom = frame.scale.y(low);
    (top, bottom)
}

fn side_color(side: Side) -> egui::Color32 {
    match side {
        Side::Buy => theme::BUY,
        Side::Sell => theme::SELL,
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_bar(
    frame: &LayerFrame<'_>,
    level: DetailLevel,
    rows: &BTreeMap<i64, FootprintLevel>,
    row_group: f64,
    xc: f32,
    ratio: Decimal,
    min_qty: Decimal,
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

    // Split style: the footprint owns the candle's interior. A backdrop in
    // the canvas color keeps the heatmap from bleeding through the rows (it
    // stays fully visible between candles), and a hairline spine at the
    // central axis keeps the bar's midline readable after the candle body
    // fades to outline — the reference charts' thin gray candle spine.
    if frame.config.style == crate::footprint_config::FootprintStyle::Split
        && level >= DetailLevel::Profile
        && let (Some(&first), Some(&last)) = (rows.keys().next(), rows.keys().next_back())
    {
        let (_, bar_bottom) = row_band(frame, first, row_group);
        let (bar_top, _) = row_band(frame, last, row_group);
        let reach = (frame.half - 1.0).max(1.0);
        painter.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(xc - reach, bar_top),
                egui::pos2(xc + reach, bar_bottom),
            ),
            egui::Rounding::ZERO,
            CANVAS_BACKDROP,
        );
        painter.line_segment(
            [egui::pos2(xc, bar_top), egui::pos2(xc, bar_bottom)],
            egui::Stroke::new(1.0_f32, theme::TEXT_FAINT.gamma_multiply(0.3)),
        );
    }

    for (&row, cell) in rows {
        if *cells_left == 0 {
            return;
        }
        *cells_left -= 1;
        let (top, bottom) = row_band(frame, row, row_group);
        if bottom < frame.chart_rect.top() || top > frame.chart_rect.bottom() {
            continue;
        }
        let row_height = (bottom - top).max(1.0);
        let is_poc = poc == Some(row);
        let buy_imbalance = dominates(cell.buy, neighbour(row - 1, Side::Sell));
        let sell_imbalance = dominates(cell.sell, neighbour(row + 1, Side::Buy));

        if frame.config.style == crate::footprint_config::FootprintStyle::Split
            && level >= DetailLevel::Profile
        {
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
                    side_color(side).gamma_multiply(if chip_side.is_some() { 0.55 } else { 0.5 }),
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
                    egui::Stroke::new(1.0_f32, side_color(side)),
                );
            }
            // Deep zoom: the delta number over the left half, side-colored
            // so the column scans without reading (panel must-fix), width-
            // clamped so it never bleeds out.
            if level == DetailLevel::Detailed
                && let Some(text) = delta_text
            {
                let width_budget = (frame.half - 6.0) / 3.0;
                let font =
                    egui::FontId::monospace((row_height - 2.0).min(width_budget).clamp(8.0, 13.0));
                painter.text(
                    egui::pos2(xc - 3.0, (top + bottom) / 2.0),
                    egui::Align2::RIGHT_CENTER,
                    text,
                    font,
                    winner.map_or(theme::TEXT_MUTED, side_color),
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
                let font =
                    egui::FontId::monospace((row_height - 2.0).min(width_budget).clamp(8.0, 13.0));
                if is_poc {
                    painter.rect_filled(
                        egui::Rect::from_min_max(
                            egui::pos2(xc - frame.half, top),
                            egui::pos2(xc + frame.half, bottom),
                        ),
                        egui::Rounding::ZERO,
                        theme::POC.gamma_multiply(0.18),
                    );
                    draw_poc_dot(frame, xc, row, row_group);
                }
                let mid = (top + bottom) / 2.0;
                if level == DetailLevel::Compact {
                    let delta = cell.delta();
                    let color = if delta >= Decimal::ZERO {
                        theme::BUY
                    } else {
                        theme::SELL
                    };
                    painter.text(
                        egui::pos2(xc, mid),
                        egui::Align2::CENTER_CENTER,
                        fmt_qty(delta),
                        font,
                        color,
                    );
                } else {
                    // sell | buy, the tape's own left-to-right: taker-sells
                    // hit the bid printed on the left, taker-buys lift the
                    // ask on the right. Imbalanced cells get a filled pill so
                    // the eye catches them before the digits resolve.
                    for (side, qty, imbalanced, align, x) in [
                        (
                            Side::Sell,
                            cell.sell,
                            sell_imbalance,
                            egui::Align2::RIGHT_CENTER,
                            xc - 3.0,
                        ),
                        (
                            Side::Buy,
                            cell.buy,
                            buy_imbalance,
                            egui::Align2::LEFT_CENTER,
                            xc + 3.0,
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
                            painter.rect_filled(
                                cell_rect,
                                egui::Rounding::same(2.0),
                                side_color(side).gamma_multiply(0.35),
                            );
                        }
                        let color = if imbalanced {
                            side_color(side)
                        } else {
                            theme::TEXT_MUTED
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
        for (extreme, align, offset) in [
            (Extreme::Low, egui::Align2::CENTER_TOP, 3.0),
            (Extreme::High, egui::Align2::CENTER_BOTTOM, -3.0),
        ] {
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
            let low = row as f64 * row_group;
            let y = match extreme {
                Extreme::Low => frame.scale.y(low) + offset,
                Extreme::High => frame.scale.y(low + row_group) + offset,
            };
            let text = format!("{:.1}x", ratio_value.to_f64().unwrap_or(0.0));
            let galley =
                painter.layout_no_wrap(text, egui::FontId::monospace(10.0), theme::TEXT_PRIMARY);
            let anchor = align.anchor_size(egui::pos2(xc, y), galley.size());
            painter.rect_filled(
                anchor.expand(2.0),
                egui::Rounding::same(2.0),
                CANVAS_BACKDROP,
            );
            painter.rect_stroke(
                anchor.expand(2.0),
                egui::Rounding::same(2.0),
                egui::Stroke::new(1.0_f32, side_color(dominant_side)),
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
        egui::Stroke::new(3.5_f32, CANVAS_BACKDROP),
    );
    frame.painter.line_segment(
        [egui::pos2(x_from, y), egui::pos2(x_to, y)],
        egui::Stroke::new(1.5_f32, theme::POC),
    );
}

/// Full-candle POC line, the non-split styles' and Marks level's shape.
fn draw_poc_dot(frame: &LayerFrame<'_>, xc: f32, row: i64, row_group: f64) {
    draw_poc_line(frame, xc - frame.half, xc + frame.half, row, row_group);
}

fn draw_zone_mark(frame: &LayerFrame<'_>, mark: &ZoneMark, row_group: f64) {
    let (top, _) = row_band(frame, mark.high_bucket, row_group);
    let (_, bottom) = row_band(frame, mark.low_bucket, row_group);
    let left = (frame.x_center)(mark.first_slot) - frame.half;
    let right = (frame.x_center)(mark.last_slot) + frame.half;
    // A zone is memory: it outlives its bars to the right edge as a hairline,
    // and marks its own bars with a firmer band on the side that dominated.
    let color = side_color(mark.side);
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
    group: Decimal,
    k: i64,
    aggregated_any: bool,
    capped: bool,
    zones_dropped: bool,
    min_qty: Decimal,
) {
    let mut text = String::from("footprint");
    match level {
        DetailLevel::Off => text.push_str(" · zoom in for detail"),
        DetailLevel::Marks => text.push_str(" · marks (zoom in for numbers)"),
        DetailLevel::Profile => text.push_str(" · profile"),
        DetailLevel::Compact => text.push_str(" · delta"),
        // The legend names what the columns actually are — "sell|buy" over
        // a delta ladder would misread every number (data honesty).
        DetailLevel::Detailed => text.push_str(
            if frame.config.style == crate::footprint_config::FootprintStyle::Split {
                " · delta|volume"
            } else {
                " · sell|buy"
            },
        ),
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
    if aggregated_any {
        text.push_str(" · coarsened bars hidden");
    }
    if frame.side_inferred {
        text.push_str(" · side inferred");
    }
    if capped {
        text.push_str(" · capped");
    }
    if zones_dropped {
        text.push_str(&format!(" · strongest {MAX_ZONE_MARKS} zones"));
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
        assert!(crate::viewport::MAX_CANDLE_WIDTH >= DETAILED_MIN_WIDTH);
    }

    #[test]
    fn levels_need_both_width_and_a_reachable_row_height() {
        // Wide candle, healthy rows: full detail.
        assert_eq!(
            level_for(100.0, 12.0, PROFILE_MIN_ROW),
            DetailLevel::Detailed
        );
        // Wide candle, hairline base rows: grouping x100 still reaches 12px.
        assert_eq!(
            level_for(100.0, 0.2, PROFILE_MIN_ROW),
            DetailLevel::Detailed
        );
        // Wide candle, sub-hairline rows: the extended snap ladder rescues
        // detail far deeper than 100× (an index future on the 0.01 fallback
        // grid), so only truly hopeless rows drop to profile, then marks.
        assert_eq!(
            level_for(100.0, 0.05, PROFILE_MIN_ROW),
            DetailLevel::Detailed
        );
        assert_eq!(
            level_for(100.0, 0.0006, PROFILE_MIN_ROW),
            DetailLevel::Profile
        );
        assert_eq!(
            level_for(100.0, 0.0003, PROFILE_MIN_ROW),
            DetailLevel::Marks
        );
        // Width floors gate exactly.
        assert_eq!(level_for(60.0, 12.0, PROFILE_MIN_ROW), DetailLevel::Compact);
        assert_eq!(level_for(30.0, 12.0, PROFILE_MIN_ROW), DetailLevel::Profile);
        assert_eq!(level_for(10.0, 12.0, PROFILE_MIN_ROW), DetailLevel::Marks);
        assert_eq!(level_for(6.0, 12.0, PROFILE_MIN_ROW), DetailLevel::Off);
    }

    /// Full body up to the Profile floor, outline-only by the Detailed
    /// floor, monotonic in between — and never outside [0, 1].
    #[test]
    fn candle_body_fade_spans_profile_to_detailed() {
        assert_eq!(candle_body_fade(8.0), 1.0);
        assert_eq!(candle_body_fade(PROFILE_MIN_WIDTH), 1.0);
        assert_eq!(candle_body_fade(DETAILED_MIN_WIDTH), 0.0);
        assert_eq!(candle_body_fade(160.0), 0.0);
        let mid = candle_body_fade((PROFILE_MIN_WIDTH + DETAILED_MIN_WIDTH) / 2.0);
        assert!(mid > 0.0 && mid < 1.0);
        assert!(candle_body_fade(30.0) > candle_body_fade(50.0));
    }

    #[test]
    fn lod_changes_only_past_the_dead_band_in_both_directions() {
        let mut lod = FootprintLod::default();
        // The first frame takes the strict answer.
        assert_eq!(
            lod.resolve(80.0, 12.0, PROFILE_MIN_ROW),
            DetailLevel::Detailed
        );
        // Just under the floor: inside the 15% band, the level holds.
        assert_eq!(
            lod.resolve(68.0, 12.0, PROFILE_MIN_ROW),
            DetailLevel::Detailed
        );
        // 15% past the floor: the downgrade happens.
        assert_eq!(
            lod.resolve(60.0, 12.0, PROFILE_MIN_ROW),
            DetailLevel::Compact
        );
        // Upgrades need the same clearance: 80px is over the 72px floor but
        // not 15% over, so the level holds — an instant upgrade against a
        // banded downgrade is a blinker at the boundary.
        assert_eq!(
            lod.resolve(80.0, 12.0, PROFILE_MIN_ROW),
            DetailLevel::Compact
        );
        assert_eq!(
            lod.resolve(90.0, 12.0, PROFILE_MIN_ROW),
            DetailLevel::Detailed
        );
        // The blinker scenario itself: oscillating across the floor by a
        // hair must not change the level once settled.
        for width in [73.0, 71.0, 73.0, 71.0, 73.0] {
            assert_eq!(
                lod.resolve(width, 12.0, PROFILE_MIN_ROW),
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
            lod.resolve(100.0, 0.0001, PROFILE_MIN_ROW),
            DetailLevel::Marks
        );
        // ...then the real span arrives: two steps away, no band, snap.
        assert_eq!(
            lod.resolve(100.0, 12.0, PROFILE_MIN_ROW),
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
}
