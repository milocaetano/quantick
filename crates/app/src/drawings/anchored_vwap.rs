//! Anchored VWAP: one anchor picks the bar the average starts at, the object
//! shows the volume-weighted average price from that bar to the live edge,
//! with up to three standard-deviation band pairs.
//!
//! The anchor carries *where* (a bar on the tape); the math lives in the
//! `indicators` crate's [`AnchoredVwap`](quantick_indicators::native::AnchoredVwap)
//! kernel — the same golden-tested code path a scripted consumer would run,
//! never a fork of it. `crate::avwap::refresh` replays that kernel into the
//! payload's cache whenever something it depends on changes; this file only
//! projects and paints the cached rows.
//!
//! Honesty rules the paint keeps:
//! - a bar the kernel answered `na` for (zero traded volume since the anchor)
//!   breaks the line rather than being bridged over;
//! - the anchor marker sits on the bar that actually seeds the sums — the
//!   candle the click landed on, at its own time;
//! - a pane that cannot name its bar width (`px_per_bar <= 0`) draws only
//!   the anchor, never a guessed series.

use eframe::egui;
use egui_phosphor::regular as icons;
use quantick_indicators::SourceId;
use serde::{Deserialize, Serialize};
use std::any::Any;

use super::{
    DrawContext, Drawing, DrawingPayload, DrawingStyle, DrawingToolImpl, PresetHost, ToolShortcut,
    drawing_stroke,
};

pub(super) static TOOL: AnchoredVwapTool = AnchoredVwapTool;

pub(super) struct AnchoredVwapTool;

const PRESET_FORMAT_VERSION: u32 = 1;
/// Number of optional σ-band pairs — the kernel's own count.
pub const AVWAP_BAND_PAIRS: usize = 3;
/// Plot values per cached row: vwap + (upper, lower) per pair.
pub const AVWAP_ROW_WIDTH: usize = 1 + 2 * AVWAP_BAND_PAIRS;
/// Default σ multipliers, pair 1..=3 (TradingView's 1σ/2σ/3σ convention).
pub const AVWAP_DEFAULT_MULTS: [f64; AVWAP_BAND_PAIRS] = [1.0, 2.0, 3.0];
/// Band *lines* fade with distance from the vwap; the trader's colour keeps
/// carrying the object, so these are alphas over `style.color`, not hues.
const BAND_LINE_ALPHA: [f32; AVWAP_BAND_PAIRS] = [0.55, 0.40, 0.30];
/// Band fills as fractions of the style's own `fill_alpha`, so the existing
/// fill slider governs the whole stack and three pairs never add up to paint.
const BAND_FILL_FRAC: [f32; AVWAP_BAND_PAIRS] = [1.0, 0.65, 0.40];
/// Band line width relative to the vwap's own stroke: the hairline of an
/// annotation — the bands frame, the vwap leads.
const BAND_WIDTH_FRAC: f32 = 0.5;
/// The anchor marker: a ring on the seed bar, a shade wider than the
/// selection handles (3.5 px) so the two never blur into one blob.
const ANCHOR_RADIUS_PX: f32 = 4.0;
const ANCHOR_RING_PX: f32 = 1.5;
/// A fresh anchored VWAP is a derived series, not a note: it opens in the
/// palette's reserved cyan, heavier than the annotation hairline, and with
/// enough fill for the one default band pair to actually read.
const AVWAP_DEFAULT_COLOR: egui::Color32 = crate::theme::DRAW_CYAN;
const AVWAP_DEFAULT_WIDTH_PX: f32 = 2.0;
const AVWAP_DEFAULT_FILL_ALPHA: u8 = 24;

/// The sources an anchored VWAP may average. Price-scaled only: a VWAP of
/// delta or cvd would put a flow series on the price axis — the scale lie
/// `SourceId::is_price_scaled` exists to prevent.
pub const AVWAP_SOURCES: [SourceId; 7] = [
    SourceId::Open,
    SourceId::High,
    SourceId::Low,
    SourceId::Close,
    SourceId::Hl2,
    SourceId::Hlc3,
    SourceId::Ohlc4,
];

