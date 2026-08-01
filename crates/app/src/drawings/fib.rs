//! Fib is a level model, not seven fixed lines.
//!
//! Retracement and extension share this one module: the editable level list,
//! the built-in presets, the linear and log formulas, band fills, label
//! formatting (cached — labels are re-formatted only when an anchor price or
//! the configuration changes, never per frame) and the level-editor tab both
//! tools mount in the inspector. Only the geometry and the price formula
//! differ between the two kinds.

use std::any::Any;
use std::cell::RefCell;

use eframe::egui;
use serde::{Deserialize, Serialize};

use super::{
    DrawContext, Drawing, DrawingPayload, DrawingStyle, FIB_LABEL_OFFSET_PX, FIB_LABEL_SIZE_PX,
    PresetHost, distance_to_segment, drawing_stroke,
};

/// Ratios closer than this are the same level; a duplicate is rejected.
pub const RATIO_DUPLICATE_TOLERANCE: f64 = 0.000_001;
/// Longest accepted custom level label.
pub const MAX_LEVEL_LABEL_LEN: usize = 40;
/// Upper bound of the band-fill alpha (~60% opacity, the spec's slider cap).
pub const MAX_BAND_ALPHA: u8 = 153;
/// Default band-fill alpha (~9%, inside the spec's recommended 8-18%).
pub const DEFAULT_BAND_ALPHA: u8 = 24;
/// Dash geometry of the anchor guides shown while selected.
const GUIDE_DASH_PX: f32 = 4.0;
/// Version stamp of the exported preset format.
const PRESET_FORMAT_VERSION: u32 = 1;

/// Which price formula the object projects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FibKind {
    /// Two anchors; levels give back part of the A→B move:
    /// `P(r) = PA + (PB − PA)·r`.
    Retracement,
    /// Three anchors; the A→B impulse is projected from origin C:
    /// `P(r) = PC + (PB − PA)·r`.
    Extension,
}

impl FibKind {
    #[must_use]
    pub fn required_points(self) -> usize {
        match self {
            Self::Retracement => 2,
            Self::Extension => 3,
        }
    }
}

/// How a level's text is composed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LabelMode {
    #[default]
    PercentPrice,
    RatioPrice,
    PriceOnly,
}

impl LabelMode {
    const ALL: [Self; 3] = [Self::PercentPrice, Self::RatioPrice, Self::PriceOnly];

    #[must_use]
    fn describe(self) -> &'static str {
        match self {
            Self::PercentPrice => "Percent + price",
            Self::RatioPrice => "Ratio + price",
            Self::PriceOnly => "Price only",
        }
    }
}

/// Where the labels sit relative to the level span.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LabelPosition {
    #[default]
    RightOutside,
    RightInside,
    Left,
}

impl LabelPosition {
    const ALL: [Self; 3] = [Self::RightOutside, Self::RightInside, Self::Left];

    #[must_use]
    fn describe(self) -> &'static str {
        match self {
            Self::RightOutside => "Right - outside",
            Self::RightInside => "Right - inside",
            Self::Left => "Left",
        }
    }
}

/// How far the level lines run horizontally.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Extend {
    #[default]
    Anchors,
    Right,
    Both,
}

impl Extend {
    const ALL: [Self; 3] = [Self::Anchors, Self::Right, Self::Both];

    #[must_use]
    fn describe(self) -> &'static str {
        match self {
            Self::Anchors => "Between anchors",
            Self::Right => "To the right",
            Self::Both => "Both sides",
        }
    }
}

/// One configurable level of a Fib object.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FibLevelSpec {
    pub ratio: f64,
    /// Custom text; empty means the automatic ratio label.
    pub label: String,
    pub visible: bool,
    /// Per-level colour override; `None` inherits the object's colour.
    pub color: Option<[u8; 3]>,
    /// Fill from this level to the next visible one.
    pub fill_to_next: bool,
}

impl FibLevelSpec {
    #[must_use]
    pub fn new(ratio: f64) -> Self {
        Self {
            ratio,
            label: String::new(),
            visible: true,
            color: None,
            fill_to_next: false,
        }
    }
}

/// A named built-in ratio set. Built-ins are read-only; custom presets go
/// through the [`PresetHost`].
pub struct FibPreset {
    pub name: &'static str,
    pub ratios: &'static [f64],
}

