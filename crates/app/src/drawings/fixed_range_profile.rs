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
    DrawContext, Drawing, DrawingPayload, DrawingStyle, DrawingToolImpl, PresetHost, drawing_stroke,
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
const VA_DASH_PX: f32 = 4.0;
const VA_GAP_PX: f32 = 3.0;
/// Row fills inside vs outside the value area — the area is read by weight,
/// not by outline.
const ROW_ALPHA_IN_VA: f32 = 0.55;
const ROW_ALPHA_OUT_VA: f32 = 0.30;

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
    /// Bars the anchors span on the chart, prefix candles included.
    pub bars_total: usize,
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
        // The cache key carries the value-area fraction, so the next refresh
        // recomputes; nothing to invalidate by hand.
        true
    }
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
        let left = points[0].x.min(points[1].x);
        let right = points[0].x.max(points[1].x);
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

        if let Some((profile, value_area)) = profile {
            let range_width = (right - left).max(1.0);
            let height = row_height(profile, ctxt);
            let max_volume = to_f64(profile.max_level_volume()).max(f64::MIN_POSITIVE);
            let in_va = |bucket: i64| {
                payload.show_value_area
                    && value_area.is_some_and(|area| bucket >= area.val && bucket <= area.vah)
            };
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
                    painter.rect_filled(
                        egui::Rect::from_min_max(
                            egui::pos2(left, y_top),
                            egui::pos2(left + buy_width, y_bottom),
                        ),
                        egui::Rounding::ZERO,
                        theme::BUY.gamma_multiply(alpha),
                    );
                    painter.rect_filled(
                        egui::Rect::from_min_max(
                            egui::pos2(left + buy_width, y_top),
                            egui::pos2(left + width, y_bottom),
                        ),
                        egui::Rounding::ZERO,
                        theme::SELL.gamma_multiply(alpha),
                    );
                } else {
                    painter.rect_filled(
                        egui::Rect::from_min_max(
                            egui::pos2(left, y_top),
                            egui::pos2(left + width, y_bottom),
                        ),
                        egui::Rounding::ZERO,
                        style.color.gamma_multiply(alpha),
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
                    painter.line_segment(
                        [egui::pos2(left, y), egui::pos2(right, y)],
                        egui::Stroke::new(style.width_px.max(1.0), theme::POC),
                    );
                }
                if payload.show_value_area {
                    // VAH tops its row, VAL bottoms its row: the dashes hug
                    // the area they bound.
                    let vah_y = ctxt
                        .scale
                        .y(to_f64(profile.bucket_price(area.vah.saturating_add(1))));
                    let val_y = ctxt.scale.y(to_f64(profile.bucket_price(area.val)));
                    for y in [vah_y, val_y] {
                        painter.add(egui::Shape::dashed_line(
                            &[egui::pos2(left, y), egui::pos2(right, y)],
                            egui::Stroke::new(style.width_px.max(0.75), style.color),
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
                if cache.bars_covered < cache.bars_total {
                    status.push_str(&format!(
                        " · profile from {} of {} bars",
                        cache.bars_covered, cache.bars_total
                    ));
                }
                if cache.key.side_inferred {
                    // The delta and the buy/sell split rest on guessed
                    // aggressor sides — same label the footprint legend uses.
                    status.push_str(" · side inferred");
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
                        painter.text(
                            egui::pos2(right + LABEL_OFFSET_PX, ctxt.scale.y(to_f64(price))),
                            egui::Align2::LEFT_CENTER,
                            format!("{name} {price}"),
                            egui::FontId::proportional(LABEL_SIZE_PX),
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
            painter.text(
                egui::pos2(left, bottom + LABEL_OFFSET_PX),
                egui::Align2::LEFT_TOP,
                status,
                egui::FontId::proportional(LABEL_SIZE_PX),
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
        let left = points[0].x.min(points[1].x);
        let right = points[0].x.max(points[1].x);
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

    #[test]
    fn preset_round_trip_excludes_the_cache() {
        let mut payload = FrvpPayload {
            value_area_pct: 68,
            width_frac: 0.5,
            show_value_area: false,
            show_poc: true,
            delta_coloring: true,
            show_labels: false,
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
                },
                profile: None,
                empty: Some(FrvpEmpty::NoTape),
                bars_covered: 0,
                bars_total: 5,
            }),
        };
        let exported = payload.export_preset().expect("frvp exports its preset");
        // No derived state travels with a preset.
        assert!(!exported.to_string().contains("cache"));

        let mut restored = FrvpPayload::default();
        assert!(restored.import_preset(&exported));
        assert_eq!(restored.value_area_pct, 68);
        assert!(restored.delta_coloring);
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