/// One σ-band pair's configuration.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AvwapBand {
    pub on: bool,
    pub mult: f64,
}

/// Everything one refresh computed for one object. Derived state: excluded
/// from equality and presets (the [`FrvpCache`](super::FrvpCache) rule), so a
/// recompute can never read as a user edit to the undo history.
#[derive(Debug, Clone)]
pub struct AvwapCache {
    /// The **closed-state** inputs the rows were computed from;
    /// `avwap::refresh` replays only when this misses. The forming bar has
    /// its own signature ([`AvwapCache::partial`]) precisely so a live tick
    /// re-evaluates one row, never the anchored span.
    pub key: AvwapCacheKey,
    /// The forming bar the last row describes, when one is on the tape.
    pub partial: Option<AvwapPartialSig>,
    /// The slot the first row belongs to — the anchor's own bar.
    pub first_slot: usize,
    /// One row per bar from the anchor to the newest — the forming bar's
    /// row last, when `partial` is set: `[vwap, u1, l1, u2, l2, u3, l3]`,
    /// `NaN` where the kernel answered `na` or the pair is off.
    pub rows: Vec<[f64; AVWAP_ROW_WIDTH]>,
    /// The kernel as of the last **closed** bar — the accumulators the next
    /// bar close extends and the next forming-bar tick previews against,
    /// so neither replays the anchored span.
    pub(crate) kernel: quantick_indicators::native::AnchoredVwap,
}

/// The forming bar's exact signature — its last-trade instant and trade
/// count — so the live row recomputes when (and only when) it changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AvwapPartialSig {
    pub close_time: i64,
    pub trades: u64,
}

/// Everything the **closed-bar** replay depends on. An anchor drag changes
/// the slot, a bar close bumps `closed_len`, a config edit changes
/// source/bands, a rebuild bumps the revision, a backfill page grows the
/// prefix.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AvwapCacheKey {
    pub anchor_slot: usize,
    pub timeline_revision: u64,
    pub closed_len: usize,
    pub prefix_len: usize,
    pub source: SourceId,
    pub bands: [AvwapBand; AVWAP_BAND_PAIRS],
}

/// The versioned on-disk shape of a saved preset — config only, never
/// coordinates or cache.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct AvwapPresetData {
    version: u32,
    /// The source by its dialect name, so the file stays hand-readable.
    source: String,
    band_on: [bool; AVWAP_BAND_PAIRS],
    band_mult: [f64; AVWAP_BAND_PAIRS],
}

#[derive(Debug, Clone)]
pub struct AvwapPayload {
    /// The price series the average weighs.
    pub source: SourceId,
    /// The σ-band pairs, nearest first.
    pub bands: [AvwapBand; AVWAP_BAND_PAIRS],
    /// Derived state, refreshed by `crate::avwap::refresh`.
    pub cache: Option<AvwapCache>,
}

impl Default for AvwapPayload {
    /// hlc3 with the 1σ pair on — the kernel's own defaults.
    fn default() -> Self {
        Self {
            source: SourceId::Hlc3,
            bands: [
                AvwapBand {
                    on: true,
                    mult: AVWAP_DEFAULT_MULTS[0],
                },
                AvwapBand {
                    on: false,
                    mult: AVWAP_DEFAULT_MULTS[1],
                },
                AvwapBand {
                    on: false,
                    mult: AVWAP_DEFAULT_MULTS[2],
                },
            ],
            cache: None,
        }
    }
}

impl PartialEq for AvwapPayload {
    /// Config only — the cache is derived (see [`AvwapCache`]).
    fn eq(&self, other: &Self) -> bool {
        self.source == other.source && self.bands == other.bands
    }
}