pub const RETRACEMENT_PRESETS: [FibPreset; 4] = [
    FibPreset {
        name: "Standard",
        ratios: &[0.0, 0.236, 0.382, 0.5, 0.618, 0.786, 1.0],
    },
    FibPreset {
        name: "Compact",
        ratios: &[0.0, 0.382, 0.5, 0.618, 1.0],
    },
    FibPreset {
        name: "Deep",
        ratios: &[0.0, 0.236, 0.382, 0.5, 0.618, 0.705, 0.786, 0.886, 1.0],
    },
    FibPreset {
        name: "Extended",
        ratios: &[
            -0.618, -0.272, 0.0, 0.236, 0.382, 0.5, 0.618, 0.786, 1.0, 1.272, 1.618,
        ],
    },
];

pub const EXTENSION_PRESETS: [FibPreset; 3] = [
    FibPreset {
        name: "Targets",
        ratios: &[0.618, 1.0, 1.272, 1.618, 2.0, 2.618],
    },
    FibPreset {
        name: "Conservative",
        ratios: &[0.618, 1.0, 1.272, 1.618],
    },
    FibPreset {
        name: "Expanded",
        ratios: &[0.382, 0.618, 1.0, 1.272, 1.618, 2.0, 2.618, 4.236],
    },
];

#[must_use]
pub fn builtin_presets(kind: FibKind) -> &'static [FibPreset] {
    match kind {
        FibKind::Retracement => &RETRACEMENT_PRESETS,
        FibKind::Extension => &EXTENSION_PRESETS,
    }
}

/// Formatted level labels, rebuilt only when anchors or config change.
#[derive(Debug, Clone, Default, PartialEq)]
struct LabelCache {
    key: (u64, u64, u64, u64),
    labels: Vec<String>,
}

/// The tool-owned state of one Fib object.
#[derive(Debug, Clone)]
pub struct FibPayload {
    pub kind: FibKind,
    pub levels: Vec<FibLevelSpec>,
    pub band_alpha: u8,
    /// Explicit scale choice: log never follows the chart silently, and it
    /// only ever computes over positive prices.
    pub log_scale: bool,
    pub label_mode: LabelMode,
    pub label_position: LabelPosition,
    pub extend: Extend,
    /// Bumped by the editor so the label cache notices config edits.
    revision: u64,
    cache: RefCell<LabelCache>,
}

impl PartialEq for FibPayload {
    fn eq(&self, other: &Self) -> bool {
        // The cache and its revision are memoization, not state.
        self.kind == other.kind
            && self.levels == other.levels
            && self.band_alpha == other.band_alpha
            && self.log_scale == other.log_scale
            && self.label_mode == other.label_mode
            && self.label_position == other.label_position
            && self.extend == other.extend
    }
}

impl FibPayload {
    #[must_use]
    pub fn new(kind: FibKind) -> Self {
        let standard = builtin_presets(kind)[0].ratios;
        Self {
            kind,
            levels: standard
                .iter()
                .map(|&ratio| FibLevelSpec::new(ratio))
                .collect(),
            band_alpha: DEFAULT_BAND_ALPHA,
            log_scale: false,
            label_mode: LabelMode::default(),
            label_position: LabelPosition::default(),
            extend: Extend::default(),
            revision: 0,
            cache: RefCell::new(LabelCache::default()),
        }
    }

    pub fn apply_preset(&mut self, preset: &FibPreset) {
        self.levels = preset
            .ratios
            .iter()
            .map(|&ratio| FibLevelSpec::new(ratio))
            .collect();
        self.touch();
    }

    fn touch(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    /// Whether `ratio` would duplicate an existing level (other than `skip`).
    #[must_use]
    fn is_duplicate(&self, ratio: f64, skip: Option<usize>) -> bool {
        self.levels.iter().enumerate().any(|(index, level)| {
            Some(index) != skip && (level.ratio - ratio).abs() < RATIO_DUPLICATE_TOLERANCE
        })
    }

    #[must_use]
    fn visible_count(&self) -> usize {
        self.levels.iter().filter(|level| level.visible).count()
    }
}

/// The versioned on-disk shape of a saved preset. Coordinates, name, lock
/// and visibility never travel with it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct FibPresetData {
    version: u32,
    kind: FibKind,
    levels: Vec<FibLevelSpec>,
    band_alpha: u8,
    label_mode: LabelMode,
    label_position: LabelPosition,
    extend: Extend,
}

