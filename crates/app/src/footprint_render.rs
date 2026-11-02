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
//! formatting) is a pure function tested without egui — the level ladder in
//! [`quantick_chart::footprint_lod`], the ladder arithmetic in
//! [`quantick_chart::footprint_projection`]; only the painting itself touches
//! the frame.

use std::collections::BTreeMap;

use eframe::egui;
use quantick_engine::{BarFootprint, FootprintLevel, Side, StackedZone};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive as _;

use crate::chart::PriceScale;
use crate::footprint_config::{FootprintStyle, StylePlate};
#[cfg(test)]
use crate::theme;

use bar::{BarPaint, draw_bar, draw_poc_dot, draw_zone_mark};
use heat::{HeatScale, heat_scale};
use quantick_chart::footprint_legend::{LegendFacts, legend_text};
use quantick_chart::footprint_lod::{DetailLevel, GROUP_SNAP, height_to_split, with_poc_floor};
use quantick_chart::footprint_projection::{
    ADAPTIVE_FLOOR_BARS, ZoneMark, adaptive_min_qty, bar_delta, coalesce_zones, fmt_delta, fmt_qty,
    poc_of, regroup, zones_of,
};
use quantick_stores::footprint_presentation::{
    CENTER_GUTTER_PX, CLUSTER_BOX_PAD_PX, CLUSTER_GUTTER_PX, detailed_min_width,
};
pub use quantick_stores::footprint_presentation::{FootprintLod, candle_dressing};

mod bar;
mod heat;

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

/// Everything one frame of the layer needs, borrowed from the pane's draw.
pub struct LayerFrame<'a> {
    pub palette: std::sync::Arc<crate::theme::Palette>,
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
///
/// One frame is three steps, each a value of its own: resolve what the zoom
/// supports ([`LayerPlan`]), fix this frame's display grid and thresholds
/// ([`LayerPass`]), then paint in order — the zone washes under the cells,
/// the bars, the delta strip, and last the [`Legend`] saying what was drawn.
pub fn draw_layer(frame: &LayerFrame<'_>, lod: &mut FootprintLod) {
    let plan = LayerPlan::resolve(frame, lod);
    if plan.level == DetailLevel::Off {
        // Nothing to compute and nothing to draw — but the legend still
        // explains the silence, or an enabled layer reads as broken.
        plan.into_legend(1, Decimal::ZERO, None).draw(frame);
        return;
    }
    let pass = LayerPass::new(frame, &plan, lod);
    let ladders = pass.regroup_visible();
    let zones_dropped = pass.paint_zone_marks(ladders.zones);
    let heat = pass.heat(lod, &ladders.rows);
    let cells_left = pass.paint_bars(&ladders.rows, heat);
    pass.paint_delta_totals();

    let mut legend = plan.into_legend(pass.k, pass.min_qty, pass.split_hint);
    legend.aggregated_any = ladders.aggregated_any;
    legend.capped = cells_left == 0;
    legend.zones_dropped = zones_dropped;
    legend.draw(frame);
}

/// What the zoom supports this frame: the capture grid, the level and the
/// style actually drawn after any handover. Resolved once, before anything
/// is measured against a ladder.
struct LayerPlan {
    group: Decimal,
    group_f: f64,
    base_row_px: f32,
    level: DetailLevel,
    style: FootprintStyle,
    /// The `QUANTICK_FOOTPRINT_DEBUG` suffix for the legend, when asked for.
    debug: Option<String>,
}