impl DrawingPayload for AvwapPayload {
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
        toml::Value::try_from(AvwapPresetData {
            version: PRESET_FORMAT_VERSION,
            source: self.source.as_str().to_owned(),
            band_on: [self.bands[0].on, self.bands[1].on, self.bands[2].on],
            band_mult: [self.bands[0].mult, self.bands[1].mult, self.bands[2].mult],
        })
        .ok()
    }
    fn import_preset(&mut self, value: &toml::Value) -> bool {
        let Ok(data) = value.clone().try_into::<AvwapPresetData>() else {
            return false;
        };
        let Some(source) = AVWAP_SOURCES
            .into_iter()
            .find(|source| source.as_str() == data.source)
        else {
            return false;
        };
        if data.version != PRESET_FORMAT_VERSION
            || data.band_mult.iter().any(|mult| !mult.is_finite() || *mult < 0.0)
        {
            return false;
        }
        self.source = source;
        for (pair, band) in self.bands.iter_mut().enumerate() {
            band.on = data.band_on[pair];
            band.mult = data.band_mult[pair];
        }
        // The cache key carries source and bands, so the next refresh
        // replays; nothing to invalidate by hand.
        true
    }
}

/// x of an absolute slot through the anchor's own position and the pane's
/// bar width — a series tool's one-anchor version of the profile's affine
/// two-anchor map.
fn slot_x(anchor_x: f32, anchor_bar: f32, px_per_bar: f32, slot: usize) -> f32 {
    #[allow(clippy::cast_precision_loss)]
    let bar = slot as f32;
    anchor_x + (bar - anchor_bar) * px_per_bar
}

/// The cached slots whose x falls inside `chart_rect`, padded by one so a
/// segment entering from off screen still draws its visible half.
fn visible_rows(
    cache: &AvwapCache,
    anchor_x: f32,
    anchor_bar: f32,
    px_per_bar: f32,
    chart_rect: egui::Rect,
) -> std::ops::Range<usize> {
    if cache.rows.is_empty() || px_per_bar <= 0.0 {
        return 0..0;
    }
    let bar_of_x = |x: f32| anchor_bar + (x - anchor_x) / px_per_bar;
    #[allow(clippy::cast_precision_loss)]
    let first = cache.first_slot as f32;
    let lo = (bar_of_x(chart_rect.left()) - first - 1.0).max(0.0);
    let hi = (bar_of_x(chart_rect.right()) - first + 1.0).max(0.0);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let lo = (lo as usize).min(cache.rows.len());
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let hi = (hi.ceil() as usize + 1).min(cache.rows.len());
    lo..hi.max(lo)
}

/// Stroke a cached column as a polyline, breaking on `na` — the honest gap,
/// never a bridge.
#[allow(clippy::too_many_arguments)]
fn stroke_column(
    painter: &egui::Painter,
    cache: &AvwapCache,
    range: std::ops::Range<usize>,
    column: usize,
    anchor_x: f32,
    anchor_bar: f32,
    px_per_bar: f32,
    ctxt: &DrawContext<'_>,
    stroke: egui::Stroke,
) {
    let mut run: Vec<egui::Pos2> = Vec::with_capacity(range.len());
    for index in range {
        let value = cache.rows[index][column];
        if value.is_nan() {
            if run.len() > 1 {
                painter.add(egui::Shape::line(std::mem::take(&mut run), stroke));
            } else {
                run.clear();
            }
            continue;
        }
        run.push(egui::pos2(
            slot_x(anchor_x, anchor_bar, px_per_bar, cache.first_slot + index),
            ctxt.scale.y(value),
        ));
    }
    if run.len() > 1 {
        painter.add(egui::Shape::line(run, stroke));
    }
}