impl DrawingPayload for FibPayload {
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
        toml::Value::try_from(FibPresetData {
            version: PRESET_FORMAT_VERSION,
            kind: self.kind,
            levels: self.levels.clone(),
            band_alpha: self.band_alpha,
            label_mode: self.label_mode,
            label_position: self.label_position,
            extend: self.extend,
        })
        .ok()
    }
    fn import_preset(&mut self, value: &toml::Value) -> bool {
        let Ok(data) = value.clone().try_into::<FibPresetData>() else {
            return false;
        };
        if data.version != PRESET_FORMAT_VERSION
            || data.kind != self.kind
            || data.levels.is_empty()
            || !data.levels.iter().any(|level| level.visible)
            || data.levels.iter().any(|level| !level.ratio.is_finite())
        {
            return false;
        }
        self.levels = data.levels;
        self.band_alpha = data.band_alpha.min(MAX_BAND_ALPHA);
        self.label_mode = data.label_mode;
        self.label_position = data.label_position;
        self.extend = data.extend;
        self.touch();
        true
    }
}

/// Parse a level ratio the way a trader writes one: a bare number is always
/// a ratio (`.618`, `0.618`, `-0.272`), percent needs the sign (`61.8%`),
/// and the decimal comma is accepted alongside the dot.
#[must_use]
pub fn parse_ratio(input: &str) -> Option<f64> {
    let trimmed = input.trim().replace(',', ".");
    let (number, percent) = match trimmed.strip_suffix('%') {
        Some(rest) => (rest.trim_end(), true),
        None => (trimmed.as_str(), false),
    };
    let value: f64 = number.parse().ok()?;
    if !value.is_finite() {
        return None;
    }
    Some(if percent { value / 100.0 } else { value })
}

/// Whether the log formulas are computable over these anchor prices.
#[must_use]
pub fn log_scale_allowed(a: f64, b: f64, c: f64) -> bool {
    a > 0.0 && b > 0.0 && c > 0.0
}

/// The price of one level. `c` is ignored by retracement; for extension it
/// is the projection origin. Log demands positive prices — `None` otherwise.
#[must_use]
pub fn level_price(
    kind: FibKind,
    log_scale: bool,
    a: f64,
    b: f64,
    c: f64,
    ratio: f64,
) -> Option<f64> {
    if !log_scale {
        return Some(match kind {
            FibKind::Retracement => a + (b - a) * ratio,
            FibKind::Extension => c + (b - a) * ratio,
        });
    }
    if !log_scale_allowed(a, b, c) {
        return None;
    }
    let log_move = (b / a).ln();
    Some(match kind {
        FibKind::Retracement => a * (log_move * ratio).exp(),
        FibKind::Extension => c * (log_move * ratio).exp(),
    })
}

fn format_price(price: f64) -> String {
    if price.abs() < 1.0 {
        format!("{price:.6}")
    } else {
        format!("{price:.2}")
    }
}

fn format_label(mode: LabelMode, level: &FibLevelSpec, price: f64) -> String {
    let ratio_part = if level.label.is_empty() {
        match mode {
            LabelMode::PercentPrice => format!("{:.1}%", level.ratio * 100.0),
            LabelMode::RatioPrice => format!("{:.3}", level.ratio),
            LabelMode::PriceOnly => String::new(),
        }
    } else {
        level.label.clone()
    };
    if ratio_part.is_empty() {
        format_price(price)
    } else {
        format!("{ratio_part} \u{00b7} {}", format_price(price))
    }
}

fn level_color(level: &FibLevelSpec, style: DrawingStyle) -> egui::Color32 {
    level
        .color
        .map_or(style.color, |[r, g, b]| egui::Color32::from_rgb(r, g, b))
}

/// The effective log flag: the explicit choice, honoured only while every
/// anchor price is positive (never a silent approximation).
fn effective_log(payload: &FibPayload, a: f64, b: f64, c: f64) -> bool {
    payload.log_scale && log_scale_allowed(a, b, c)
}

struct Anchors {
    a: f64,
    b: f64,
    c: f64,
}

fn anchor_prices(payload: &FibPayload, ctxt: &DrawContext<'_>) -> Option<Anchors> {
    let required = payload.kind.required_points();
    if ctxt.anchors.len() < required {
        return None;
    }
    let a = ctxt.anchors[0].price;
    let b = ctxt.anchors[1].price;
    let c = if payload.kind == FibKind::Extension {
        ctxt.anchors[2].price
    } else {
        b
    };
    Some(Anchors { a, b, c })
}

fn level_span(points: &[egui::Pos2], chart_rect: egui::Rect, extend: Extend) -> (f32, f32) {
    let left = points
        .iter()
        .map(|point| point.x)
        .fold(f32::INFINITY, f32::min);
    let right = points
        .iter()
        .map(|point| point.x)
        .fold(f32::NEG_INFINITY, f32::max);
    match extend {
        Extend::Anchors => (left, right),
        Extend::Right => (left, chart_rect.right()),
        Extend::Both => (chart_rect.left(), chart_rect.right()),
    }
}

