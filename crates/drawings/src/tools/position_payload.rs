//! A position's own state and the edits its floating bar and the control
//! plane make to it: the ratio, the two zone colours, the zone opacity and
//! the drawn width.
//!
//! `ratio` is always the ratio the plan's three prices make. While the
//! projected leg is free it places that leg; once a drag pins the leg,
//! `ratio` follows the pin, so a saved default stores what the trader sees.

use std::any::Any;

use serde::{Deserialize, Serialize};

use super::DrawingPayload;
use super::position_core::{Legs, MAX_RATIO, MIN_RATIO, MIN_WIDTH_BARS, Side, legs};
use crate::ChartPoint;

/// The ratios the risk/reward menu offers, as reward per unit of risk.
pub const RATIO_PRESETS: [f64; 3] = [1.0, 2.0, 3.0];
/// The ratio a fresh position opens with, until the trader saves another
/// as the tool's default.
pub const DEFAULT_RATIO: f64 = 1.0;
/// Factory zone colours, the same on every theme: a position is the
/// trader's own plan, not candle ink, so it never takes `Buy`/`Sell`.
pub const FACTORY_TARGET_RGB: Rgb = [0x2E, 0xA0, 0x64];
pub const FACTORY_STOP_RGB: Rgb = [0xD6, 0x30, 0x31];
/// Factory zone opacity, in percent.
pub const FACTORY_ZONE_OPACITY_PCT: u8 = 10;
/// The most a zone may be filled, in percent: a solid zone hides the bars.
pub const MAX_ZONE_OPACITY_PCT: u8 = 100;
/// Two ratios this close are one preset.
const PRESET_TOLERANCE: f64 = 1e-6;
const FORMAT_VERSION: u32 = 1;

/// Straight sRGB bytes.
pub type Rgb = [u8; 3];

/// The position's own state; see the module docs for `ratio`.
#[derive(Debug, Clone, PartialEq)]
pub struct PositionPayload {
    pub ratio: f64,
    /// Once the trader drags the projected leg, its distance from the entry
    /// — an offset, so moving the whole plan carries the leg with it.
    pub projected_offset: Option<f64>,
    pub target_rgb: Rgb,
    pub stop_rgb: Rgb,
    pub zone_opacity_pct: u8,
    /// The drawn columns once the trader resized the width: `[left, right]`
    /// from the entry anchor, in units of the anchors' own span (one bar
    /// when they share a bar). A unit of the anchors projects with them, so
    /// the box keeps its length in time across a timeframe switch and on a
    /// pane mirroring it. `None` spans the anchors.
    pub span: Option<[f32; 2]>,
}

impl Default for PositionPayload {
    fn default() -> Self {
        Self {
            ratio: DEFAULT_RATIO,
            projected_offset: None,
            target_rgb: FACTORY_TARGET_RGB,
            stop_rgb: FACTORY_STOP_RGB,
            zone_opacity_pct: FACTORY_ZONE_OPACITY_PCT,
            span: None,
        }
    }
}

/// Why an edit was refused; nothing changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PositionRefusal {
    RatioOutOfRange,
    OpacityOutOfRange,
    WidthOutOfRange,
    /// The anchors make no plan: entry and edge at one price.
    NoPlan,
}

impl PositionRefusal {
    #[must_use]
    pub fn reason(self) -> &'static str {
        match self {
            Self::RatioOutOfRange => "ratio must be between 0.1 and 20 reward per unit of risk",
            Self::OpacityOutOfRange => "zone opacity must be between 0 and 100 percent",
            Self::WidthOutOfRange => "width must be between 14 and 100000 bars",
            Self::NoPlan => "the position's entry and edge are at one price",
        }
    }
}

pub(super) fn valid_ratio(ratio: f64) -> bool {
    (MIN_RATIO..=MAX_RATIO).contains(&ratio)
}

/// The preset `ratio` is, if it is one.
#[must_use]
pub fn preset_of(ratio: f64) -> Option<f64> {
    RATIO_PRESETS
        .into_iter()
        .find(|preset| (preset - ratio).abs() <= PRESET_TOLERANCE)
}

