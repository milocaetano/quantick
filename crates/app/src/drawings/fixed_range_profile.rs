//! Fixed range volume profile: two anchors pick a bar range, the object shows
//! where inside that range the volume actually traded.
//!
//! The anchors carry the *time* axis of the object; the price axis comes from
//! the data — the profile's rows span whatever prices the range printed, not
//! whatever heights the anchors were dropped at. The engine does the folding
//! ([`VolumeProfile::merge`] over the range's per-bar footprint ladders); this
//! file only projects and paints what `frvp::refresh` cached in the payload.
//!
//! Honesty rules the paint must keep:
//! - a range whose bars carry no tape (venue prefix candles, a feed with no
//!   traded volume) says so instead of showing an empty histogram as "quiet";
//! - partial coverage is spoken (`N of M bars`), never blended away;
//! - a cap-coarsened profile names its effective row width, like the
//!   footprint legend does.

use eframe::egui;
use egui_phosphor::regular as icons;
use quantick_engine::{ValueArea, VolumeProfile};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::any::Any;

use super::measure_core::MEASURE_FAMILY;
use super::{
    Constrain, DrawContext, Drawing, DrawingPayload, DrawingStyle, DrawingToolImpl, Handles,
    PresetHost, drawing_stroke,
};
use crate::chart::to_f64;
use crate::theme;

pub(super) static TOOL: FixedRangeProfile = FixedRangeProfile;

pub(super) struct FixedRangeProfile;

const PRESET_FORMAT_VERSION: u32 = 1;
pub const DEFAULT_VALUE_AREA_PCT: u8 = 70;
const DEFAULT_WIDTH_FRAC: f32 = 0.30;
const MIN_WIDTH_FRAC: f32 = 0.10;
const MAX_WIDTH_FRAC: f32 = 1.0;
const LABEL_SIZE_PX: f32 = 10.0;
/// Gap between the range's geometry and its text plates.
const LABEL_OFFSET_PX: f32 = 4.0;
/// The resize handles never sit closer than this to the band's edges, so
/// they stay grabbable however tall the profile is.
const HANDLE_EDGE_MARGIN_PX: f32 = 8.0;
const VA_DASH_PX: f32 = 4.0;
const VA_GAP_PX: f32 = 3.0;
/// Row fills inside vs outside the value area — the area is read by weight,
/// not by outline.
const ROW_ALPHA_IN_VA: f32 = 0.55;
const ROW_ALPHA_OUT_VA: f32 = 0.30;
/// The dark half of the double stroke that keeps the profile readable over
/// the liquidity heatmap. Deliberately darker than `theme::CANVAS`: the
/// heatmap ramp starts at black, and a casing that ties with the ramp's
/// floor stops separating exactly where separation is needed. 12:1 against
/// the ramp's cyan plateau, ~24:1 against its yellow band (panel numbers).
const CASING: egui::Color32 = egui::Color32::from_rgba_premultiplied(5, 7, 12, 235);
/// How much wider the casing is than the ink it carries — 1px showing on
/// each side.
const CASING_EXTRA_PX: f32 = 2.0;
/// Silhouette ink weights: the value area keeps its by-weight reading in an
/// axis the heatmap does not occupy (stroke width + brightness), replacing
/// the fill alphas that a variable background destroys.
const OUTLINE_IN_VA_PX: f32 = 1.9;
const OUTLINE_OUT_VA_PX: f32 = 1.25;
const OUTLINE_OUT_VA_BRIGHTNESS: f32 = 0.6;

/// Why the payload holds no profile, spoken to the trader as-is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrvpEmpty {
    /// The bars in range carry no footprint ladders — venue history candles,
    /// or a range dropped where nothing traded.
    NoTape,
    /// The feed reports no traded volume, so no honest profile exists.
    Blocked,
}

/// What one refresh computed for one object. Derived state: deliberately
/// excluded from equality and presets, the same rule as `Drawing::off_series`,
/// so a recompute can never read as a user edit to the undo history.
#[derive(Debug, Clone)]
pub struct FrvpCache {
    /// The inputs this result was computed from; `frvp::refresh` skips the
    /// merge while it matches.
    pub key: FrvpCacheKey,
    /// The folded profile and its value area, when the range had tape.
    pub profile: Option<(VolumeProfile, Option<ValueArea>)>,
    /// Why `profile` is `None`, when it is.
    pub empty: Option<FrvpEmpty>,
    /// Bars whose ladders went into the fold (the partial counts once).
    pub bars_covered: usize,
    /// Bars folded from an **approximated** ladder — venue candles with no
    /// tape, their volume spread over their own high–low. Spoken by the
    /// status line, never blended away.
    pub bars_approximated: usize,
    /// Bars the anchors span on the chart, prefix candles included.
    pub bars_total: usize,
    /// The oldest global slot the L2 heatmap covers this frame — where the
    /// paint cuts from fill to silhouette. Presentation state beside the
    /// key: the map's boundary moving must never re-merge the fold.
    pub heat_first_slot: Option<usize>,
}

/// Everything the merge depends on. Anchor moves change the slots, a refold
/// changes the group, a rebuild bumps the revision, a bar close bumps
/// `closed_len`, the live edge bumps `partial_snapshot`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrvpCacheKey {
    pub start_slot: usize,
    pub end_slot: usize,
    pub group: Decimal,
    pub timeline_revision: u64,
    pub closed_len: usize,
    pub include_partial: bool,
    pub partial_snapshot: u64,
    pub value_area_pct: u8,
    pub blocked: bool,
    /// Whether the feed *infers* aggressor sides (tick rule) rather than
    /// reporting them. In the key so a feed switch re-stamps the label: a
    /// delta whose sides were guessed must say so, like the footprint legend
    /// does.
    pub side_inferred: bool,
    /// Whether venue-history candles are folded in as approximated ladders
    /// — the payload's own switch, in the key so toggling it re-folds.
    pub approximate: bool,
}