/// Paint one Fib object (both kinds). Levels are computed in price space and
/// projected through the chart's scale, so linear and log both land exactly.
pub(super) fn paint(
    painter: &egui::Painter,
    chart_rect: egui::Rect,
    style: DrawingStyle,
    points: &[egui::Pos2],
    ctxt: &DrawContext<'_>,
) {
    let Some(payload) = ctxt.payload.as_any().downcast_ref::<FibPayload>() else {
        return;
    };
    let stroke = drawing_stroke(style);
    let Some(anchors) = anchor_prices(payload, ctxt) else {
        // Draft preview: connect the anchors placed so far.
        for pair in points.windows(2) {
            painter.line_segment([pair[0], pair[1]], stroke);
        }
        return;
    };
    let Anchors { a, b, c } = anchors;
    let log = effective_log(payload, a, b, c);
    let (left, right) = level_span(points, chart_rect, payload.extend);

    // Band fills first, under the level lines. Skipped on the halo pass.
    if !ctxt.halo && payload.band_alpha > 0 {
        let visible: Vec<(f32, egui::Color32)> = payload
            .levels
            .iter()
            .filter(|level| level.visible)
            .filter_map(|level| {
                level_price(payload.kind, log, a, b, c, level.ratio)
                    .map(|price| (ctxt.scale.y(price), level_color(level, style)))
            })
            .collect();
        for (index, level) in payload
            .levels
            .iter()
            .filter(|level| level.visible)
            .enumerate()
        {
            if !level.fill_to_next || index + 1 >= visible.len() {
                continue;
            }
            let (y, color) = visible[index];
            let (next_y, _) = visible[index + 1];
            let fill = egui::Color32::from_rgba_unmultiplied(
                color.r(),
                color.g(),
                color.b(),
                payload.band_alpha,
            );
            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(left, y.min(next_y)),
                    egui::pos2(right, y.max(next_y)),
                ),
                egui::Rounding::ZERO,
                fill,
            );
        }
    }

    // Level lines (and cached labels on the real pass).
    let labels = (!ctxt.halo).then(|| labels_for(payload, a, b, c, log));
    let mut label_index = 0usize;
    for level in payload.levels.iter().filter(|level| level.visible) {
        let Some(price) = level_price(payload.kind, log, a, b, c, level.ratio) else {
            continue;
        };
        let y = ctxt.scale.y(price);
        let line_stroke = if ctxt.halo {
            stroke
        } else {
            egui::Stroke::new(style.width_px, level_color(level, style))
        };
        painter.line_segment([egui::pos2(left, y), egui::pos2(right, y)], line_stroke);
        if let Some(labels) = &labels
            && let Some(text) = labels.get(label_index)
        {
            let (anchor, x) = match payload.label_position {
                LabelPosition::RightOutside => {
                    (egui::Align2::LEFT_BOTTOM, right + FIB_LABEL_OFFSET_PX)
                }
                LabelPosition::RightInside => {
                    (egui::Align2::RIGHT_BOTTOM, right - FIB_LABEL_OFFSET_PX)
                }
                LabelPosition::Left => (egui::Align2::LEFT_BOTTOM, left + FIB_LABEL_OFFSET_PX),
            };
            painter.text(
                egui::pos2(x, y),
                anchor,
                text,
                egui::FontId::monospace(FIB_LABEL_SIZE_PX),
                level_color(level, style),
            );
        }
        label_index += 1;
    }

    // Anchor guides while selected: dashed A-B (and B-C for extension).
    if ctxt.selected && !ctxt.halo && points.len() >= 2 {
        let guide = egui::Stroke::new(1.0_f32, style.color);
        painter.add(egui::Shape::dashed_line(
            &[points[0], points[1]],
            guide,
            GUIDE_DASH_PX,
            GUIDE_DASH_PX,
        ));
        if payload.kind == FibKind::Extension && points.len() >= 3 {
            painter.add(egui::Shape::dashed_line(
                &[points[1], points[2]],
                guide,
                GUIDE_DASH_PX,
                GUIDE_DASH_PX,
            ));
        }
    }
}

/// Rebuild the label cache if anchors or config changed; return a clone of
/// the cached strings (cheap: a handful of small strings, only while a Fib
/// is actually painting labels).
fn labels_for(payload: &FibPayload, a: f64, b: f64, c: f64, log: bool) -> Vec<String> {
    let key = (
        a.to_bits() ^ u64::from(log),
        b.to_bits(),
        c.to_bits(),
        payload.revision,
    );
    let mut cache = payload.cache.borrow_mut();
    if cache.key != key {
        cache.key = key;
        cache.labels = payload
            .levels
            .iter()
            .filter(|level| level.visible)
            .filter_map(|level| {
                level_price(payload.kind, log, a, b, c, level.ratio)
                    .map(|price| format_label(payload.label_mode, level, price))
            })
            .collect();
    }
    cache.labels.clone()
}