impl PositionPayload {
    /// The pinned projected price, for a plan entered at `entry`.
    #[must_use]
    pub fn pinned(&self, entry: f64) -> Option<f64> {
        self.projected_offset.map(|offset| entry + offset)
    }
    /// The zone fill alpha its opacity makes.
    #[must_use]
    pub fn zone_alpha(&self) -> u8 {
        (u16::from(self.zone_opacity_pct.min(MAX_ZONE_OPACITY_PCT)) * 255 / 100) as u8
    }
    /// Keep `ratio` the ratio `plan` makes, held to the readable range.
    pub(super) fn follow(&mut self, plan: &Legs) {
        let ratio = plan.ratio();
        if ratio.is_finite() {
            self.ratio = ratio.clamp(MIN_RATIO, MAX_RATIO);
        }
    }
    /// Set the zone opacity, in percent.
    ///
    /// # Errors
    /// Above [`MAX_ZONE_OPACITY_PCT`].
    pub fn set_zone_opacity(&mut self, pct: u8) -> Result<(), PositionRefusal> {
        if pct > MAX_ZONE_OPACITY_PCT {
            return Err(PositionRefusal::OpacityOutOfRange);
        }
        self.zone_opacity_pct = pct;
        Ok(())
    }
    /// The drawn width in bars on the anchors' own chart.
    #[must_use]
    pub fn width_bars(&self, anchors: &[ChartPoint]) -> f32 {
        let width = match (self.span, anchors) {
            (Some([left, right]), _) => (right - left) * span_unit_bars(anchors),
            (None, [entry, edge, ..]) => (edge.bar - entry.bar).abs(),
            (None, _) => 0.0,
        };
        width.max(MIN_WIDTH_BARS)
    }
    /// Make the position `bars` wide, its left edge where it is.
    ///
    /// # Errors
    /// A width that is not finite or narrower than [`MIN_WIDTH_BARS`].
    pub fn set_width_bars(
        &mut self,
        anchors: &[ChartPoint],
        bars: f32,
    ) -> Result<(), PositionRefusal> {
        if !(MIN_WIDTH_BARS..=MAX_WIDTH_BARS).contains(&bars) {
            return Err(PositionRefusal::WidthOutOfRange);
        }
        let unit = span_unit_bars(anchors);
        let left = match (self.span, anchors) {
            (Some([left, _]), _) => left,
            (None, [entry, edge, ..]) => (edge.bar - entry.bar).min(0.0) / unit,
            (None, _) => 0.0,
        };
        self.span = Some([left, left + bars / unit]);
        Ok(())
    }
}

/// The widest a position is drawn, in bars.
pub const MAX_WIDTH_BARS: f32 = 100_000.0;
/// Below this the anchors share a bar, and a span unit is one bar.
pub(super) const SAME_BAR: f32 = 1e-3;

/// One unit of [`PositionPayload::span`], in bars of the anchors' chart.
pub(super) fn span_unit_bars(anchors: &[ChartPoint]) -> f32 {
    match anchors {
        [entry, edge, ..] if (edge.bar - entry.bar).abs() > SAME_BAR => {
            (edge.bar - entry.bar).abs()
        }
        _ => 1.0,
    }
}

/// Set the plan's ratio: the target moves, the entry and the stop never do,
/// whichever leg the trader measured when drawing. The edge anchor becomes
/// the stop, so the target is the projected leg and follows `ratio`.
///
/// # Errors
/// A ratio outside the readable range, or anchors that make no plan.
pub fn set_plan_ratio(
    side: Side,
    anchors: &mut [ChartPoint],
    payload: &mut PositionPayload,
    ratio: f64,
) -> Result<(), PositionRefusal> {
    if !valid_ratio(ratio) {
        return Err(PositionRefusal::RatioOutOfRange);
    }
    let [entry, edge, ..] = anchors else {
        return Err(PositionRefusal::NoPlan);
    };
    let plan = legs(
        side,
        entry.price,
        edge.price,
        payload.ratio,
        payload.pinned(entry.price),
    )
    .ok_or(PositionRefusal::NoPlan)?;
    edge.price = plan.stop;
    payload.ratio = ratio;
    payload.projected_offset = None;
    Ok(())
}