/// The versioned on-disk shape of a saved preset. Coordinates and cache never
/// travel with it, only the tool-owned config.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct FrvpPresetData {
    version: u32,
    value_area_pct: u8,
    width_frac: f32,
    show_value_area: bool,
    show_poc: bool,
    delta_coloring: bool,
    show_labels: bool,
    /// Added after v1 presets shipped; absent in older files, so it defaults
    /// rather than invalidating them.
    #[serde(default)]
    extend_right: bool,
    /// Same vintage rule as `extend_right`: older presets default to the
    /// honest behaviour (outline over the map).
    #[serde(default = "default_true")]
    outline_over_heatmap: bool,
    /// Same vintage rule again: approximating venue history is the default
    /// because the label speaks whenever it happens.
    #[serde(default = "default_true")]
    approximate_history: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone)]
pub struct FrvpPayload {
    /// The value-area fraction, in percent — 70 is the classic convention.
    pub value_area_pct: u8,
    /// Histogram width as a fraction of the range width at full volume.
    pub width_frac: f32,
    pub show_value_area: bool,
    pub show_poc: bool,
    /// Split each row into its buy and sell quantities instead of one bar.
    pub delta_coloring: bool,
    pub show_labels: bool,
    /// The developing mode: the range's right edge is the newest bar (the
    /// forming one included), whatever the second anchor says — the profile
    /// keeps growing as the tape prints. The anchors are untouched, so
    /// switching this off restores exactly the range that was drawn.
    pub extend_right: bool,
    /// Over the liquidity heatmap the profile draws as a silhouette (double
    /// stroke over an untouched map) instead of a fill that composes into
    /// the cells. Off = always fill, for whoever prefers the solid object
    /// and accepts the fight.
    pub outline_over_heatmap: bool,
    /// Venue-history candles (no tape) join the fold as approximated
    /// ladders — volume spread over each candle's high–low, labeled
    /// `approximated from OHLC` in the status. Off = those bars contribute
    /// nothing, exactly as before this option existed.
    pub approximate_history: bool,
    /// Derived state, refreshed by `frvp::refresh`; see [`FrvpCache`].
    pub cache: Option<FrvpCache>,
}

impl Default for FrvpPayload {
    fn default() -> Self {
        Self {
            value_area_pct: DEFAULT_VALUE_AREA_PCT,
            width_frac: DEFAULT_WIDTH_FRAC,
            show_value_area: true,
            show_poc: true,
            delta_coloring: false,
            show_labels: true,
            extend_right: false,
            outline_over_heatmap: true,
            approximate_history: true,
            cache: None,
        }
    }
}

impl PartialEq for FrvpPayload {
    /// Config only — the cache is derived, and a refresh that changed it must
    /// not look like an edit (`Drawings::record` compares snapshots).
    fn eq(&self, other: &Self) -> bool {
        self.value_area_pct == other.value_area_pct
            && self.width_frac == other.width_frac
            && self.show_value_area == other.show_value_area
            && self.show_poc == other.show_poc
            && self.delta_coloring == other.delta_coloring
            && self.show_labels == other.show_labels
            && self.extend_right == other.extend_right
            && self.outline_over_heatmap == other.outline_over_heatmap
            && self.approximate_history == other.approximate_history
    }
}