/// Hit-test one Fib object: any visible level line, or the anchor legs.
pub(super) fn hit_test(
    chart_rect: egui::Rect,
    points: &[egui::Pos2],
    position: egui::Pos2,
    radius_px: f32,
    ctxt: &DrawContext<'_>,
) -> bool {
    let Some(payload) = ctxt.payload.as_any().downcast_ref::<FibPayload>() else {
        return false;
    };
    let Some(Anchors { a, b, c }) = anchor_prices(payload, ctxt) else {
        return false;
    };
    if points.len() >= 2 && distance_to_segment(position, points[0], points[1]) <= radius_px {
        return true;
    }
    if payload.kind == FibKind::Extension
        && points.len() >= 3
        && distance_to_segment(position, points[1], points[2]) <= radius_px
    {
        return true;
    }
    let log = effective_log(payload, a, b, c);
    let (left, right) = level_span(points, chart_rect, payload.extend);
    position.x >= left - radius_px
        && position.x <= right + radius_px
        && payload
            .levels
            .iter()
            .filter(|level| level.visible)
            .filter_map(|level| level_price(payload.kind, log, a, b, c, level.ratio))
            .any(|price| (position.y - ctxt.scale.y(price)).abs() <= radius_px)
}

/// The Levels tab both Fib tools mount in the inspector. Returns whether the
/// payload or the geometry changed (folded into the shared undo coalescing).
pub(super) fn draw_levels_tab(
    ui: &mut egui::Ui,
    drawing: &mut Drawing,
    host: &mut dyn PresetHost,
) -> bool {
    let tool_id = drawing.tool.id();
    let anchor_prices: Vec<f64> = drawing.points.iter().map(|point| point.price).collect();
    let Some(payload) = drawing.payload.as_any_mut().downcast_mut::<FibPayload>() else {
        return false;
    };
    let mut edited = false;
    let mut swap_anchors = false;

    // Built-in presets: read-only ratio sets, applied as one edit.
    ui.horizontal(|ui| {
        ui.label("Preset");
        for preset in builtin_presets(payload.kind) {
            if ui.small_button(preset.name).clicked() {
                payload.apply_preset(preset);
                edited = true;
            }
        }
    });

    // Custom presets via the host: apply, save (with explicit overwrite),
    // delete, and the explicit per-kind default for new objects.
    let customs = host.custom_preset_names(tool_id);
    ui.horizontal(|ui| {
        let selected_id = ui.id().with("custom-preset");
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
        if ui
            .add_enabled(has_selection, egui::Button::new("Set as default").small())
            .on_hover_text("New objects of this tool start from this preset")
            .clicked()
        {
            host.set_default_preset(tool_id, Some(selected.clone()));
        }
    });
    ui.horizontal(|ui| {
        let name_id = ui.id().with("preset-name");
        let mut name: String = ui.data_mut(|data| data.get_temp(name_id).unwrap_or_default());
        ui.add(
            egui::TextEdit::singleline(&mut name)
                .hint_text("Preset name")
                .desired_width(120.0),
        );
        let confirm_id = ui.id().with("preset-overwrite");
        let pending: bool = ui.data_mut(|data| data.get_temp(confirm_id).unwrap_or(false));
        let trimmed = name.trim();
        let valid = !trimmed.is_empty() && trimmed.len() <= MAX_LEVEL_LABEL_LEN;
        if pending {
            ui.label("Overwrite?");
            if ui.small_button("Yes").clicked() {
                if let Some(value) = payload.export_preset() {
                    host.save_custom_preset(tool_id, trimmed, value, true);
                }
                ui.data_mut(|data| data.insert_temp(confirm_id, false));
            }
            if ui.small_button("No").clicked() {
                ui.data_mut(|data| data.insert_temp(confirm_id, false));
            }
        } else if ui
            .add_enabled(valid, egui::Button::new("Save preset").small())
            .clicked()
            && let Some(value) = payload.export_preset()
            && !host.save_custom_preset(tool_id, trimmed, value, false)
        {
            // The name exists: overwriting a custom needs explicit consent.
            ui.data_mut(|data| data.insert_temp(confirm_id, true));
        }
        ui.data_mut(|data| data.insert_temp(name_id, name));
        if host.default_preset(tool_id).is_some() && ui.small_button("Clear default").clicked() {
            host.set_default_preset(tool_id, None);
        }
    });
    ui.separator();

    // The level list. Visible / ratio / label / colour / fill / delete.
    let mut error: Option<&'static str> = None;
    let mut remove: Option<usize> = None;
    let single_level = payload.levels.len() == 1;
    let single_visible = payload.visible_count() <= 1;
    let mut ratio_edits: Vec<(usize, f64)> = Vec::new();
    for (index, level) in payload.levels.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            let mut visible = level.visible;
            if ui
                .checkbox(&mut visible, "")
                .on_hover_text("Show this level")
                .changed()
            {
                if !visible && single_visible && level.visible {
                    error = Some("At least one level stays visible.");
                } else {
                    level.visible = visible;
                    edited = true;
                }
            }
            // Ratio as text: ".618", "0,618" and "61.8%" all parse.
            let ratio_id = ui.id().with(("ratio", index));
            let mut buffer: String = ui.data_mut(|data| {
                data.get_temp(ratio_id)
                    .unwrap_or_else(|| format!("{:.3}", level.ratio))
            });
            let response = ui.add(
                egui::TextEdit::singleline(&mut buffer)
                    .desired_width(56.0)
                    .font(egui::TextStyle::Monospace),
            );
            if response.lost_focus() {
                match parse_ratio(&buffer) {
                    Some(ratio) if (ratio - level.ratio).abs() >= RATIO_DUPLICATE_TOLERANCE => {
                        ratio_edits.push((index, ratio));
                    }
                    Some(_) => {}
                    None => error = Some("Ratios read like 0.618 or 61.8%."),
                }
                buffer = format!("{:.3}", level.ratio);
            }
            ui.data_mut(|data| data.insert_temp(ratio_id, buffer));
            // Custom label (empty = automatic).
            let mut label = level.label.clone();
            if ui
                .add(
                    egui::TextEdit::singleline(&mut label)
                        .hint_text("auto")
                        .desired_width(80.0),
                )
                .changed()
            {
                label.truncate(MAX_LEVEL_LABEL_LEN);
                level.label = label;
                edited = true;
            }
            // Colour override; the small x returns to the inherited colour.
            let mut color = level.color.map_or(drawing_color_of(ui), |[r, g, b]| {
                egui::Color32::from_rgb(r, g, b)
            });
            if ui.color_edit_button_srgba(&mut color).changed() {
                level.color = Some([color.r(), color.g(), color.b()]);
                edited = true;
            }
            if level.color.is_some()
                && ui
                    .small_button("x")
                    .on_hover_text("Inherit colour")
                    .clicked()
            {
                level.color = None;
                edited = true;
            }
            let mut fill = level.fill_to_next;
            if ui
                .checkbox(&mut fill, "fill")
                .on_hover_text("Fill to the next visible level")
                .changed()
            {
                level.fill_to_next = fill;
                edited = true;
            }
            if ui
                .add_enabled(!single_level, egui::Button::new("\u{00d7}").small())
                .on_hover_text("Delete this level")
                .clicked()
            {
                remove = Some(index);
            }
        });
    }
    for (index, ratio) in ratio_edits {
        if payload.is_duplicate(ratio, Some(index)) {
            error = Some("That ratio already exists.");
        } else {
            payload.levels[index].ratio = ratio;
            edited = true;
        }
    }
    if let Some(index) = remove {
        payload.levels.remove(index);
        edited = true;
    }
    if let Some(message) = error {
        ui.colored_label(crate::theme::WARN, message);
    }
    ui.horizontal(|ui| {
        if ui.small_button("+ Add level").clicked() {
            let mut candidate = 0.5;
            while payload.is_duplicate(candidate, None) {
                candidate += 0.1;
            }
            payload.levels.push(FibLevelSpec::new(candidate));
            edited = true;
        }
        if ui.small_button("Sort").clicked() {
            payload
                .levels
                .sort_by(|left, right| left.ratio.total_cmp(&right.ratio));
            edited = true;
        }
        if ui.small_button("Restore").clicked() {
            payload.apply_preset(&builtin_presets(payload.kind)[0]);
            edited = true;
        }
    });
    ui.separator();

    // Bands, labels, extension and scale.
    edited |= ui
        .add(egui::Slider::new(&mut payload.band_alpha, 0..=MAX_BAND_ALPHA).text("band opacity"))
        .changed();
    ui.horizontal(|ui| {
        egui::ComboBox::from_label("labels")
            .selected_text(payload.label_mode.describe())
            .show_ui(ui, |ui| {
                for mode in LabelMode::ALL {
                    edited |= ui
                        .selectable_value(&mut payload.label_mode, mode, mode.describe())
                        .changed();
                }
            });
        egui::ComboBox::from_label("position")
            .selected_text(payload.label_position.describe())
            .show_ui(ui, |ui| {
                for position in LabelPosition::ALL {
                    edited |= ui
                        .selectable_value(
                            &mut payload.label_position,
                            position,
                            position.describe(),
                        )
                        .changed();
                }
            });
    });
    ui.horizontal(|ui| {
        egui::ComboBox::from_label("extend")
            .selected_text(payload.extend.describe())
            .show_ui(ui, |ui| {
                for extend in Extend::ALL {
                    edited |= ui
                        .selectable_value(&mut payload.extend, extend, extend.describe())
                        .changed();
                }
            });
        let log_allowed = anchor_prices.iter().all(|&price| price > 0.0);
        let log = ui
            .add_enabled(
                log_allowed,
                egui::Checkbox::new(&mut payload.log_scale, "log scale"),
            )
            .on_disabled_hover_text("Log needs every anchor price above zero.");
        edited |= log.changed();
        if ui
            .small_button("Invert A \u{2194} B")
            .on_hover_text("Swap the A and B anchors; C never moves")
            .clicked()
        {
            swap_anchors = true;
        }
    });
    if edited {
        payload.touch();
    }
    if swap_anchors && drawing.points.len() >= 2 {
        drawing.points.swap(0, 1);
        edited = true;
    }
    edited
}