/// Fill between a band pair's columns, one quad per adjacent slot where both
/// rows have both values.
#[allow(clippy::too_many_arguments)]
fn fill_pair(
    painter: &egui::Painter,
    cache: &AvwapCache,
    range: std::ops::Range<usize>,
    pair: usize,
    anchor_x: f32,
    anchor_bar: f32,
    px_per_bar: f32,
    ctxt: &DrawContext<'_>,
    color: egui::Color32,
) {
    let upper = 1 + 2 * pair;
    let lower = upper + 1;
    for index in range.clone() {
        let next = index + 1;
        if !range.contains(&next) {
            break;
        }
        let (u0, l0) = (cache.rows[index][upper], cache.rows[index][lower]);
        let (u1, l1) = (cache.rows[next][upper], cache.rows[next][lower]);
        if u0.is_nan() || l0.is_nan() || u1.is_nan() || l1.is_nan() {
            continue;
        }
        let x0 = slot_x(anchor_x, anchor_bar, px_per_bar, cache.first_slot + index);
        let x1 = slot_x(anchor_x, anchor_bar, px_per_bar, cache.first_slot + next);
        painter.add(egui::Shape::convex_polygon(
            vec![
                egui::pos2(x0, ctxt.scale.y(u0)),
                egui::pos2(x1, ctxt.scale.y(u1)),
                egui::pos2(x1, ctxt.scale.y(l1)),
                egui::pos2(x0, ctxt.scale.y(l0)),
            ],
            color,
            egui::Stroke::NONE,
        ));
    }
}