/// `#RRGGBB`.
#[must_use]
pub fn rgb_hex([r, g, b]: Rgb) -> String {
    format!("#{r:02X}{g:02X}{b:02X}")
}

/// `#RRGGBB`, either case, the `#` required.
#[must_use]
pub fn parse_rgb_hex(text: &str) -> Option<Rgb> {
    let digits = text.strip_prefix('#')?;
    if digits.len() != 6 || !digits.is_ascii() {
        return None;
    }
    let byte = |at: usize| u8::from_str_radix(&digits[at..at + 2], 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?])
}

/// The look a preset and a saved default carry. Every field is optional on
/// read: a payload written before colours existed takes the factory ones.
#[derive(Serialize, Deserialize)]
pub(super) struct PresetData {
    pub version: u32,
    pub ratio: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zone_opacity_pct: Option<u8>,
}

#[derive(Serialize, Deserialize)]
struct SavedData {
    #[serde(flatten)]
    look: PresetData,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    projected_offset: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    span: Option<[f32; 2]>,
}

impl PositionPayload {
    fn look(&self) -> PresetData {
        PresetData {
            version: FORMAT_VERSION,
            ratio: self.ratio,
            target_color: Some(rgb_hex(self.target_rgb)),
            stop_color: Some(rgb_hex(self.stop_rgb)),
            zone_opacity_pct: Some(self.zone_opacity_pct),
        }
    }
    /// The look `data` states, or `None` for one this version cannot read.
    fn read_look(data: &PresetData) -> Option<(f64, Rgb, Rgb, u8)> {
        let colour = |text: &Option<String>, factory| match text {
            Some(text) => parse_rgb_hex(text),
            None => Some(factory),
        };
        let opacity = data.zone_opacity_pct.unwrap_or(FACTORY_ZONE_OPACITY_PCT);
        (data.version == FORMAT_VERSION
            && valid_ratio(data.ratio)
            && opacity <= MAX_ZONE_OPACITY_PCT)
            .then_some(())?;
        Some((
            data.ratio,
            colour(&data.target_color, FACTORY_TARGET_RGB)?,
            colour(&data.stop_color, FACTORY_STOP_RGB)?,
            opacity,
        ))
    }
    fn take_look(&mut self, (ratio, target, stop, opacity): (f64, Rgb, Rgb, u8)) {
        self.ratio = ratio;
        self.target_rgb = target;
        self.stop_rgb = stop;
        self.zone_opacity_pct = opacity;
    }
}

impl DrawingPayload for PositionPayload {
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
    /// A preset is the look: ratio, colours and opacity. A pinned leg and a
    /// width are one chart's geometry.
    fn export_preset(&self) -> Option<toml::Value> {
        toml::Value::try_from(self.look()).ok()
    }
    fn import_preset(&mut self, value: &toml::Value) -> bool {
        let Some(look) = PresetData::deserialize(value.clone())
            .ok()
            .and_then(|data| Self::read_look(&data))
        else {
            return false;
        };
        self.take_look(look);
        true
    }
    fn export_saved(&self) -> Option<toml::Value> {
        toml::Value::try_from(SavedData {
            look: self.look(),
            projected_offset: self.projected_offset,
            span: self.span,
        })
        .ok()
    }
    fn import_saved(&mut self, value: &toml::Value) -> bool {
        let Ok(data) = SavedData::deserialize(value.clone()) else {
            return false;
        };
        let Some(look) = Self::read_look(&data.look) else {
            return false;
        };
        // The width floor is the painter's: in span units it depends on the
        // anchors, which a payload does not hold.
        let finite_span = data
            .span
            .is_none_or(|[left, right]| left.is_finite() && right.is_finite() && right > left);
        if data
            .projected_offset
            .is_some_and(|offset| !offset.is_finite())
            || !finite_span
        {
            return false;
        }
        self.take_look(look);
        self.projected_offset = data.projected_offset;
        self.span = data.span;
        true
    }
}

#[cfg(test)]
#[path = "position_payload_tests.rs"]
mod tests;