/// The colour the current drawing paints with, for the per-level colour
/// button's initial value. Read from the style the caller stashed in the
/// Ui's data (set by the tool impl before calling the editor).
fn drawing_color_of(ui: &egui::Ui) -> egui::Color32 {
    ui.data(|data| data.get_temp(egui::Id::new("fib-editor-drawing-color")))
        .unwrap_or(super::DEFAULT_DRAWING_COLOR)
}

/// Stash the drawing colour for [`drawing_color_of`].
pub(super) fn remember_drawing_color(ui: &egui::Ui, color: egui::Color32) {
    ui.data_mut(|data| data.insert_temp(egui::Id::new("fib-editor-drawing-color"), color));
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f64 = 1e-9;

    fn close(left: f64, right: f64) -> bool {
        (left - right).abs() < EPS
    }

    #[test]
    fn retracement_formula_is_exact_in_up_and_down_moves() {
        // Down move: A above B (the prototype's 71,824.50 -> 69,111.50).
        let (a, b) = (71_824.50, 69_111.50);
        let golden = [
            (0.0, 71_824.50),
            (0.236, 71_184.232),
            (0.382, 70_788.134),
            (0.5, 70_468.0),
            (0.618, 70_147.866),
            (0.786, 69_692.082),
            (1.0, 69_111.50),
        ];
        for (ratio, expected) in golden {
            let price = level_price(FibKind::Retracement, false, a, b, b, ratio).unwrap();
            assert!(
                close(price, expected),
                "ratio {ratio}: {price} vs {expected}"
            );
        }
        // Up move mirrors exactly.
        let price = level_price(FibKind::Retracement, false, 100.0, 200.0, 200.0, 0.618).unwrap();
        assert!(close(price, 161.8));
    }

    #[test]
    fn extension_projects_the_impulse_from_origin_c_and_reverse_keeps_c() {
        let (a, b, c) = (100.0, 150.0, 120.0);
        let price = level_price(FibKind::Extension, false, a, b, c, 1.618).unwrap();
        assert!(close(price, 120.0 + 50.0 * 1.618));
        // Reverse swaps A and B and never moves C.
        let reversed = level_price(FibKind::Extension, false, b, a, c, 1.618).unwrap();
        assert!(close(reversed, 120.0 - 50.0 * 1.618));
    }

    #[test]
    fn log_formulas_compound_the_ratio_of_prices() {
        // Retracement: P(r) = PA * (PB/PA)^r.
        let price = level_price(FibKind::Retracement, true, 100.0, 400.0, 400.0, 0.5).unwrap();
        assert!(close(price, 200.0), "half the log move of 100->400 is 200");
        // Extension: P(r) = PC * (PB/PA)^r.
        let projected = level_price(FibKind::Extension, true, 100.0, 400.0, 50.0, 0.5).unwrap();
        assert!(close(projected, 100.0));
        // Reverse swaps A/B: the ratio of prices inverts.
        let reversed = level_price(FibKind::Retracement, true, 400.0, 100.0, 100.0, 0.5).unwrap();
        assert!(close(reversed, 200.0));
    }

    #[test]
    fn log_refuses_non_positive_prices_instead_of_guessing() {
        assert!(level_price(FibKind::Retracement, true, 0.0, 10.0, 10.0, 0.5).is_none());
        assert!(level_price(FibKind::Extension, true, 10.0, 20.0, -5.0, 0.5).is_none());
        assert!(!log_scale_allowed(-1.0, 2.0, 3.0));
        // Linear never needs the guard.
        assert!(level_price(FibKind::Retracement, false, -10.0, 10.0, 10.0, 0.5).is_some());
    }

    #[test]
    fn the_ratio_parser_reads_ratios_percents_and_decimal_commas() {
        assert!(close(parse_ratio(".618").unwrap(), 0.618));
        assert!(close(parse_ratio("0.618").unwrap(), 0.618));
        assert!(close(parse_ratio("61.8%").unwrap(), 0.618));
        assert!(close(parse_ratio("0,618").unwrap(), 0.618));
        assert!(close(parse_ratio("61,8 %").unwrap(), 0.618));
        assert!(close(parse_ratio("-0.272").unwrap(), -0.272));
        assert!(close(parse_ratio("161.8%").unwrap(), 1.618));
        assert!(parse_ratio("abc").is_none());
        assert!(parse_ratio("").is_none());
        assert!(parse_ratio("inf").is_none());
    }

    #[test]
    fn presets_start_every_fib_and_round_trip_through_the_preset_format() {
        let mut payload = FibPayload::new(FibKind::Retracement);
        assert_eq!(payload.levels.len(), 7, "Standard retracement has 7 levels");
        payload.levels[1].label = "golden".to_owned();
        payload.levels[1].color = Some([255, 159, 67]);
        payload.levels[1].fill_to_next = true;
        payload.band_alpha = 40;
        payload.label_mode = LabelMode::RatioPrice;
        payload.extend = Extend::Right;

        let exported = payload.export_preset().expect("fib exports its preset");
        let mut restored = FibPayload::new(FibKind::Retracement);
        assert!(restored.import_preset(&exported));
        assert_eq!(restored, payload, "the preset format round-trips");

        // A preset of the other kind is refused, not misapplied.
        let mut extension = FibPayload::new(FibKind::Extension);
        assert!(!extension.import_preset(&exported));
    }

    #[test]
    fn imports_reject_broken_or_future_preset_files() {
        let mut payload = FibPayload::new(FibKind::Retracement);
        let mut exported = payload.export_preset().unwrap();
        let table = exported.as_table_mut().unwrap();
        table.insert("version".into(), toml::Value::Integer(99));
        assert!(
            !payload.import_preset(&exported),
            "an unknown version must not half-apply"
        );
    }

    #[test]
    fn duplicate_ratios_within_tolerance_are_rejected() {
        let payload = FibPayload::new(FibKind::Retracement);
        assert!(payload.is_duplicate(0.618, None));
        assert!(payload.is_duplicate(0.618 + RATIO_DUPLICATE_TOLERANCE / 2.0, None));
        assert!(
            !payload.is_duplicate(0.618, Some(4)),
            "a level may keep its own ratio"
        );
        assert!(!payload.is_duplicate(0.9, None));
    }

    #[test]
    fn labels_follow_the_mode_and_custom_text() {
        let level = FibLevelSpec::new(0.618);
        assert_eq!(
            format_label(LabelMode::PercentPrice, &level, 70_147.866),
            "61.8% \u{00b7} 70147.87"
        );
        assert_eq!(
            format_label(LabelMode::RatioPrice, &level, 70_147.866),
            "0.618 \u{00b7} 70147.87"
        );
        assert_eq!(
            format_label(LabelMode::PriceOnly, &level, 70_147.866),
            "70147.87"
        );
        let mut named = FibLevelSpec::new(0.618);
        named.label = "golden".to_owned();
        assert_eq!(
            format_label(LabelMode::PercentPrice, &named, 1.0),
            "golden \u{00b7} 1.00"
        );
    }

    #[test]
    fn the_label_cache_rebuilds_only_when_prices_or_config_change() {
        let mut payload = FibPayload::new(FibKind::Retracement);
        let first = labels_for(&payload, 200.0, 100.0, 100.0, false);
        assert_eq!(first.len(), 7);
        let again = labels_for(&payload, 200.0, 100.0, 100.0, false);
        assert_eq!(first, again);
        // A config edit bumps the revision and the labels follow.
        payload.levels[0].label = "top".to_owned();
        payload.touch();
        let after = labels_for(&payload, 200.0, 100.0, 100.0, false);
        assert!(after[0].starts_with("top"));
    }
}