impl DrawingToolImpl for AnchoredVwapTool {
    fn id(&self) -> &'static str {
        "anchored-vwap"
    }
    fn name(&self) -> &'static str {
        "Anchored VWAP"
    }
    fn settings_title(&self) -> &'static str {
        "Anchored VWAP settings"
    }
    fn icon(&self) -> &'static str {
        icons::ANCHOR_SIMPLE
    }
    fn hover_text(&self) -> &'static str {
        "Anchored VWAP - click the bar the average starts at (W)"
    }
    fn required_points(&self) -> usize {
        1
    }
    fn shortcut(&self) -> Option<ToolShortcut> {
        Some(ToolShortcut {
            key: egui::Key::W,
            shift: false,
        })
    }
    fn supports_fill(&self) -> bool {
        true
    }
    /// A VWAP is a price whatever band the click started over.
    fn price_band_only(&self) -> bool {
        true
    }
    fn default_color(&self) -> Option<egui::Color32> {
        Some(AVWAP_DEFAULT_COLOR)
    }
    fn default_width_px(&self) -> Option<f32> {
        Some(AVWAP_DEFAULT_WIDTH_PX)
    }
    fn default_fill_alpha(&self) -> Option<u8> {
        Some(AVWAP_DEFAULT_FILL_ALPHA)
    }
    fn default_payload(&self) -> Box<dyn DrawingPayload> {
        Box::new(AvwapPayload::default())
    }
    fn extra_tab(&self) -> Option<&'static str> {
        Some("VWAP")
    }
    fn draw_extra_tab(
        &self,
        ui: &mut egui::Ui,
        drawing: &mut Drawing,
        host: &mut dyn PresetHost,
    ) -> bool {
        draw_vwap_tab(ui, drawing, host)
    }
    fn paint(
        &self,
        painter: &egui::Painter,
        chart_rect: egui::Rect,
        style: DrawingStyle,
        points: &[egui::Pos2],
        ctxt: &DrawContext<'_>,
    ) {
        let Some(payload) = ctxt.payload.as_any().downcast_ref::<AvwapPayload>() else {
            return;
        };
        let Some(anchor) = points.first().copied() else {
            return;
        };
        let stroke = drawing_stroke(style);
        let anchor_bar = ctxt.anchors.first().map(|point| point.bar);

        if let (Some(cache), Some(anchor_bar), true) =
            (payload.cache.as_ref(), anchor_bar, ctxt.px_per_bar > 0.0)
        {
            let range = visible_rows(cache, anchor.x, anchor_bar, ctxt.px_per_bar, chart_rect);
            if !ctxt.halo {
                // Fills behind everything, farthest pair first so the nearer,
                // denser fills layer on top.
                for pair in (0..AVWAP_BAND_PAIRS).rev() {
                    if !payload.bands[pair].on || style.fill_alpha == 0 {
                        continue;
                    }
                    let alpha = (f32::from(style.fill_alpha) * BAND_FILL_FRAC[pair])
                        .round()
                        .clamp(0.0, 255.0);
                    let color = style.color.gamma_multiply(alpha / 255.0);
                    fill_pair(
                        painter,
                        cache,
                        range.clone(),
                        pair,
                        anchor.x,
                        anchor_bar,
                        ctxt.px_per_bar,
                        ctxt,
                        color,
                    );
                }
                for (pair, line_alpha) in BAND_LINE_ALPHA.iter().enumerate() {
                    if !payload.bands[pair].on {
                        continue;
                    }
                    let band_stroke = egui::Stroke::new(
                        (style.width_px * BAND_WIDTH_FRAC).max(0.5),
                        style.color.gamma_multiply(*line_alpha),
                    );
                    for column in [1 + 2 * pair, 2 + 2 * pair] {
                        stroke_column(
                            painter,
                            cache,
                            range.clone(),
                            column,
                            anchor.x,
                            anchor_bar,
                            ctxt.px_per_bar,
                            ctxt,
                            band_stroke,
                        );
                    }
                }
            }
            // The vwap itself — the one stroke the halo pass repaints.
            stroke_column(
                painter,
                cache,
                range,
                0,
                anchor.x,
                anchor_bar,
                ctxt.px_per_bar,
                ctxt,
                stroke,
            );
        }

        // While selected, the shared anchor handle (same ring language, same
        // point) is the one marker — two concentric rings half a pixel apart
        // read as a smudge, not as an affordance.
        if !ctxt.halo && !ctxt.selected {
            // The anchor marker: a ring on the bar that actually seeds the
            // sums, at the average's own first value — not on the raw click.
            // A click past the newest bar clamps to it, and the ring saying
            // so is the honesty the marker owes. Falls back to the clicked
            // point only while no cache exists yet.
            let marker = match (payload.cache.as_ref(), anchor_bar) {
                (Some(cache), Some(anchor_bar))
                    if ctxt.px_per_bar > 0.0
                        && cache.rows.first().is_some_and(|row| !row[0].is_nan()) =>
                {
                    egui::pos2(
                        slot_x(anchor.x, anchor_bar, ctxt.px_per_bar, cache.first_slot),
                        ctxt.scale.y(cache.rows[0][0]),
                    )
                }
                _ => anchor,
            };
            if chart_rect.contains(marker) {
                painter.circle_filled(marker, ANCHOR_RADIUS_PX, crate::theme::CANVAS);
                painter.circle_stroke(
                    marker,
                    ANCHOR_RADIUS_PX,
                    egui::Stroke::new(ANCHOR_RING_PX, style.color),
                );
            }
        }
    }
    fn hit_test(
        &self,
        chart_rect: egui::Rect,
        points: &[egui::Pos2],
        position: egui::Pos2,
        radius_px: f32,
        ctxt: &DrawContext<'_>,
    ) -> bool {
        let Some(anchor) = points.first().copied() else {
            return false;
        };
        if anchor.distance_sq(position) <= radius_px * radius_px {
            return true;
        }
        // Near the vwap line: resolve the bar under the pointer and compare
        // against that slot's cached value (and its neighbours', so a steep
        // segment is still grabbable between two centres).
        let Some(payload) = ctxt.payload.as_any().downcast_ref::<AvwapPayload>() else {
            return false;
        };
        let (Some(cache), Some(anchor_point)) = (payload.cache.as_ref(), ctxt.anchors.first())
        else {
            return false;
        };
        if ctxt.px_per_bar <= 0.0 || !chart_rect.contains(position) {
            return false;
        }
        let bar = anchor_point.bar + (position.x - anchor.x) / ctxt.px_per_bar;
        #[allow(clippy::cast_precision_loss)]
        let offset = bar - cache.first_slot as f32;
        if offset < -1.0 {
            return false;
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let nearest = offset.max(0.0).round() as usize;
        for index in nearest.saturating_sub(1)..=nearest + 1 {
            let Some(row) = cache.rows.get(index) else {
                continue;
            };
            if row[0].is_nan() {
                continue;
            }
            if (ctxt.scale.y(row[0]) - position.y).abs() <= radius_px {
                return true;
            }
        }
        false
    }
    #[cfg(test)]
    fn test_geometry(&self) -> (Vec<egui::Pos2>, egui::Pos2) {
        (vec![egui::pos2(200.0, 150.0)], egui::pos2(201.0, 151.0))
    }
}