impl DrawingPayload for FrvpPayload {
    fn clone_box(&self) -> Box<dyn DrawingPayload> {
        Box::new(self.clone())
    }
    fn eq_dyn(&self, other: &dyn DrawingPayload) -> bool {
        other
            .as_any()
            .downcast_ref::<Self>()
            .is_some_and(|other| self == other)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    fn export_preset(&self) -> Option<toml::Value> {
        toml::Value::try_from(FrvpPresetData {
            version: PRESET_FORMAT_VERSION,
            value_area_pct: self.value_area_pct,
            width_frac: self.width_frac,
            show_value_area: self.show_value_area,
            show_poc: self.show_poc,
            delta_coloring: self.delta_coloring,
            show_labels: self.show_labels,
            extend_right: self.extend_right,
            outline_over_heatmap: self.outline_over_heatmap,
            approximate_history: self.approximate_history,
        })
        .ok()
    }
    fn import_preset(&mut self, value: &toml::Value) -> bool {
        let Ok(data) = value.clone().try_into::<FrvpPresetData>() else {
            return false;
        };
        if data.version != PRESET_FORMAT_VERSION
            || data.value_area_pct == 0
            || data.value_area_pct > 100
            || !data.width_frac.is_finite()
        {
            return false;
        }
        self.value_area_pct = data.value_area_pct;
        self.width_frac = data.width_frac.clamp(MIN_WIDTH_FRAC, MAX_WIDTH_FRAC);
        self.show_value_area = data.show_value_area;
        self.show_poc = data.show_poc;
        self.delta_coloring = data.delta_coloring;
        self.show_labels = data.show_labels;
        self.extend_right = data.extend_right;
        self.outline_over_heatmap = data.outline_over_heatmap;
        self.approximate_history = data.approximate_history;
        // The cache key carries the value-area fraction, so the next refresh
        // recomputes; nothing to invalidate by hand.
        true
    }
}

/// The horizontal edges of the range, in screen space. Normally the two
/// anchors' x's; in the developing mode the right edge is the newest covered
/// bar instead, found by extending the anchors' own bar→x line — the mapping
/// is affine across the history region, so two anchors are enough to name
/// any slot's x without asking the pane.
fn range_edges(payload: &FrvpPayload, points: &[egui::Pos2], ctxt: &DrawContext<'_>) -> (f32, f32) {
    let left = points[0].x.min(points[1].x);
    let mut right = points[0].x.max(points[1].x);
    if payload.extend_right
        && let Some(cache) = payload.cache.as_ref()
        && let [a, b, ..] = ctxt.anchors
    {
        let bar_span = b.bar - a.bar;
        if bar_span.abs() > f32::EPSILON {
            let slot_width = (points[1].x - points[0].x) / bar_span;
            // The right boundary of the newest covered slot (its centre is
            // `end_slot + 0.5`, its trailing edge half a slot further).
            #[allow(clippy::cast_precision_loss)]
            let live_x = points[0].x + (cache.key.end_slot as f32 + 1.0 - a.bar) * slot_width;
            right = right.max(live_x);
        }
    }
    (left, right)
}

/// x of an arbitrary bar coordinate through the anchors' own affine bar→x
/// map — the same trick the developing mode's live edge uses. `None` when
/// the anchors share a bar and the map has no slope.
fn bar_x(points: &[egui::Pos2], anchors: &[super::ChartPoint], bar: f32) -> Option<f32> {
    let (a, b) = (anchors.first()?, anchors.get(1)?);
    let span = b.bar - a.bar;
    if span.abs() <= f32::EPSILON {
        return None;
    }
    let slot_width = (points[1].x - points[0].x) / span;
    Some(points[0].x + (bar - a.bar) * slot_width)
}

/// One straight piece of the silhouette, with the value-area membership that
/// picks its ink weight. Collected first and stroked in two passes — every
/// casing under every ink — so a corner never has a later casing overpainting
/// an earlier ink.
struct SilhouetteSegment {
    from: egui::Pos2,
    to: egui::Pos2,
    in_va: bool,
}

/// Text with a glyph knockout: the same galley painted four times offset in
/// the casing colour and once in its own — no plate, so no rectangle of the
/// map is covered to make room for a word.
fn knockout_text(
    painter: &egui::Painter,
    pos: egui::Pos2,
    anchor: egui::Align2,
    text: &str,
    color: egui::Color32,
) {
    let font = egui::FontId::proportional(LABEL_SIZE_PX);
    let casing_galley = painter.layout_no_wrap(text.to_owned(), font.clone(), CASING);
    let ink_galley = painter.layout_no_wrap(text.to_owned(), font, color);
    let corner = anchor.anchor_size(pos, ink_galley.size()).min;
    for offset in [
        egui::vec2(-1.0, 0.0),
        egui::vec2(1.0, 0.0),
        egui::vec2(0.0, -1.0),
        egui::vec2(0.0, 1.0),
    ] {
        painter.galley(corner + offset, casing_galley.clone(), CASING);
    }
    painter.galley(corner, ink_galley, color);
}

/// The vertical span the object occupies: the profile's own price extent when
/// there is one — the data owns the price axis — else the anchors' heights,
/// which is all a draft or an empty range has to say.
fn price_extent(
    payload: &FrvpPayload,
    points: &[egui::Pos2],
    ctxt: &DrawContext<'_>,
) -> (f32, f32) {
    if let Some((profile, _)) = payload.cache.as_ref().and_then(|cache| cache.profile.as_ref())
        && let (Some((&low, _)), Some((&high, _))) = (
            profile.levels().first_key_value(),
            profile.levels().last_key_value(),
        )
    {
        let top = ctxt.scale.y(to_f64(profile.bucket_price(high + 1)));
        let bottom = ctxt.scale.y(to_f64(profile.bucket_price(low)));
        return (top.min(bottom), top.max(bottom));
    }
    let ys: Vec<f32> = points.iter().map(|point| point.y).collect();
    let top = ys.iter().copied().fold(f32::INFINITY, f32::min);
    let bottom = ys.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    (top, bottom)
}

/// One row's screen rect data: bottom edge y, height, and the x the fill runs
/// to. Height comes from the scale's f64 density × the profile's group —
/// never `y(a) - y(b)`, whose f32 rounding drifts across a tall ladder.
fn row_height(profile: &VolumeProfile, ctxt: &DrawContext<'_>) -> f32 {
    #[allow(clippy::cast_possible_truncation)]
    let height = (ctxt.scale.px_per_price() * to_f64(profile.group())) as f32;
    height.max(1.0)
}

fn fmt_qty(value: Decimal) -> String {
    crate::chart::compact_value(to_f64(value))
}

impl DrawingToolImpl for FixedRangeProfile {
    fn id(&self) -> &'static str {
        "fixed-range-profile"
    }
    fn name(&self) -> &'static str {
        "Fixed range volume profile"
    }
    fn settings_title(&self) -> &'static str {
        "Fixed range volume profile settings"
    }
    fn icon(&self) -> &'static str {
        icons::CHART_BAR_HORIZONTAL
    }
    fn hover_text(&self) -> &'static str {
        "Fixed range volume profile - two bars, volume by price with POC and value area"
    }
    fn required_points(&self) -> usize {
        2
    }
    fn placement_hint(&self, placed: usize) -> Option<&'static str> {
        (placed == 1).then_some("click the other end of the range")
    }
    fn family(&self) -> Option<super::ToolFamily> {
        Some(MEASURE_FAMILY)
    }
    fn supports_fill(&self) -> bool {
        false
    }
    /// Profile rows are prices. Started over an indicator band, the object
    /// still belongs to the candles' price axis — see `price_band_only`.
    fn price_band_only(&self) -> bool {
        true
    }
    fn default_payload(&self) -> Box<dyn DrawingPayload> {
        Box::new(FrvpPayload::default())
    }
    fn extra_tab(&self) -> Option<&'static str> {
        Some("Profile")
    }
    fn draw_extra_tab(
        &self,
        ui: &mut egui::Ui,
        drawing: &mut Drawing,
        host: &mut dyn PresetHost,
    ) -> bool {
        draw_profile_tab(ui, drawing, host)
    }
    fn paint(
        &self,
        painter: &egui::Painter,
        chart_rect: egui::Rect,
        style: DrawingStyle,
        points: &[egui::Pos2],
        ctxt: &DrawContext<'_>,
    ) {
        let Some(payload) = ctxt.payload.as_any().downcast_ref::<FrvpPayload>() else {
            return;
        };
        let stroke = drawing_stroke(style);
        if points.len() < 2 {
            // A one-anchor draft with no hover yet: mark the starting edge.
            if let Some(point) = points.first() {
                painter.line_segment(
                    [
                        egui::pos2(point.x, chart_rect.top()),
                        egui::pos2(point.x, chart_rect.bottom()),
                    ],
                    stroke,
                );
            }
            return;
        }
        let (left, right) = range_edges(payload, points, ctxt);
        let (top, bottom) = price_extent(payload, points, ctxt);

        // The range's edges — the stroke geometry, which is all the halo
        // pass paints.
        for x in [left, right] {
            painter.line_segment([egui::pos2(x, top), egui::pos2(x, bottom)], stroke);
        }
        if ctxt.halo {
            return;
        }

        let cache = payload.cache.as_ref();
        let profile = cache.and_then(|cache| cache.profile.as_ref());

        // Where the fill gives way to the silhouette: the left boundary of
        // the liquidity map, expressed in this object's own coordinates. Left
        // of the cut the profile composes over candles exactly as before;
        // right of it a fill would compose into the map's cells (worst case
        // measured at 1.002:1 contrast) — so the shape is drawn instead, and
        // not one uncovered pixel of the map is altered.
        let cut_x = payload
            .outline_over_heatmap
            .then(|| cache.and_then(|cache| cache.heat_first_slot))
            .flatten()
            .and_then(|slot| {
                #[allow(clippy::cast_precision_loss)]
                bar_x(points, ctxt.anchors, slot as f32)
            })
            .map(|x| x.max(left));
        let outline_active = cut_x.is_some_and(|x| x < right);

        if let Some((profile, value_area)) = profile {
            let range_width = (right - left).max(1.0);
            let height = row_height(profile, ctxt);
            let max_volume = to_f64(profile.max_level_volume()).max(f64::MIN_POSITIVE);
            let in_va = |bucket: i64| {
                payload.show_value_area
                    && value_area.is_some_and(|area| bucket >= area.val && bucket <= area.vah)
            };
            let fill_limit = if outline_active { cut_x } else { None };
            for (&bucket, level) in profile.levels() {
                let y_bottom = ctxt.scale.y(to_f64(profile.bucket_price(bucket)));
                let y_top = y_bottom - height;
                if y_bottom < chart_rect.top() || y_top > chart_rect.bottom() {
                    continue;
                }
                #[allow(clippy::cast_possible_truncation)]
                let width = ((to_f64(level.volume()) / max_volume) as f32)
                    * payload.width_frac
                    * range_width;
                let tip = left + width;
                // The fill stops at the map's boundary; the silhouette pass
                // below carries the rest of the row.
                let fill_tip = fill_limit.map_or(tip, |cut| tip.min(cut));
                if fill_tip <= left {
                    continue;
                }
                let alpha = if in_va(bucket) {
                    ROW_ALPHA_IN_VA
                } else {
                    ROW_ALPHA_OUT_VA
                };
                if payload.delta_coloring {
                    // The row split by aggressor: buys from the edge, sells
                    // continuing — the same quantities the footprint shows.
                    let volume = to_f64(level.volume()).max(f64::MIN_POSITIVE);
                    #[allow(clippy::cast_possible_truncation)]
                    let buy_width = ((to_f64(level.buy) / volume) as f32) * width;
                    let buy_tip = (left + buy_width).min(fill_tip);
                    painter.rect_filled(
                        egui::Rect::from_min_max(
                            egui::pos2(left, y_top),
                            egui::pos2(buy_tip, y_bottom),
                        ),
                        egui::Rounding::ZERO,
                        theme::BUY.gamma_multiply(alpha),
                    );
                    if fill_tip > buy_tip {
                        painter.rect_filled(
                            egui::Rect::from_min_max(
                                egui::pos2(buy_tip, y_top),
                                egui::pos2(fill_tip, y_bottom),
                            ),
                            egui::Rounding::ZERO,
                            theme::SELL.gamma_multiply(alpha),
                        );
                    }
                } else {
                    painter.rect_filled(
                        egui::Rect::from_min_max(
                            egui::pos2(left, y_top),
                            egui::pos2(fill_tip, y_bottom),
                        ),
                        egui::Rounding::ZERO,
                        style.color.gamma_multiply(alpha),
                    );
                }
            }

            // The silhouette: the histogram's staircase envelope right of the
            // cut, double-stroked — casing under ink, all casings first so a
            // corner never has a later casing overpainting an earlier ink.
            // The value area keeps its by-weight reading in the ink's width
            // and brightness; rows short of the cut hug the boundary, so the
            // filled and outlined halves read as one object.
            if outline_active && let Some(base) = cut_x {
                let mut segments: Vec<SilhouetteSegment> = Vec::new();
                let mut previous: Option<(i64, f32, f32)> = None; // bucket, tip x, top y
                for (&bucket, level) in profile.levels() {
                    let y_bottom = ctxt.scale.y(to_f64(profile.bucket_price(bucket)));
                    let y_top = y_bottom - height;
                    if y_bottom < chart_rect.top() || y_top > chart_rect.bottom() {
                        continue;
                    }
                    #[allow(clippy::cast_possible_truncation)]
                    let width = ((to_f64(level.volume()) / max_volume) as f32)
                        * payload.width_frac
                        * range_width;
                    let ex = (left + width).max(base);
                    let row_in_va = in_va(bucket);
                    match previous {
                        Some((prev_bucket, prev_ex, prev_top)) if prev_bucket + 1 == bucket => {
                            // Contiguous rows: one horizontal at the shared
                            // boundary, from tip to tip.
                            segments.push(SilhouetteSegment {
                                from: egui::pos2(prev_ex, prev_top),
                                to: egui::pos2(ex, y_bottom),
                                in_va: row_in_va,
                            });
                        }
                        other => {
                            // A gap (or the first row): close the previous
                            // run down to the boundary and open this one.
                            if let Some((_, prev_ex, prev_top)) = other {
                                segments.push(SilhouetteSegment {
                                    from: egui::pos2(prev_ex, prev_top),
                                    to: egui::pos2(base, prev_top),
                                    in_va: row_in_va,
                                });
                            }
                            segments.push(SilhouetteSegment {
                                from: egui::pos2(base, y_bottom),
                                to: egui::pos2(ex, y_bottom),
                                in_va: row_in_va,
                            });
                        }
                    }
                    segments.push(SilhouetteSegment {
                        from: egui::pos2(ex, y_bottom),
                        to: egui::pos2(ex, y_top),
                        in_va: row_in_va,
                    });
                    previous = Some((bucket, ex, y_top));
                }
                if let Some((_, prev_ex, prev_top)) = previous {
                    segments.push(SilhouetteSegment {
                        from: egui::pos2(prev_ex, prev_top),
                        to: egui::pos2(base, prev_top),
                        in_va: false,
                    });
                }
                let ink_width = |in_va: bool| {
                    if in_va {
                        style.width_px.max(OUTLINE_IN_VA_PX)
                    } else {
                        style.width_px.clamp(0.75, OUTLINE_OUT_VA_PX)
                    }
                };
                for segment in &segments {
                    painter.line_segment(
                        [segment.from, segment.to],
                        egui::Stroke::new(ink_width(segment.in_va) + CASING_EXTRA_PX, CASING),
                    );
                }
                for segment in &segments {
                    let color = if segment.in_va {
                        style.color
                    } else {
                        style.color.gamma_multiply(OUTLINE_OUT_VA_BRIGHTNESS)
                    };
                    painter.line_segment(
                        [segment.from, segment.to],
                        egui::Stroke::new(ink_width(segment.in_va), color),
                    );
                }
            }

            if let Some(area) = value_area {
                if payload.show_poc {
                    let y = ctxt.scale.y(to_f64(
                        profile
                            .bucket_price(area.poc)
                            .saturating_add(profile.group() / Decimal::TWO),
                    ));
                    let width = style.width_px.max(1.0);
                    // The casing carries the POC over the map's yellow band —
                    // #FFD54F against it is the worst number of the scene
                    // (1.05:1); against the casing it is ~21:1. Over plain
                    // canvas the casing is near-invisible, so it simply stays.
                    painter.line_segment(
                        [egui::pos2(left, y), egui::pos2(right, y)],
                        egui::Stroke::new(width + CASING_EXTRA_PX, CASING),
                    );
                    painter.line_segment(
                        [egui::pos2(left, y), egui::pos2(right, y)],
                        egui::Stroke::new(width, theme::POC),
                    );
                }
                if payload.show_value_area {
                    // VAH tops its row, VAL bottoms its row: the dashes hug
                    // the area they bound. Casing dashes share the geometry,
                    // so the phase matches and the map shows through the gaps.
                    let vah_y = ctxt
                        .scale
                        .y(to_f64(profile.bucket_price(area.vah.saturating_add(1))));
                    let val_y = ctxt.scale.y(to_f64(profile.bucket_price(area.val)));
                    let width = style.width_px.max(0.75);
                    for y in [vah_y, val_y] {
                        let ends = [egui::pos2(left, y), egui::pos2(right, y)];
                        painter.add(egui::Shape::dashed_line(
                            &ends,
                            egui::Stroke::new(width + CASING_EXTRA_PX, CASING),
                            VA_DASH_PX,
                            VA_GAP_PX,
                        ));
                        painter.add(egui::Shape::dashed_line(
                            &ends,
                            egui::Stroke::new(width, style.color),
                            VA_DASH_PX,
                            VA_GAP_PX,
                        ));
                    }
                }
            }
        }

        if !ctxt.primary_band || !payload.show_labels {
            return;
        }
        // The status line under the range: what the profile is made of, in
        // the footprint legend's language. Everything honesty demands lives
        // here — coverage, effective rows, why the range is empty.
        let mut status = String::new();
        match (profile, cache) {
            (Some((profile, value_area)), Some(cache)) => {
                status.push_str(&format!(
                    "vol {} · Δ {}",
                    fmt_qty(profile.total_volume()),
                    fmt_qty(profile.total_delta())
                ));
                status.push_str(&format!(" · rows {}", profile.group()));
                if profile.is_aggregated() {
                    // The rows are coarser than the capture grid — spoken,
                    // like the footprint legend speaks its coarsening.
                    status.push_str(" · grouped");
                }
                if payload.extend_right {
                    // The developing mode: this profile follows the tape.
                    status.push_str(" · to live");
                }
                if cache.bars_covered + cache.bars_approximated < cache.bars_total {
                    status.push_str(&format!(
                        " · profile from {} of {} bars",
                        cache.bars_covered + cache.bars_approximated,
                        cache.bars_total
                    ));
                }
                if cache.bars_approximated > 0 {
                    // Venue candles joined without tape: their placement is
                    // approximated, and the profile says so at the point of
                    // reading, never in a tooltip.
                    status.push_str(&format!(
                        " · approximated from OHLC ({} of {} bars)",
                        cache.bars_approximated, cache.bars_total
                    ));
                }
                if cache.key.side_inferred {
                    // The delta and the buy/sell split rest on guessed
                    // aggressor sides — same label the footprint legend uses.
                    status.push_str(" · side inferred");
                }
                if outline_active {
                    // The object changed its look on its own; the app says
                    // why, in the same chain as everything else it does.
                    status.push_str(" · outline over heatmap");
                }
                if let Some(area) = value_area {
                    // POC/VAH/VAL price plates at the right edge of the range.
                    let labels = [
                        ("POC", area.poc, theme::POC),
                        ("VAH", area.vah.saturating_add(1), theme::TEXT_MUTED),
                        ("VAL", area.val, theme::TEXT_MUTED),
                    ];
                    for (name, bucket, color) in labels {
                        if name != "POC" && !payload.show_value_area {
                            continue;
                        }
                        if name == "POC" && !payload.show_poc {
                            continue;
                        }
                        let price = profile.bucket_price(bucket);
                        knockout_text(
                            painter,
                            egui::pos2(right + LABEL_OFFSET_PX, ctxt.scale.y(to_f64(price))),
                            egui::Align2::LEFT_CENTER,
                            &format!("{name} {price}"),
                            color,
                        );
                    }
                }
            }
            (None, Some(cache)) => status.push_str(match cache.empty {
                Some(FrvpEmpty::Blocked) => "feed reports no traded volume",
                _ => "no tape in range",
            }),
            // Not refreshed yet (first frame of a fresh object): say nothing
            // rather than guessing. A profile without a cache cannot exist —
            // the profile *lives in* the cache — but the tuple can't say so.
            (_, None) => {}
        }
        if !status.is_empty() {
            knockout_text(
                painter,
                egui::pos2(left, bottom + LABEL_OFFSET_PX),
                egui::Align2::LEFT_TOP,
                &status,
                theme::TEXT_MUTED,
            );
        }
    }
    fn hit_test(
        &self,
        _chart_rect: egui::Rect,
        points: &[egui::Pos2],
        position: egui::Pos2,
        radius_px: f32,
        ctxt: &DrawContext<'_>,
    ) -> bool {
        let Some(payload) = ctxt.payload.as_any().downcast_ref::<FrvpPayload>() else {
            return false;
        };
        if points.len() < 2 {
            return false;
        }
        let (left, right) = range_edges(payload, points, ctxt);
        let (top, bottom) = price_extent(payload, points, ctxt);
        let in_band = position.y >= top - radius_px && position.y <= bottom + radius_px;
        if !in_band {
            return false;
        }
        // Either edge grabs like a line; the histogram's own strip grabs as
        // an interior. The empty middle of a wide range stays click-through —
        // a full-rect hit would shadow every candle inside the range.
        let near_edge = (position.x - left).abs() <= radius_px
            || (position.x - right).abs() <= radius_px;
        let histogram_right = left + payload.width_frac * (right - left).max(1.0);
        let in_histogram = position.x >= left - radius_px && position.x <= histogram_right + radius_px;
        near_edge || in_histogram
    }

    /// The grab points live on the object, not on the anchors: the profile's
    /// price extent comes from the data, so the raw anchors sit at whatever
    /// heights they were dropped at — possibly far from the histogram, or
    /// off screen after a drag. A resize handle nobody can find is a resize
    /// handle that does not exist (the channel's rail handles set the
    /// precedent). Each handle keeps its anchor's x and centres itself in
    /// the drawn extent, so it is visible whenever the object is.
    fn handles(
        &self,
        chart_rect: egui::Rect,
        points: &[egui::Pos2],
        ctxt: &DrawContext<'_>,
    ) -> Option<Handles> {
        let payload = ctxt.payload.as_any().downcast_ref::<FrvpPayload>()?;
        if points.len() < 2 {
            return None;
        }
        // Centre of the *visible* slice of the extent, not of the extent
        // itself: a profile taller than the viewport would put its midpoint
        // off screen and hide the handles all over again.
        let (top, bottom) = price_extent(payload, points, ctxt);
        let visible_top = top.max(chart_rect.top());
        let visible_bottom = bottom.min(chart_rect.bottom());
        let mid = if visible_top <= visible_bottom {
            (visible_top + visible_bottom) / 2.0
        } else {
            chart_rect.center().y
        }
        .clamp(
            chart_rect.top() + HANDLE_EDGE_MARGIN_PX,
            chart_rect.bottom() - HANDLE_EDGE_MARGIN_PX,
        );
        Some(Handles::from_slice(&[
            egui::pos2(points[0].x, mid),
            egui::pos2(points[1].x, mid),
        ]))
    }

    /// A handle drag resizes the range: the grabbed edge's anchor follows the
    /// pointer in time and keeps its price — the profile ignores anchor
    /// prices, and letting them wander with every resize would scatter the
    /// anchors for no visible effect.
    fn drag_handle(
        &self,
        _chart_rect: egui::Rect,
        points: &[egui::Pos2],
        handle: usize,
        to: egui::Pos2,
        _ctxt: &DrawContext<'_>,
        // A range is two instants and no angle; nothing to hold level.
        _constrain: Constrain,
    ) -> Option<Handles> {
        if points.len() < 2 || handle >= 2 {
            return None;
        }
        let mut anchors = Handles::from_slice(points);
        anchors[handle].x = to.x;
        Some(anchors)
    }

    #[cfg(test)]
    fn test_geometry(&self) -> (Vec<egui::Pos2>, egui::Pos2) {
        (
            vec![egui::pos2(100.0, 100.0), egui::pos2(250.0, 200.0)],
            // On the left edge, inside the anchors' vertical span.
            egui::pos2(100.0, 150.0),
        )
    }
}