impl LayerPlan {
    fn resolve(frame: &LayerFrame<'_>, lod: &mut FootprintLod) -> Self {
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
        let scaled_width = frame.config.scaled_width(frame.candle_width);
        // `auto` is answered before anything is measured: it is a question about
        // the zoom, and every floor below is measured against a concrete style.
        // The chain is walked richest-first and the first link the candle can pay
        // for wins, so one wheel walks three columns → two → a shape.
        let requested = frame.config.style.resolve_auto(|style| {
            scaled_width
                >= detailed_min_width(style, frame.config)
                    * frame.palette.footprint_text.width_scale()
        });
        let scaled = lod.levels.resolve(
            scaled_width,
            base_row_px,
            frame
                .palette
                .footprint_text
                .row_floors(frame.config.row_floors()),
            frame.config.compact_entry_width() * frame.palette.footprint_text.width_scale(),
            detailed_min_width(requested, frame.config)
                * frame.palette.footprint_text.width_scale(),
        );
        let level = with_poc_floor(scaled, frame.candle_width);
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
        #[cfg(any(feature = "scenario-harness", test))]
        let debug = {
            static DEBUG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
            (*DEBUG.get_or_init(|| {
                crate::hooks::captured::var("QUANTICK_FOOTPRINT_DEBUG")
                    .is_some_and(|value| value == "1")
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
        #[cfg(not(any(feature = "scenario-harness", test)))]
        let debug: Option<String> = None;
        Self {
            group,
            group_f,
            base_row_px,
            level,
            style,
            debug,
        }
    }

    /// The legend for this plan at display multiple `k`, with no notes yet.
    fn into_legend(self, k: i64, min_qty: Decimal, split_hint: Option<f32>) -> Legend {
        Legend {
            level: self.level,
            style: self.style,
            group: self.group,
            split_hint,
            k,
            min_qty,
            aggregated_any: false,
            capped: false,
            zones_dropped: false,
            debug: self.debug,
        }
    }
}

/// The visible ladders on the display grid, carried between the passes.
struct DisplayLadders {
    /// Each drawable ladder's display rows beside its global slot.
    rows: Vec<(usize, BTreeMap<i64, FootprintLevel>)>,
    /// The stacked zones those rows hold, before coalescing.
    zones: Vec<(usize, StackedZone)>,
    /// Whether a cap-coarsened ladder was hidden.
    aggregated_any: bool,
}

/// One painting frame of a layer that draws something: the display grid and
/// the imbalance thresholds fixed for the frame, and the paint passes as
/// methods in the order they land.
struct LayerPass<'p, 'f> {
    frame: &'p LayerFrame<'f>,
    level: DetailLevel,
    style: FootprintStyle,
    /// The display multiple of the capture grid.
    k: i64,
    /// One display row's height in price.
    row_group_f: f64,
    /// The legend's "single rows at N× height", when rows are merged.
    split_hint: Option<f32>,
    ratio: Decimal,
    min_qty: Decimal,
}

impl<'p, 'f> LayerPass<'p, 'f> {
    fn new(frame: &'p LayerFrame<'f>, plan: &LayerPlan, lod: &mut FootprintLod) -> Self {
        // The configured floors: band fineness for the profile, the number-row
        // height for the text levels - the trader's "more, thinner rows".
        let min_row = frame
            .palette
            .footprint_text
            .row_floors(frame.config.row_floors())
            .for_level(plan.level);
        let resolved = lod.levels.resolve_multiple(plan.base_row_px, min_row);
        let split_hint = (plan.level >= DetailLevel::Profile)
            .then(|| height_to_split(resolved, plan.base_row_px, min_row))
            .flatten();
        let k = resolved.unwrap_or(GROUP_SNAP[GROUP_SNAP.len() - 1]);
        let min_qty = match frame.config.imbalance_min_qty {
            Some(pinned) => pinned,
            None => lod.adaptive_floor(frame.footprints.len(), plan.group, || {
                adaptive_min_qty(frame.footprints.iter().rev().take(ADAPTIVE_FLOOR_BARS))
            }),
        };
        Self {
            frame,
            level: plan.level,
            style: plan.style,
            k,
            row_group_f: plan.group_f * k as f64,
            split_hint,
            ratio: frame.config.imbalance_ratio,
            min_qty,
        }
    }

    /// The ladders on screen, each beside its global slot.
    fn visible_ladders(&self) -> impl Iterator<Item = (usize, &'p BarFootprint)> + use<'p, 'f> {
        let frame = self.frame;
        let (start, end) = frame.visible;
        (start.max(frame.first_state_slot)..end)
            .filter_map(move |slot| {
                let fp = frame.footprints.get(slot - frame.first_state_slot)?;
                Some((slot, fp))
            })
            .chain(
                frame
                    .partial
                    .filter(|_| frame.partial_slot >= start && frame.partial_slot < end)
                    .map(|fp| (frame.partial_slot, fp)),
            )
    }

    /// The first walk over the visible ladders: each regrouped onto the
    /// display grid once, with its zones found on the same rows.
    ///
    /// Two passes over the visible ladders, and the split is not an
    /// optimisation: a zone's wash has to land *under* the cells, not over
    /// them. Painted last, it tinted the digits along with their background —
    /// a row that was both POC and inside a zone read at ~3.9:1. The regrouped
    /// rows are carried between the passes rather than folded twice, so the
    /// second pass costs nothing but the walk.
    fn regroup_visible(&self) -> DisplayLadders {
        let (start, end) = self.frame.visible;
        let mut ladders = DisplayLadders {
            rows: Vec::with_capacity(end.saturating_sub(start)),
            zones: Vec::new(),
            aggregated_any: false,
        };
        if self.level >= DetailLevel::Marks {
            for (slot, fp) in self.visible_ladders() {
                if fp.is_aggregated() {
                    // A cap-coarsened ladder lives on a doubled grid; drawing it
                    // with the frame's row geometry would put its rows at the
                    // wrong prices. Hiding it and saying so is the honest v1
                    // (the cap only trips on pathological bars).
                    ladders.aggregated_any = true;
                    continue;
                }
                // Zones and the POC are computed on the display rows the eye
                // compares — the same rows the cells draw.
                let rows = regroup(fp, self.k);
                for zone in zones_of(
                    &rows,
                    self.ratio,
                    self.min_qty,
                    self.frame.config.stacked_count,
                ) {
                    ladders.zones.push((slot, zone));
                }
                ladders.rows.push((slot, rows));
            }
        }
        ladders
    }

    /// The zone washes, under everything else; `true` when the cap dropped some.
    fn paint_zone_marks(&self, zones: Vec<(usize, StackedZone)>) -> bool {
        let (marks, zones_dropped) = coalesce_zones(zones, MAX_ZONE_MARKS);
        for mark in &marks {
            draw_zone_mark(self.frame, mark, self.row_group_f);
        }
        zones_dropped
    }

    /// The heat ramp's cuts: percentiles of the distribution the visible ladders
    /// hold, on the display grid they are drawn on. Read from the maps the pass
    /// above already built rather than folding them a second time — the cache
    /// key moves with the visible window, so during a drag every frame is a
    /// miss, and a second fold there was a full regroup of every visible bar at
    /// frame rate.
    fn heat(
        &self,
        lod: &mut FootprintLod,
        rows: &[(usize, BTreeMap<i64, FootprintLevel>)],
    ) -> Option<HeatScale> {
        if self.style == crate::footprint_config::FootprintStyle::Cluster {
            lod.heat_scale(
                self.frame.visible,
                self.frame.footprints.len(),
                self.k,
                || heat_scale(rows.iter().map(|(_, rows)| rows)),
            )
        } else {
            None
        }
    }

    /// The second walk: every bar's rows, or its POC alone below Profile.
    /// Returns the cell budget left, so the legend can say it ran out.
    fn paint_bars(
        &self,
        rows: &[(usize, BTreeMap<i64, FootprintLevel>)],
        heat: Option<HeatScale>,
    ) -> usize {
        let frame = self.frame;
        let paint = BarPaint {
            frame,
            level: self.level,
            style: self.style,
            row_group: self.row_group_f,
            ratio: self.ratio,
            min_qty: self.min_qty,
            heat,
        };
        let mut cells_left = CELL_BUDGET;
        for (slot, rows) in rows {
            if self.level >= DetailLevel::Profile && cells_left > 0 {
                draw_bar(&paint, rows, (frame.x_center)(*slot), &mut cells_left);
            } else if frame.config.show_poc
                && let Some(poc) = poc_of(rows)
            {
                draw_poc_dot(frame, (frame.x_center)(*slot), poc, self.row_group_f);
            }
        }
        cells_left
    }

    /// The per-bar delta totals strip at the chart's bottom — the reference
    /// charts' footer chips: one signed, side-colored number per bar saying
    /// who won it overall. From Compact up: at Profile widths the chips
    /// would overlap into noise. `bar_delta` is the tested fold.
    ///
    /// Every style, not just the split. Who won the bar is a reading of the
    /// bar, orthogonal to how its rows are drawn; withholding it from the
    /// ladder made that style strictly poorer than its sibling rather than a
    /// different way of seeing the same thing.
    fn paint_delta_totals(&self) {
        let frame = self.frame;
        if self.level < DetailLevel::Compact || !frame.config.show_delta_totals {
            return;
        }
        for (slot, fp) in self.visible_ladders() {
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
                frame.palette.font(quantick_appearance::FontRole::Footprint),
                frame.palette.footprint_total_ink(),
            );
            let center = egui::pos2(
                (frame.x_center)(slot),
                frame.chart_rect.bottom() - TOTALS_STRIP_OFFSET_Y,
            );
            let rect = egui::Rect::from_center_size(center, galley.size() + egui::vec2(6.0, 3.0));
            frame.painter.rect_filled(
                rect,
                egui::Rounding::same(2.0),
                frame.palette.side_color(side).gamma_multiply(0.8),
            );
            frame.painter.galley(
                rect.min + egui::vec2(3.0, 1.5),
                galley,
                frame.palette.footprint_total_ink(),
            );
        }
    }
}

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

/// What the legend line says about this frame: the level and style drawn,
/// the effective row grouping, the imbalance floor, and each honesty note a
/// pass raised along the way. Built by the frame, painted last.
struct Legend {
    level: DetailLevel,
    style: FootprintStyle,
    group: Decimal,
    /// How much taller merged rows must draw to split (`height_to_split`).
    split_hint: Option<f32>,
    k: i64,
    min_qty: Decimal,
    /// A cap-coarsened ladder was hidden.
    aggregated_any: bool,
    /// The cell budget ran out before every row was painted.
    capped: bool,
    /// The zone cap kept only the strongest zones.
    zones_dropped: bool,
    debug: Option<String>,
}

impl Legend {
    fn draw(self, frame: &LayerFrame<'_>) {
        let palette = &frame.palette;
        let Self {
            level,
            style,
            group,
            split_hint,
            k,
            min_qty,
            aggregated_any,
            capped,
            zones_dropped,
            debug,
        } = self;
        let text = legend_text(&LegendFacts {
            requested_style: frame.config.style.id(),
            drawn_style: style.id(),
            detailed_legend: style.detailed_legend(),
            level,
            candle_width: frame.candle_width,
            numbers_at_px: frame.config.compact_entry_width() * frame.config.detail_scale,
            rows: group.saturating_mul(Decimal::from(k)),
            split_hint,
            min_qty,
            heat_cells: style == crate::footprint_config::FootprintStyle::Cluster,
            aggregated_any,
            side_inferred: frame.side_inferred,
            plate_over_map: frame.depth_visible && style.plate() == StylePlate::Casing,
            capped,
            zone_cap: zones_dropped.then_some(MAX_ZONE_MARKS),
            debug: debug.as_deref(),
        });
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
            frame.palette.label_font(10.5),
            palette.text_muted(),
        );
    }
}

#[cfg(any(feature = "scenario-harness", test))]
crate::hooks::declare_hooks!["QUANTICK_FOOTPRINT_DEBUG"];

#[cfg(test)]
mod tests {
    use super::bar::*;
    const PROFILE_COLOR: egui::Color32 = egui::Color32::from_gray(0xD8);
    use super::*;

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
}