/// The VWAP tab in the inspector: source, the three band pairs, and named
/// presets through the host — the FRVP tab's contract.
fn draw_vwap_tab(ui: &mut egui::Ui, drawing: &mut Drawing, host: &mut dyn PresetHost) -> bool {
    let tool_id = drawing.tool.id();
    let Some(payload) = drawing.payload.as_any_mut().downcast_mut::<AvwapPayload>() else {
        return false;
    };
    let mut edited = false;

    ui.horizontal(|ui| {
        ui.label("source");
        egui::ComboBox::from_id_salt("avwap-source")
            .selected_text(payload.source.as_str())
            .show_ui(ui, |ui| {
                for source in AVWAP_SOURCES {
                    if ui
                        .selectable_value(&mut payload.source, source, source.as_str())
                        .changed()
                    {
                        edited = true;
                    }
                }
            });
    });
    for (pair, band) in payload.bands.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            edited |= ui
                .checkbox(&mut band.on, format!("{}σ band", pair + 1))
                .on_hover_text(
                    "standard-deviation band around the anchored VWAP; \
                     the multiplier scales how far from the average it sits",
                )
                .changed();
            let drag = egui::DragValue::new(&mut band.mult)
                .range(0.0..=f64::MAX)
                .speed(0.1)
                .suffix(" σ");
            edited |= ui.add_enabled(band.on, drag).changed();
        });
    }

    ui.separator();

    // Named presets via the host — the same contract the FRVP tab uses.
    let customs = host.custom_preset_names(tool_id);
    ui.horizontal(|ui| {
        let selected_id = ui.id().with("avwap-preset");
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
        let name_id = ui.id().with("avwap-preset-name");
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

    fn cache_with_rows(first_slot: usize, rows: Vec<[f64; AVWAP_ROW_WIDTH]>) -> AvwapCache {
        AvwapCache {
            key: AvwapCacheKey {
                anchor_slot: first_slot,
                timeline_revision: 0,
                closed_len: first_slot + rows.len(),
                prefix_len: 0,
                source: SourceId::Hlc3,
                bands: AvwapPayload::default().bands,
            },
            partial: None,
            first_slot,
            rows,
            kernel: quantick_indicators::native::AnchoredVwap::default(),
        }
    }

    #[test]
    fn preset_round_trip_excludes_the_cache() {
        let mut payload = AvwapPayload {
            source: SourceId::Close,
            bands: [
                AvwapBand { on: true, mult: 1.5 },
                AvwapBand { on: true, mult: 2.5 },
                AvwapBand {
                    on: false,
                    mult: 3.0,
                },
            ],
            cache: Some(cache_with_rows(2, vec![[100.0; AVWAP_ROW_WIDTH]])),
        };
        let exported = payload.export_preset().expect("avwap exports its preset");
        assert!(!exported.to_string().contains("cache"));

        let mut restored = AvwapPayload::default();
        assert!(restored.import_preset(&exported));
        assert_eq!(restored.source, SourceId::Close);
        assert_eq!(restored.bands[0].mult, 1.5);
        assert!(restored.bands[1].on);
        assert!(restored.cache.is_none(), "a preset never installs a cache");

        // Equality ignores the cache: dropping it changes nothing.
        let with_cache = payload.clone();
        payload.cache = None;
        assert!(payload.eq_dyn(&with_cache));
    }

    #[test]
    fn import_rejects_bad_versions_flow_sources_and_bad_mults() {
        let mut payload = AvwapPayload::default();
        let mut bad = payload.export_preset().unwrap();
        if let toml::Value::Table(table) = &mut bad {
            table.insert("version".into(), toml::Value::Integer(99));
        }
        assert!(!payload.import_preset(&bad));

        let mut flow = payload.export_preset().unwrap();
        if let toml::Value::Table(table) = &mut flow {
            table.insert("source".into(), toml::Value::String("cvd".into()));
        }
        assert!(
            !payload.import_preset(&flow),
            "a flow series on the price axis is refused at load"
        );

        let mut negative = payload.export_preset().unwrap();
        if let toml::Value::Table(table) = &mut negative {
            table.insert(
                "band_mult".into(),
                toml::Value::Array(vec![
                    toml::Value::Float(-1.0),
                    toml::Value::Float(2.0),
                    toml::Value::Float(3.0),
                ]),
            );
        }
        assert!(!payload.import_preset(&negative));
    }

    #[test]
    fn every_avwap_source_is_price_scaled() {
        for source in AVWAP_SOURCES {
            assert!(source.is_price_scaled(), "{source:?}");
        }
    }

    #[test]
    fn hit_test_finds_the_line_and_na_breaks_it() {
        let scale = PriceScale::from_range(90.0, 110.0, 0.0, 400.0);
        let payload = AvwapPayload {
            cache: Some(cache_with_rows(
                10,
                vec![
                    [100.0, f64::NAN, f64::NAN, f64::NAN, f64::NAN, f64::NAN, f64::NAN],
                    [f64::NAN; AVWAP_ROW_WIDTH],
                    [100.0, f64::NAN, f64::NAN, f64::NAN, f64::NAN, f64::NAN, f64::NAN],
                ],
            )),
            ..AvwapPayload::default()
        };
        let anchors = [ChartPoint::at(10.0, 100.0)];
        let chart_rect = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 400.0));
        // Anchor bar 10 sits at x=100 with 20 px per bar.
        let anchor_screen = [egui::pos2(100.0, scale.y(100.0))];
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
        };
        let y = scale.y(100.0);
        // On the line at slot 10 and slot 12.
        assert!(TOOL.hit_test(chart_rect, &anchor_screen, egui::pos2(100.0, y), 4.0, &ctxt));
        assert!(TOOL.hit_test(chart_rect, &anchor_screen, egui::pos2(140.0, y), 4.0, &ctxt));
        // Slot 11 is `na`: nothing to grab there but the neighbours' reach.
        assert!(!TOOL.hit_test(
            chart_rect,
            &anchor_screen,
            egui::pos2(120.0, y + 30.0),
            4.0,
            &ctxt
        ));
        // Far from the line and the anchor: no hit.
        assert!(!TOOL.hit_test(
            chart_rect,
            &anchor_screen,
            egui::pos2(300.0, y - 80.0),
            4.0,
            &ctxt
        ));
    }

    #[test]
    fn visible_rows_clips_to_the_chart() {
        let cache = cache_with_rows(0, vec![[100.0; AVWAP_ROW_WIDTH]; 100]);
        let chart_rect = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(200.0, 400.0));
        // 20 px per bar, anchor bar 0 at x=0: slots 0..=10 are on screen.
        let range = visible_rows(&cache, 0.0, 0.0, 20.0, chart_rect);
        assert!(range.start == 0);
        assert!(range.end <= 13, "one slot of padding each side: {range:?}");
        assert!(range.end >= 11, "every on-screen slot draws: {range:?}");
        // A pane that cannot name its bar width draws nothing.
        assert_eq!(visible_rows(&cache, 0.0, 0.0, 0.0, chart_rect), 0..0);
    }
}
