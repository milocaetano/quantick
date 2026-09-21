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
//! - every profile names how many bars it folded, so one `vol` figure can be
//!   compared with another's without counting candles by eye;
//! - partial coverage is spoken (`N of M bars`), never blended away;
//! - the range that folds is the rectangle that was drawn — an anchor's
//!   integer bar coordinate is a candle's centre, the same convention the
//!   viewport paints with (see [`crate::frvp`]);
//! - a cap-coarsened profile names its effective row width, like the
//!   footprint legend does.

use eframe::egui;
use egui_phosphor::regular as icons;
#[cfg(test)]
use quantick_anchored_studies::FrvpCacheKey;
use quantick_anchored_studies::{LevelPrices, ProfileOutput, RangeProfile as FrvpCache};
use quantick_engine::{ValueArea, VolumeProfile};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::any::Any;

use super::measure_core::MEASURE_FAMILY;
use super::{
    Constrain, DrawContext, Drawing, DrawingPayload, DrawingStyle, DrawingToolImpl, Handles,
    PresetHost,
};
use crate::chart::to_f64;
use crate::theme::CASING;

mod geometry;
mod paint;

use paint::{FrvpPass, paint_body};

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
/// Silhouette ink weights: the value area keeps its by-weight reading in an
/// axis the heatmap does not occupy (stroke width + brightness), replacing
/// the fill alphas that a variable background destroys.
const OUTLINE_IN_VA_PX: f32 = 1.9;
const OUTLINE_OUT_VA_PX: f32 = 1.25;
const OUTLINE_OUT_VA_BRIGHTNESS: f32 = 0.6;

/// Why the payload holds no profile, spoken to the trader as-is.
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
    /// Render-only heat boundary; never part of the computational key.
    pub heat_first_slot: Option<usize>,
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
            heat_first_slot: None,
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
    /// The clone carries the config and **drops the cache**.
    ///
    /// Every undo step snapshots every drawing, twice per edit, as deep as the
    /// history goes. Copying the fold into all of them would hold a capped
    /// ladder, a second capped map and a batch of pending spreads per profile
    /// per step — for state no undo can use, since the cache is excluded from
    /// equality and the next refresh rebuilds it regardless. The frame after
    /// an undo pays one re-fold; the alternative was megabytes of dead history
    /// per object.
    fn clone_box(&self) -> Box<dyn DrawingPayload> {
        Box::new(Self {
            cache: None,
            heat_first_slot: None,
            ..self.clone()
        })
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
        && let Some(cache) = payload.cache.as_ref().map(FrvpCache::output)
        && let [a, b, ..] = ctxt.anchors
    {
        let bar_span = b.bar - a.bar;
        if bar_span.abs() > f32::EPSILON {
            let slot_width = (points[1].x - points[0].x) / bar_span;
            // The right boundary of the newest covered slot: its centre is
            // `end_slot`, its trailing edge half a slot further — the same
            // centre-is-the-integer convention the fold reads anchors with.
            #[allow(clippy::cast_precision_loss)]
            let live_x = points[0].x + (cache.key.end_slot as f32 + 0.5 - a.bar) * slot_width;
            right = right.max(live_x);
        }
    }
    (left, right)
}