/// The Profile tab in the inspector: value-area fraction, histogram width,
/// the display toggles, and named presets through the host.
fn draw_profile_tab(ui: &mut egui::Ui, drawing: &mut Drawing, host: &mut dyn PresetHost) -> bool {
    let tool_id = drawing.tool.id();
    let Some(payload) = drawing.payload.as_any_mut().downcast_mut::<FrvpPayload>() else {
        return false;
    };
    let mut edited = false;

    let mut pct = i32::from(payload.value_area_pct);
    if ui
        .add(egui::Slider::new(&mut pct, 1..=100).text("value area %"))
        .changed()
    {
        payload.value_area_pct = u8::try_from(pct).unwrap_or(DEFAULT_VALUE_AREA_PCT);
        edited = true;
    }
    if ui
        .add(
            egui::Slider::new(&mut payload.width_frac, MIN_WIDTH_FRAC..=MAX_WIDTH_FRAC)
                .text("histogram width"),
        )
        .changed()
    {
        edited = true;
    }
    edited |= ui
        .checkbox(&mut payload.show_poc, "POC line")
        .changed();
    edited |= ui
        .checkbox(&mut payload.show_value_area, "value area")
        .changed();
    edited |= ui
        .checkbox(&mut payload.delta_coloring, "split rows by buy/sell")
        .changed();
    edited |= ui.checkbox(&mut payload.show_labels, "labels").changed();
    edited |= ui
        .checkbox(&mut payload.extend_right, "extend to newest bar")
        .on_hover_text(
            "The developing profile: the right edge follows the tape and \
             every new bar joins the fold. The anchors stay where you put \
             them, so switching this off restores the drawn range.",
        )
        .changed();
    edited |= ui
        .checkbox(&mut payload.outline_over_heatmap, "outline over liquidity map")
        .on_hover_text(
            "Over the liquidity map the profile draws as an outline, so the \
             map is never painted over. Off = always fill, and the two \
             layers compose.",
        )
        .changed();
    edited |= ui
        .checkbox(&mut payload.approximate_history, "approximate from candles")
        .on_hover_text(
            "Venue-history candles carry no tape; with this on their volume              is spread over each candle's high–low and the status line says              'approximated from OHLC'. Off = those bars contribute nothing.",
        )
        .changed();

    ui.separator();

    // Named presets via the host — the same contract the Fib tab uses.
    let customs = host.custom_preset_names(tool_id);
    ui.horizontal(|ui| {
        let selected_id = ui.id().with("frvp-preset");
        let mut selected: String =
            ui.data_mut(|data| data.get_temp(selected_id).unwrap_or_default());
        egui::ComboBox::from_id_salt(selected_id)
            .selected_text(if selected.is_empty() {
                "Custom presets"
            } else {
                selected.as_str()
            })
            .show_ui(ui, |ui| {
                for name in &customs {
                    ui.selectable_value(&mut selected, name.clone(), name);
                }
            });
        ui.data_mut(|data| data.insert_temp(selected_id, selected.clone()));
        let has_selection = customs.contains(&selected);
        if ui
            .add_enabled(has_selection, egui::Button::new("Apply").small())
            .clicked()
            && let Some(value) = host.load_custom_preset(tool_id, &selected)
            && payload.import_preset(&value)
        {
            edited = true;
        }
        if ui
            .add_enabled(has_selection, egui::Button::new("Delete").small())
            .clicked()
        {
            host.delete_custom_preset(tool_id, &selected);
        }
    });
    ui.horizontal(|ui| {
        let name_id = ui.id().with("frvp-preset-name");
        let mut name: String = ui.data_mut(|data| data.get_temp(name_id).unwrap_or_default());
        ui.add(
            egui::TextEdit::singleline(&mut name)
                .hint_text("Preset name")
                .desired_width(120.0),
        );
        let trimmed = name.trim();
        if ui
            .add_enabled(!trimmed.is_empty(), egui::Button::new("Save preset").small())
            .clicked()
            && let Some(value) = payload.export_preset()
        {
            host.save_custom_preset(tool_id, trimmed, value, true);
        }
        ui.data_mut(|data| data.insert_temp(name_id, name));
    });

    edited
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chart::PriceScale;
    use crate::drawings::{ChartPoint, ValueUnit};

    /// The handles sit on the drawn object — each anchor's x at the visible
    /// extent's midpoint — and a handle drag moves only that edge's bar,
    /// never the anchors' prices. This is what makes the resize findable
    /// after any drag: the grab points are wherever the histogram is.
    #[test]
    fn handles_ride_the_visible_object_and_resize_in_time_only() {
        let chart_rect =
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 400.0));
        let scale = PriceScale::from_range(90.0, 110.0, 0.0, 400.0);
        let payload = FrvpPayload::default();
        let anchors = [ChartPoint::at(10.0, 104.0), ChartPoint::at(30.0, 96.0)];
        let points = [egui::pos2(100.0, 120.0), egui::pos2(300.0, 280.0)];
        let ctxt = DrawContext {
            payload: &payload,
            anchors: &anchors,
            scale: &scale,
            unit: ValueUnit::Price,
            primary_band: true,
            style: DrawingStyle::default(),
            selected: true,
            halo: false,
        };

        let handles = TOOL.handles(chart_rect, &points, &ctxt).expect("overridden");
        // No cache: the extent falls back to the anchors' heights, midpoint
        // 200. Each handle keeps its own anchor's x.
        assert_eq!(handles.as_slice(), &[
            egui::pos2(100.0, 200.0),
            egui::pos2(300.0, 200.0)
        ]);

        // Dragging the right handle left shrinks the range: only that
        // anchor's x moves, both ys (prices) stay put.
        let dragged = TOOL
            .drag_handle(chart_rect, &points, 1, egui::pos2(180.0, 350.0), &ctxt, Constrain::Free)
            .expect("tool owns the gesture");
        assert_eq!(dragged.as_slice(), &[
            egui::pos2(100.0, 120.0),
            egui::pos2(180.0, 280.0)
        ]);

        // An extent taller than the viewport clamps the handles into the
        // visible band instead of hiding them off screen.
        let tall = [egui::pos2(100.0, -900.0), egui::pos2(300.0, 1500.0)];
        let clamped = TOOL.handles(chart_rect, &tall, &ctxt).expect("overridden");
        assert!(
            clamped
                .iter()
                .all(|handle| chart_rect.expand(-HANDLE_EDGE_MARGIN_PX + 0.5).contains(*handle)),
            "handles stay inside the band: {clamped:?}"
        );
    }

    /// The fill→silhouette cut and the developing edge both project through
    /// the anchors' own affine bar→x map; anchors sharing a bar have no
    /// slope and must answer `None`, never a division blow-up.
    #[test]
    fn bar_x_extends_the_anchors_affine_map() {
        let anchors = [ChartPoint::at(10.0, 100.0), ChartPoint::at(20.0, 105.0)];
        let points = [egui::pos2(100.0, 0.0), egui::pos2(300.0, 0.0)];
        // 10 bars over 200px → 20px per bar; bar 15 lands halfway.
        assert_eq!(bar_x(&points, &anchors, 15.0), Some(200.0));
        // Extrapolation works the same both ways.
        assert_eq!(bar_x(&points, &anchors, 25.0), Some(400.0));
        assert_eq!(bar_x(&points, &anchors, 5.0), Some(0.0));

        let flat = [ChartPoint::at(10.0, 100.0), ChartPoint::at(10.0, 105.0)];
        assert_eq!(bar_x(&points, &flat, 15.0), None);
    }

    #[test]
    fn preset_round_trip_excludes_the_cache() {
        let mut payload = FrvpPayload {
            value_area_pct: 68,
            width_frac: 0.5,
            show_value_area: false,
            show_poc: true,
            delta_coloring: true,
            show_labels: false,
            extend_right: true,
            outline_over_heatmap: false,
            approximate_history: false,
            cache: Some(FrvpCache {
                key: FrvpCacheKey {
                    start_slot: 1,
                    end_slot: 5,
                    group: Decimal::ONE,
                    timeline_revision: 3,
                    closed_len: 10,
                    include_partial: false,
                    partial_snapshot: 0,
                    value_area_pct: 68,
                    blocked: false,
                    side_inferred: false,
                    approximate: true,
                },
                profile: None,
                empty: Some(FrvpEmpty::NoTape),
                bars_covered: 0,
                bars_approximated: 0,
                bars_total: 5,
                heat_first_slot: None,
            }),
        };
        let exported = payload.export_preset().expect("frvp exports its preset");
        // No derived state travels with a preset.
        assert!(!exported.to_string().contains("cache"));

        let mut restored = FrvpPayload::default();
        assert!(restored.import_preset(&exported));
        assert_eq!(restored.value_area_pct, 68);
        assert!(restored.delta_coloring);
        assert!(
            !restored.outline_over_heatmap,
            "the over-heatmap choice travels with the preset"
        );
        assert!(restored.cache.is_none(), "a preset never installs a cache");

        // Equality ignores the cache: dropping it changes nothing.
        let with_cache = payload.clone();
        payload.cache = None;
        assert!(payload.eq_dyn(&with_cache));
    }

    #[test]
    fn import_rejects_bad_versions_and_bad_fractions() {
        let mut payload = FrvpPayload::default();
        let mut bad = payload.export_preset().unwrap();
        if let toml::Value::Table(table) = &mut bad {
            table.insert("version".into(), toml::Value::Integer(99));
        }
        assert!(!payload.import_preset(&bad));

        let mut zero_pct = payload.export_preset().unwrap();
        if let toml::Value::Table(table) = &mut zero_pct {
            table.insert("value_area_pct".into(), toml::Value::Integer(0));
        }
        assert!(!payload.import_preset(&zero_pct));
    }
}