/// The status line under a range: what the profile is made of, in the
/// footprint legend's language. Pure, so what the object owes the trader can
/// be asserted in a test rather than rest on someone reading a screenshot.
///
/// The bar count is unconditional, and that is the point. Printing it only
/// when something was *wrong* left the ordinary case — an exact, fully
/// covered range — showing a `vol` figure with no denominator, so two
/// profiles side by side could not be told apart between "different window"
/// and "different market". A total without its span is not a comparable
/// number.
fn status_line(
    profile: &VolumeProfile,
    cache: &ProfileOutput<'_>,
    payload: &FrvpPayload,
    outline_active: bool,
) -> String {
    let mut status = format!(
        "vol {} · Δ {} · rows {}",
        fmt_qty(profile.total_volume()),
        fmt_signed_qty(profile.total_delta()),
        profile.group()
    );
    if profile.is_aggregated() {
        // The rows are coarser than the capture grid — spoken, like the
        // footprint legend speaks its coarsening.
        status.push_str(" · grouped");
    }
    status.push_str(&format!(
        " · {} {}",
        cache.bars_total,
        if cache.bars_total == 1 { "bar" } else { "bars" }
    ));
    if payload.extend_right {
        // The developing mode: this profile follows the tape.
        status.push_str(" · to live");
    }
    if cache.key.include_partial {
        // One of those bars is still forming, so it holds only as much of
        // its interval as has traded so far. Without this the same range
        // reads differently a second later for no stated reason.
        status.push_str(" · incl. forming bar");
    }
    if cache.bars_partly_covered > 0 {
        // The tape's first bar opened mid-interval, so it holds less than the
        // interval it occupies — and where a venue prefix exists, the candle
        // that did cover the rest was dropped at the seam. The shortfall is
        // real and unmeasurable from here, so it is named, not topped up.
        //
        // "first tape bar", not "seam bar": a pane with no venue history has
        // no seam, and its first bar is short all the same.
        status.push_str(" · first tape bar partly covered");
    }
    if cache.folding {
        // Still folding. The histogram on screen is honest about the bars it
        // has and says nothing about the ones it does not — reading it as the
        // whole range would be reading a number that is still moving, so the
        // count that is still to come is stated rather than implied by a
        // spinner. Said *before* the coverage caveat below, which counts the
        // same unfolded bars as "missing" until the fold reaches them.
        //
        // "loading", not "folding": the fold is the engine's word for what is
        // happening, and this line is read by someone in their first week. The
        // two sentences it sits beside — `profile from N of M bars` for
        // coverage, this one for progress — keep different verbs on purpose:
        // they mean opposite things about the bars they leave out.
        status.push_str(&format!(
            " · loading {} of {} bars",
            cache.bars_folded, cache.bars_total
        ));
    } else if cache.bars_covered + cache.bars_approximated < cache.bars_total {
        status.push_str(&format!(
            " · profile from {} of {} bars",
            cache.bars_covered + cache.bars_approximated,
            cache.bars_total
        ));
    }
    if cache.bars_approximated > 0 {
        // Venue candles joined without tape: their placement is approximated,
        // and the profile says so at the point of reading, never in a
        // tooltip. Phrased as a count of *approximated* bars — the old
        // "approximated from OHLC (85 of 85 bars)" read as a coverage
        // reassurance when it meant the exact opposite.
        status.push_str(&format!(
            " · {} of {} approximated from OHLC",
            cache.bars_approximated, cache.bars_total
        ));
    }
    if cache.key.side_inferred {
        // The delta and the buy/sell split rest on guessed aggressor sides —
        // same label the footprint legend uses.
        status.push_str(" · side inferred");
    }
    if outline_active {
        // The object changed its look on its own; the app says why, in the
        // same chain as everything else it does.
        status.push_str(" · outline over heatmap");
    }
    status
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

/// The status line, slid left when it would otherwise run past the right of
/// `bounds`.
///
/// It starts at the range's left edge and grows rightwards, so a range near
/// the newest bar had its tail clipped by the pane edge — and the tail is
/// where every caveat lives. A `vol` figure whose bar count and "approximated
/// from OHLC" notice were cut off reads as a complete, exact number, which is
/// the one thing this label must never do. Truncation that hides a caveat is
/// worse than a label that leaves its range.
/// Where a left-aligned label `width` px wide must start to stay inside
/// `bounds`, given where it would rather start.
///
/// The left edge wins when the label is wider than the pane: the head of the
/// line (`vol`, `Δ`) is what a truncated read must keep, and losing the tail
/// to the right edge is exactly the failure this clamp exists to prevent.
fn clamped_label_x(preferred_x: f32, width: f32, bounds: egui::Rect) -> f32 {
    (preferred_x.min(bounds.right() - width)).max(bounds.left())
}

/// The same clamp down the price axis, and for the same reason.
///
/// The status line hangs under the object's own bottom, and a profile's bottom
/// is where its *data* ends — a range folding a long history spans more price
/// than the window shows, so the line was painted below the pane and every
/// caveat on it went with it. That is worst exactly where it matters most: the
/// longest ranges are the ones that fold in stages, and the ones whose
/// coverage is partial.
///
/// Only clamped while some part of the object is on screen: a profile scrolled
/// entirely out of view keeps its label out of view too, because a caption
/// with no object under it names something the trader cannot see.
fn clamped_label_y(preferred_y: f32, span: (f32, f32), height: f32, bounds: egui::Rect) -> f32 {
    let (top, bottom) = span;
    if bottom < bounds.top() || top > bounds.bottom() {
        return preferred_y;
    }
    (preferred_y.min(bounds.bottom() - height)).max(bounds.top())
}

/// Lays the text out exactly as many times as [`knockout_text`] does — the
/// clamp reads its width off the galley it is about to paint, so keeping the
/// label on screen costs no extra layout on the per-frame path.
fn knockout_text_within(
    painter: &egui::Painter,
    pos: egui::Pos2,
    text: &str,
    color: egui::Color32,
    bounds: egui::Rect,
    object_span: (f32, f32),
) {
    let font = egui::FontId::proportional(LABEL_SIZE_PX);
    let ink_galley = painter.layout_no_wrap(text.to_owned(), font.clone(), color);
    let casing_galley = painter.layout_no_wrap(text.to_owned(), font, CASING);
    let x = clamped_label_x(pos.x, ink_galley.size().x, bounds);
    let y = clamped_label_y(pos.y, object_span, ink_galley.size().y, bounds);
    let corner = egui::Align2::LEFT_TOP
        .anchor_size(egui::pos2(x, y), ink_galley.size())
        .min;
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
    if let Some((profile, _)) = payload
        .cache
        .as_ref()
        .and_then(|cache| cache.output().profile)
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

/// A signed quantity, with the `+` written out.
///
/// Delta's sign *is* the reading — buyers or sellers took the range — and a
/// bare `214.24` against `-214.24` puts that entire meaning in a three-pixel
/// hyphen, at 10 px, in one flat muted colour, with `delta_coloring` off by
/// default. Two people already read the wrong side off this label. Zero is
/// written unsigned: it took neither side.
fn fmt_signed_qty(value: Decimal) -> String {
    let body = fmt_qty(value);
    if value > Decimal::ZERO {
        format!("+{body}")
    } else {
        body
    }
}

impl DrawingToolImpl for FixedRangeProfile {
    /// Floor to ceiling: the anchors say *when*, the drawing covers every
    /// price on screen. See [`DrawingToolImpl::painted_bounds`].
    fn painted_bounds(&self, anchors: egui::Rect, chart: egui::Rect) -> egui::Rect {
        egui::Rect::from_min_max(
            egui::pos2(anchors.left(), chart.top()),
            egui::pos2(anchors.right(), chart.bottom()),
        )
    }
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
        paint_body(painter, chart_rect, style, points, ctxt, FrvpPass::Over);
    }

    /// The histogram goes under the candles — see [`FrvpPass`].
    fn paint_under(
        &self,
        painter: &egui::Painter,
        chart_rect: egui::Rect,
        style: DrawingStyle,
        points: &[egui::Pos2],
        ctxt: &DrawContext<'_>,
    ) {
        paint_body(painter, chart_rect, style, points, ctxt, FrvpPass::Under);
    }
    fn hit_test(
        &self,
        chart_rect: egui::Rect,
        points: &[egui::Pos2],
        position: egui::Pos2,
        radius_px: f32,
        ctxt: &DrawContext<'_>,
    ) -> bool {
        geometry::hit_profile(chart_rect, points, position, radius_px, ctxt)
    }

    fn handle_hit_radius(&self, radius_px: f32, ctxt: &DrawContext<'_>) -> Option<f32> {
        ctxt.selected.then_some(radius_px.min(
            super::SELECTED_ANCHOR_RADIUS_PX
                + super::SELECTED_ANCHOR_RING_WIDTH_PX / 2.0
                + geometry::HIT_SLOP_PX,
        ))
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
    edited |= ui.checkbox(&mut payload.show_poc, "POC line").changed();
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
        .checkbox(
            &mut payload.outline_over_heatmap,
            "outline over liquidity map",
        )
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
            .add_enabled(
                !trimmed.is_empty(),
                egui::Button::new("Save preset").small(),
            )
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
    mod levels;
    mod precise_hit;
    use crate::chart::PriceScale;
    use crate::drawings::{ChartPoint, ValueUnit};

    /// One venue candle's worth of profile — enough to carry a `vol` figure
    /// into the status line.
    fn some_profile() -> VolumeProfile {
        let bar = quantick_engine::Bar {
            open_time: 0,
            close_time: 60_000,
            open: Decimal::from(100),
            high: Decimal::from(104),
            low: Decimal::from(100),
            close: Decimal::from(102),
            buy_volume: Decimal::from(6),
            sell_volume: Decimal::from(4),
            trade_count: 10,
        };
        let ladder = quantick_engine::BarFootprint::approximated(
            &bar,
            Decimal::ONE,
            quantick_engine::DEFAULT_LEVEL_CAP,
        )
        .expect("the candle traded");
        VolumeProfile::merge(vec![&ladder], quantick_engine::DEFAULT_LEVEL_CAP)
            .expect("one ladder folds")
    }

    /// A real owner for payload/geometry tests, refreshed through the public domain path.
    fn refreshed_cache(rows: &[(i64, i64)], last_slot: usize) -> FrvpCache {
        use quantick_anchored_studies::{ProfileInputs, ProfileRequest};
        use quantick_engine::{BarSpec, Side, Trade, bar_registry::BarConfiguration};
        let mut builder = BarConfiguration::from(BarSpec::Tick(1)).build();
        let mut footprint = quantick_engine::FootprintBuilder::new(Decimal::ONE, 4096);
        let mut first = None;
        for &(price, quantity) in rows {
            let trade = Trade {
                agg_id: 1,
                timestamp_ms: 0,
                price: Decimal::from(price),
                quantity: Decimal::from(quantity),
                side: Side::Buy,
            };
            first = builder.push(&trade);
            footprint.push(&trade);
        }
        let bars = vec![first.expect("fixture trade closes a bar"); last_slot + 1];
        let ladder = footprint.close().unwrap();
        let mut owned = None;
        FrvpCache::refresh(
            &mut owned,
            ProfileRequest {
                min_bar: 0.0,
                max_bar: f32::MAX,
                extend_right: true,
                approximate_history: false,
                value_area_pct: DEFAULT_VALUE_AREA_PCT,
            },
            &ProfileInputs {
                closed: &bars,
                ladders: &[ladder],
                prefix: &[],
                partial_ladder: None,
                group: Decimal::ONE,
                series_revision: 0,
                partial_version: 0,
                blocked: false,
                side_inferred: false,
                partial_bucket_slot: None,
                budget: 1500,
            },
        );
        owned.expect("refresh installs a result")
    }

    fn cache_for(total: usize, covered: usize, approximated: usize) -> ProfileOutput<'static> {
        ProfileOutput {
            key: FrvpCacheKey {
                start_slot: 0,
                end_slot: total.saturating_sub(1),
                group: Decimal::ONE,
                series_revision: 0,

                include_partial: false,
                partial_snapshot: 0,
                value_area_pct: DEFAULT_VALUE_AREA_PCT,
                blocked: false,
                side_inferred: false,
                approximate: approximated > 0,
                partly_covered: false,
            },
            profile: None,
            empty: None,
            bars_covered: covered,
            closed_covered: covered,
            bars_approximated: approximated,
            bars_partly_covered: 0,
            bars_folded: total,
            bars_total: total,
            folding: false,
        }
    }

    /// The same cache mid-fill: `folded` of `total` bars in, the fold still
    /// running over the rest.
    fn folding_cache(total: usize, folded: usize) -> ProfileOutput<'static> {
        ProfileOutput {
            folding: true,
            bars_folded: folded,
            ..cache_for(total, folded, 0)
        }
    }

    /// A profile still filling says so, and says how far it has got. Without
    /// this the histogram would read as the whole range's while it is still a
    /// prefix of it — a number that moves under the trader with nothing on
    /// screen admitting it.
    #[test]
    fn status_line_counts_out_a_fold_still_running() {
        let profile = some_profile();
        let payload = FrvpPayload::default();
        let status = status_line(&profile, &folding_cache(25_003, 4_000), &payload, false);
        assert!(
            status.contains(" · loading 4000 of 25003 bars"),
            "a fold in flight names its progress: {status}"
        );
        // And it does not *also* claim partial coverage: the bars it has not
        // reached are late, not missing.
        assert!(!status.contains("profile from"), "{status}");
    }

    /// The fold finishing turns the progress line off — a complete profile
    /// must not keep counting.
    #[test]
    fn status_line_drops_the_progress_once_the_fold_is_done() {
        let profile = some_profile();
        let payload = FrvpPayload::default();
        let status = status_line(&profile, &cache_for(85, 85, 0), &payload, false);
        assert!(!status.contains("loading"), "{status}");
    }

    /// The regression that started this: an exact, fully covered profile used
    /// to print `vol` with no denominator, so two profiles side by side could
    /// not be told apart between "different window" and "different market".
    #[test]
    fn status_line_always_names_the_bar_count() {
        let profile = some_profile();
        let payload = FrvpPayload::default();
        let status = status_line(&profile, &cache_for(85, 85, 0), &payload, false);
        assert!(
            status.contains(" · 85 bars"),
            "a complete range still owes its span: {status}"
        );
        assert!(status.starts_with("vol "), "{status}");
        // Nothing is claimed about coverage or approximation when neither
        // applies — the count alone is the whole story.
        assert!(!status.contains("profile from"), "{status}");
        assert!(!status.contains("approximated"), "{status}");
    }

    #[test]
    fn status_line_counts_one_bar_in_the_singular() {
        let profile = some_profile();
        let payload = FrvpPayload::default();
        let status = status_line(&profile, &cache_for(1, 1, 0), &payload, false);
        assert!(status.contains(" · 1 bar"), "{status}");
        assert!(!status.contains("1 bars"), "{status}");
    }

    /// The approximation caveat reads as a caveat. `approximated from OHLC
    /// (85 of 85 bars)` parsed as a coverage reassurance while meaning that
    /// every single row placement was a guess.
    #[test]
    fn status_line_names_approximated_bars_as_a_caveat() {
        let profile = some_profile();
        let payload = FrvpPayload::default();
        let status = status_line(&profile, &cache_for(85, 0, 85), &payload, false);
        assert!(status.contains(" · 85 bars"), "the span, always: {status}");
        assert!(
            status.contains(" · 85 of 85 approximated from OHLC"),
            "{status}"
        );
    }

    /// A range still under construction says so: the same anchors read
    /// differently a second later, and that is not a mystery the trader
    /// should have to solve.
    #[test]
    fn status_line_speaks_the_forming_bar() {
        let profile = some_profile();
        let payload = FrvpPayload::default();
        let mut cache = cache_for(30, 30, 0);
        assert!(!status_line(&profile, &cache, &payload, false).contains("forming"));
        cache.key.include_partial = true;
        let status = status_line(&profile, &cache, &payload, false);
        assert!(status.contains(" · incl. forming bar"), "{status}");
    }

    /// Delta's sign is the whole reading, so it is written, not implied by
    /// the absence of a hyphen. This is the label that sent two readers to
    /// the wrong side of the tape.
    #[test]
    fn delta_writes_its_sign_on_both_sides_of_zero() {
        assert_eq!(fmt_signed_qty(Decimal::from(214)), "+214.00");
        assert_eq!(fmt_signed_qty(Decimal::from(-214)), "-214.00");
        assert_eq!(fmt_signed_qty(Decimal::ZERO), "0.00", "zero took no side");
        // Volume is unsigned and must not grow a `+`.
        assert_eq!(fmt_qty(Decimal::from(985)), "985.00");

        let payload = FrvpPayload::default();
        let status = status_line(&some_profile(), &cache_for(85, 85, 0), &payload, false);
        assert!(
            status.contains("Δ +"),
            "a buy-side delta is signed: {status}"
        );
    }

    /// The status line slides left to keep its tail — where every caveat
    /// lives — inside the pane, and gives up the tail only when the label is
    /// wider than the pane itself.
    #[test]
    fn a_label_near_the_right_edge_slides_in_instead_of_being_clipped() {
        let bounds = egui::Rect::from_min_max(egui::pos2(100.0, 0.0), egui::pos2(900.0, 400.0));
        // Comfortably inside: left alone.
        assert_eq!(clamped_label_x(200.0, 300.0, bounds), 200.0);
        // Would overrun the right edge: slid back so it ends exactly on it.
        assert_eq!(clamped_label_x(800.0, 300.0, bounds), 600.0);
        // Flush against the right edge is not a slide.
        assert_eq!(clamped_label_x(600.0, 300.0, bounds), 600.0);
        // Wider than the whole pane: the head wins, the tail overflows.
        assert_eq!(clamped_label_x(400.0, 1000.0, bounds), 100.0);
        // Never dragged left of the pane by a range anchored off-screen.
        assert_eq!(clamped_label_x(-50.0, 200.0, bounds), 100.0);
    }

    /// The same rule down the price axis. A profile folding a long history
    /// spans more price than the window shows, so its bottom — and the whole
    /// caveat line hanging off it — used to sit below the pane. The label
    /// follows the object into view; an object entirely out of view keeps its
    /// label out with it.
    #[test]
    fn a_label_under_a_taller_than_the_window_profile_slides_into_view() {
        let bounds = egui::Rect::from_min_max(egui::pos2(100.0, 0.0), egui::pos2(900.0, 400.0));
        // Object crossing the pane, label below the bottom edge: slid up so
        // it ends exactly on it.
        assert_eq!(clamped_label_y(900.0, (-500.0, 880.0), 12.0, bounds), 388.0);
        // Comfortably inside: left alone.
        assert_eq!(clamped_label_y(200.0, (150.0, 260.0), 12.0, bounds), 200.0);
        // The object is entirely below the pane: no caption for something
        // that is not on screen.
        assert_eq!(clamped_label_y(900.0, (500.0, 880.0), 12.0, bounds), 900.0);
        // And entirely above it.
        assert_eq!(
            clamped_label_y(-100.0, (-500.0, -120.0), 12.0, bounds),
            -100.0
        );
    }

    /// The developing mode's live edge lands on the newest covered slot's
    /// trailing edge — half a slot past its centre, the same
    /// centre-is-the-integer convention the fold reads anchors with. A full
    /// slot past it drew the range one candle wider than it folded.
    #[test]
    fn the_developing_edge_stops_at_the_newest_slots_trailing_edge() {
        let mut payload = FrvpPayload {
            extend_right: true,
            ..FrvpPayload::default()
        };
        // end_slot 30 while the anchors only reach bar 20.
        payload.cache = Some(refreshed_cache(&[(100, 1)], 30));
        let scale = PriceScale::from_range(90.0, 110.0, 0.0, 400.0);
        let anchors = [ChartPoint::at(10.0, 104.0), ChartPoint::at(20.0, 96.0)];
        // 10 bars over 200px → 20px per slot, bar 10 at x=100.
        let points = [egui::pos2(100.0, 120.0), egui::pos2(300.0, 280.0)];
        let ctxt = DrawContext {
            payload: &payload,
            anchors: &anchors,
            scale: &scale,
            px_per_bar: 20.0,
            unit: ValueUnit::Price,
            primary_band: true,
            style: DrawingStyle::default(),
            selected: false,
            halo: false,
            content_editing: false,
        };
        let (left, right) = range_edges(&payload, &points, &ctxt);
        assert_eq!(left, 100.0, "the drawn left edge is untouched");
        // Slot 30's centre is at x = 100 + (30-10)*20 = 500; its trailing
        // edge is half a slot further.
        assert_eq!(right, 510.0);
    }

    /// The short first bar's shortfall is named, not topped up. Without this
    /// the range read as fully covered while missing whatever traded in that
    /// interval before the app connected — 36% of a minute, 94% of an hour,
    /// measured on a live BTCUSDT connect. It is worded for the tape, not for
    /// the seam: a pane with no venue history has no seam and a short first
    /// bar all the same.
    #[test]
    fn status_line_speaks_a_partly_covered_first_tape_bar() {
        let profile = some_profile();
        let payload = FrvpPayload::default();
        let mut cache = cache_for(3, 3, 0);
        assert!(!status_line(&profile, &cache, &payload, false).contains("partly covered"));

        cache.bars_partly_covered = 1;
        cache.key.partly_covered = true;
        let status = status_line(&profile, &cache, &payload, false);
        assert!(
            status.contains(" · first tape bar partly covered"),
            "{status}"
        );
        assert!(!status.contains("seam"), "no seam is claimed: {status}");
        // It is a caveat about one bar, not a coverage shortfall: all three
        // bars still contributed, so the "N of M" clause stays quiet.
        assert!(!status.contains("profile from"), "{status}");
        assert!(status.contains(" · 3 bars"), "{status}");
    }

    /// Partial coverage keeps its own explicit `N of M`, on top of the span.
    #[test]
    fn status_line_still_speaks_partial_coverage() {
        let profile = some_profile();
        let payload = FrvpPayload::default();
        let status = status_line(&profile, &cache_for(85, 40, 0), &payload, false);
        assert!(status.contains(" · 85 bars"), "{status}");
        assert!(status.contains(" · profile from 40 of 85 bars"), "{status}");
    }

    /// The handles sit on the drawn object — each anchor's x at the visible
    /// extent's midpoint — and a handle drag moves only that edge's bar,
    /// never the anchors' prices. This is what makes the resize findable
    /// after any drag: the grab points are wherever the histogram is.
    #[test]
    fn handles_ride_the_visible_object_and_resize_in_time_only() {
        let chart_rect = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 400.0));
        let scale = PriceScale::from_range(90.0, 110.0, 0.0, 400.0);
        let payload = FrvpPayload::default();
        let anchors = [ChartPoint::at(10.0, 104.0), ChartPoint::at(30.0, 96.0)];
        let points = [egui::pos2(100.0, 120.0), egui::pos2(300.0, 280.0)];
        let ctxt = DrawContext {
            payload: &payload,
            anchors: &anchors,
            scale: &scale,
            px_per_bar: 20.0,
            unit: ValueUnit::Price,
            primary_band: true,
            style: DrawingStyle::default(),
            selected: true,
            halo: false,
            content_editing: false,
        };

        let handles = TOOL
            .handles(chart_rect, &points, &ctxt)
            .expect("overridden");
        // No cache: the extent falls back to the anchors' heights, midpoint
        // 200. Each handle keeps its own anchor's x.
        assert_eq!(
            handles.as_slice(),
            &[egui::pos2(100.0, 200.0), egui::pos2(300.0, 200.0)]
        );

        // Dragging the right handle left shrinks the range: only that
        // anchor's x moves, both ys (prices) stay put.
        let dragged = TOOL
            .drag_handle(
                chart_rect,
                &points,
                1,
                egui::pos2(180.0, 350.0),
                &ctxt,
                Constrain::Free,
            )
            .expect("tool owns the gesture");
        assert_eq!(
            dragged.as_slice(),
            &[egui::pos2(100.0, 120.0), egui::pos2(180.0, 280.0)]
        );

        // An extent taller than the viewport clamps the handles into the
        // visible band instead of hiding them off screen.
        let tall = [egui::pos2(100.0, -900.0), egui::pos2(300.0, 1500.0)];
        let clamped = TOOL.handles(chart_rect, &tall, &ctxt).expect("overridden");
        assert!(
            clamped.iter().all(|handle| chart_rect
                .expand(-HANDLE_EDGE_MARGIN_PX + 0.5)
                .contains(*handle)),
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
            heat_first_slot: None,
            cache: Some(refreshed_cache(&[(100, 1)], 5)),
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
