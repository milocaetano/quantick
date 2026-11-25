//! The position drawing the long and short tools share: an entry line, a
//! stop zone and a target zone, planned on the chart and never sent.
//!
//! Two anchors hold the trader's measurement — the entry where the drag
//! started and the edge where it ended. Which leg that edge is follows from
//! the side alone: on the trade's profit side it is the target, on its loss
//! side the stop. The other leg is projected by the payload's ratio
//! (target : stop) until the trader drags it, which pins it at its price.
//!
//! No tick count and no tick snap: a drawing does not know the instrument's
//! tick size (`measure_core.rs` states the same honesty rule), so legs read
//! in points and the projected price is the exact ratio.

use std::any::Any;

use quantick_appearance::{FontRole, ResolvedTheme, Token};
use quantick_chart::style::PremultipliedRgba;
use serde::{Deserialize, Serialize};

use crate::{Align2, FontSpec, MarkScene, Px, PxRect, RgbaExt as _, Stroke, pos2, vec2};

use super::{
    Constrain, DrawContext, DrawingPayload, DrawingStyle, Handles, MarkTool, PointerHint,
    ToolFamily, dashed_segment_within,
};

/// The ratios one click on the plan tag steps through, in order.
pub const RATIO_PRESETS: [f64; 4] = [1.0, 1.5, 2.0, 3.0];
/// The ratio a fresh position opens with, until the trader saves another
/// as the tool's default.
pub const DEFAULT_RATIO: f64 = 2.0;
/// The ratios a saved or imported payload may hold. Outside them a plan is
/// unreadable on screen — one leg collapses onto the entry line.
pub const MIN_RATIO: f64 = 0.1;
pub const MAX_RATIO: f64 = 20.0;
/// The narrowest a position is drawn, in bars: a drag on one bar still
/// leaves zones wide enough to read.
pub const MIN_WIDTH_BARS: f32 = 14.0;

/// The projected leg's handle; the entry and measured edge lines follow.
pub(super) const PROJECTED_HANDLE: usize = 2;
pub(super) const ENTRY_LINE_HANDLE: usize = 3;
pub(super) const MEASURED_LINE_HANDLE: usize = 4;

const GRAB_PX: f32 = 5.0;
const LABEL_TEXT_PX: f32 = 10.0;
const LABEL_PAD_X_PX: f32 = 5.0;
const LABEL_PAD_Y_PX: f32 = 2.0;
const LABEL_RADIUS_PX: f32 = 3.0;
const LABEL_INSET_PX: f32 = 4.0;
/// The plan tag's click target, from the position's left edge: wide enough
/// for `Plan short · 2.38:1` at the label size, so a click on the side or
/// the ratio lands without measuring the words (`click` has no metrics).
const TAG_HIT_WIDTH_PX: f32 = 140.0;
const TAG_HIT_HALF_HEIGHT_PX: f32 = 9.0;
const PROJECTED_DASH_PX: f32 = 5.0;
const PROJECTED_GAP_PX: f32 = 4.0;
const PRESET_FORMAT_VERSION: u32 = 1;

pub(super) const POSITION_FAMILY: ToolFamily = ToolFamily {
    id: "position",
    title: "Position",
    icon: crate::icons::ARROWS_VERTICAL,
    icon_strokes: LONG_ICON,
    icon_dots: &[],
    icon_letter: None,
};

/// A box split by the entry, the arrow pointing to the target.
pub(super) const LONG_ICON: super::IconStrokes = &[
    &[
        (0.15, 0.12),
        (0.85, 0.12),
        (0.85, 0.88),
        (0.15, 0.88),
        (0.15, 0.12),
    ],
    &[(0.15, 0.62), (0.85, 0.62)],
    &[(0.5, 0.52), (0.5, 0.24)],
    &[(0.38, 0.36), (0.5, 0.24), (0.62, 0.36)],
];
pub(super) const SHORT_ICON: super::IconStrokes = &[
    &[
        (0.15, 0.12),
        (0.85, 0.12),
        (0.85, 0.88),
        (0.15, 0.88),
        (0.15, 0.12),
    ],
    &[(0.15, 0.38), (0.85, 0.38)],
    &[(0.5, 0.48), (0.5, 0.76)],
    &[(0.38, 0.64), (0.5, 0.76), (0.62, 0.64)],
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Long,
    Short,
}

impl Side {
    /// `+1` when a higher price is profit.
    fn sign(self) -> f64 {
        match self {
            Self::Long => 1.0,
            Self::Short => -1.0,
        }
    }
    fn word(self) -> &'static str {
        match self {
            Self::Long => "long",
            Self::Short => "short",
        }
    }
}

/// Which leg the trader measured; the other one is projected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Measured {
    Stop,
    Target,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Legs {
    pub entry: f64,
    pub stop: f64,
    pub target: f64,
    pub measured: Measured,
}

impl Legs {
    #[must_use]
    pub fn risk(&self) -> f64 {
        (self.entry - self.stop).abs()
    }
    #[must_use]
    pub fn reward(&self) -> f64 {
        (self.target - self.entry).abs()
    }
    /// Target : stop, as the plan stands — the chosen ratio, or the one a
    /// dragged leg left behind.
    #[must_use]
    pub fn ratio(&self) -> f64 {
        self.reward() / self.risk()
    }
    #[must_use]
    pub fn projected(&self) -> f64 {
        match self.measured {
            Measured::Stop => self.target,
            Measured::Target => self.stop,
        }
    }
}

/// The plan an entry and a measured edge make on `side`: the edge is the
/// target on the profit side and the stop on the loss side, and the other
/// leg sits `ratio` times further (or `1 / ratio` nearer) from the entry,
/// unless `pinned` holds it at a price still on its own side. `None` for a
/// flat measurement, which says neither where the stop is nor the target.
#[must_use]
pub fn legs(side: Side, entry: f64, edge: f64, ratio: f64, pinned: Option<f64>) -> Option<Legs> {
    if !(entry.is_finite() && edge.is_finite() && ratio.is_finite() && ratio > 0.0) {
        return None;
    }
    let toward_profit = (edge - entry) * side.sign();
    if toward_profit == 0.0 {
        return None;
    }
    let height = toward_profit.abs();
    let (measured, projected_offset) = if toward_profit > 0.0 {
        (Measured::Target, -height / ratio)
    } else {
        (Measured::Stop, height * ratio)
    };
    let projected = pinned
        .filter(|price| {
            let offset = (price - entry) * side.sign();
            price.is_finite() && offset != 0.0 && offset.signum() == projected_offset.signum()
        })
        .unwrap_or(entry + projected_offset * side.sign());
    let (stop, target) = match measured {
        Measured::Stop => (edge, projected),
        Measured::Target => (projected, edge),
    };
    Some(Legs {
        entry,
        stop,
        target,
        measured,
    })
}

/// The preset after `current`, wrapping to the first past the last.
#[must_use]
pub fn next_ratio(current: f64) -> f64 {
    RATIO_PRESETS
        .into_iter()
        .find(|preset| *preset > current + 1e-9)
        .unwrap_or(RATIO_PRESETS[0])
}

/// `2:1`, `1.5:1`, `2.37:1` — at most two decimals, none when whole.
#[must_use]
pub fn ratio_label(ratio: f64) -> String {
    let mut digits = format!("{ratio:.2}");
    while digits.ends_with('0') {
        digits.pop();
    }
    if digits.ends_with('.') {
        digits.pop();
    }
    format!("{digits}:1")
}

fn valid_ratio(ratio: f64) -> bool {
    (MIN_RATIO..=MAX_RATIO).contains(&ratio)
}

/// The position's own state: the ratio its projected leg follows, and,
/// once the trader drags a line, that leg's distance from the entry — an
/// offset, so moving the whole plan carries the leg with it.
#[derive(Debug, Clone, PartialEq)]
pub struct PositionPayload {
    pub ratio: f64,
    pub projected_offset: Option<f64>,
}

impl Default for PositionPayload {
    fn default() -> Self {
        Self {
            ratio: DEFAULT_RATIO,
            projected_offset: None,
        }
    }
}

impl PositionPayload {
    /// The pinned projected price, for a plan entered at `entry`.
    #[must_use]
    pub fn pinned(&self, entry: f64) -> Option<f64> {
        self.projected_offset.map(|offset| entry + offset)
    }
    /// Step to the next ratio preset and let the projected leg follow it.
    pub fn cycle_ratio(&mut self) {
        self.ratio = next_ratio(self.ratio);
        self.projected_offset = None;
    }
}

#[derive(Serialize, Deserialize)]
struct PresetData {
    version: u32,
    ratio: f64,
}

#[derive(Serialize, Deserialize)]
struct SavedData {
    version: u32,
    ratio: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    projected_offset: Option<f64>,
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
    /// A preset is the ratio only: a pinned leg is a price on one chart.
    fn export_preset(&self) -> Option<toml::Value> {
        toml::Value::try_from(PresetData {
            version: PRESET_FORMAT_VERSION,
            ratio: self.ratio,
        })
        .ok()
    }
    fn import_preset(&mut self, value: &toml::Value) -> bool {
        let Ok(data) = PresetData::deserialize(value.clone()) else {
            return false;
        };
        if data.version != PRESET_FORMAT_VERSION || !valid_ratio(data.ratio) {
            return false;
        }
        self.ratio = data.ratio;
        true
    }
    fn export_saved(&self) -> Option<toml::Value> {
        toml::Value::try_from(SavedData {
            version: PRESET_FORMAT_VERSION,
            ratio: self.ratio,
            projected_offset: self.projected_offset,
        })
        .ok()
    }
    fn import_saved(&mut self, value: &toml::Value) -> bool {
        let Ok(data) = SavedData::deserialize(value.clone()) else {
            return false;
        };
        if data.version != PRESET_FORMAT_VERSION
            || !valid_ratio(data.ratio)
            || data
                .projected_offset
                .is_some_and(|offset| !offset.is_finite())
        {
            return false;
        }
        self.ratio = data.ratio;
        self.projected_offset = data.projected_offset;
        true
    }
}

fn payload_of(ctxt: &DrawContext<'_>) -> PositionPayload {
    ctxt.payload
        .as_any()
        .downcast_ref::<PositionPayload>()
        .cloned()
        .unwrap_or_default()
}

/// One position as it stands on screen.
struct Frame {
    legs: Option<Legs>,
    left: f32,
    right: f32,
    entry_y: f32,
    stop_y: f32,
    target_y: f32,
}

impl Frame {
    fn projected_y(&self) -> Option<f32> {
        let legs = self.legs?;
        Some(match legs.measured {
            Measured::Stop => self.target_y,
            Measured::Target => self.stop_y,
        })
    }
    fn measured_y(&self) -> Option<f32> {
        let legs = self.legs?;
        Some(match legs.measured {
            Measured::Stop => self.stop_y,
            Measured::Target => self.target_y,
        })
    }
    fn spans(&self, x: f32) -> bool {
        x >= self.left - GRAB_PX && x <= self.right + GRAB_PX
    }
}

fn frame(points: &[Px], ctxt: &DrawContext<'_>, side: Side) -> Option<Frame> {
    let ([a, b, ..], [entry, edge, ..]) = (points, ctxt.anchors) else {
        return None;
    };
    let payload = payload_of(ctxt);
    let legs = legs(
        side,
        entry.price,
        edge.price,
        payload.ratio,
        payload.pinned(entry.price),
    );
    let left = a.x.min(b.x);
    let right =
        a.x.max(b.x)
            .max(left + MIN_WIDTH_BARS * ctxt.px_per_bar.max(0.0));
    let y = |price: f64| ctxt.scale.y(price);
    Some(Frame {
        legs,
        left,
        right,
        entry_y: a.y,
        stop_y: legs.map_or(b.y, |legs| y(legs.stop)),
        target_y: legs.map_or(b.y, |legs| y(legs.target)),
    })
}

fn tinted(color: PremultipliedRgba, alpha: u8) -> PremultipliedRgba {
    crate::rgba_unmultiplied(color.r(), color.g(), color.b(), alpha)
}

fn points_text(delta: f64) -> String {
    format!(
        "{} pts",
        quantick_engine::display::display_signed_f64(delta)
    )
}

/// What the plan tag says: the side, the ratio and the entry — `Plan`
/// first, so a drawing is never read as an open position, and the ratio
/// inside the tag's click target.
fn plan_tag(side: Side, legs: Option<&Legs>) -> String {
    match legs {
        Some(legs) => format!(
            "Plan {} · {} · entry {}",
            side.word(),
            ratio_label(legs.ratio()),
            quantick_engine::display::display_f64(legs.entry)
        ),
        None => format!("Plan {} · drag a stop or a target", side.word()),
    }
}

fn label(
    painter: &mut MarkScene,
    ctxt: &DrawContext<'_>,
    at: Px,
    anchor: Align2,
    text: &str,
    plate: PremultipliedRgba,
) {
    let appearance: &ResolvedTheme = ctxt.appearance;
    let font = FontSpec::themed(appearance, FontRole::Monospace, LABEL_TEXT_PX);
    let size = ctxt.text.size(text, font) + vec2(2.0 * LABEL_PAD_X_PX, 2.0 * LABEL_PAD_Y_PX);
    let rect = anchor.anchor_size(at, size);
    painter.rect_filled(rect, LABEL_RADIUS_PX, plate);
    painter.text(
        rect.min + vec2(LABEL_PAD_X_PX, LABEL_PAD_Y_PX),
        Align2::LEFT_TOP,
        text,
        font,
        appearance.ink_on(plate),
    );
}

pub(super) fn paint(
    painter: &mut MarkScene,
    chart_rect: PxRect,
    style: DrawingStyle,
    points: &[Px],
    ctxt: &DrawContext<'_>,
    side: Side,
) {
    let Some(frame) = frame(points, ctxt, side) else {
        if let [a, b, ..] = points {
            painter.line_segment([*a, *b], crate::drawing_stroke(style));
        }
        return;
    };
    let stop_ink = ctxt.appearance.color(Token::Sell);
    let target_ink = ctxt.appearance.color(Token::Buy);
    let (left, right) = (frame.left, frame.right);
    let zone = |y: f32| PxRect::from_two_pos(pos2(left, frame.entry_y), pos2(right, y));
    if !ctxt.halo && frame.legs.is_some() {
        painter.rect_filled(zone(frame.stop_y), 0.0, tinted(stop_ink, style.fill_alpha));
        painter.rect_filled(
            zone(frame.target_y),
            0.0,
            tinted(target_ink, style.fill_alpha),
        );
    }
    if let Some(legs) = frame.legs {
        for (y, ink, measured) in [
            (frame.stop_y, stop_ink, legs.measured == Measured::Stop),
            (
                frame.target_y,
                target_ink,
                legs.measured == Measured::Target,
            ),
        ] {
            let stroke = Stroke::new(style.width_px, ink);
            let ends = [pos2(left, y), pos2(right, y)];
            if measured {
                painter.line_segment(ends, stroke);
            } else {
                dashed_segment_within(
                    painter,
                    chart_rect,
                    ends[0],
                    ends[1],
                    stroke,
                    PROJECTED_DASH_PX,
                    PROJECTED_GAP_PX,
                );
            }
        }
    }
    let entry = [pos2(left, frame.entry_y), pos2(right, frame.entry_y)];
    painter.line_segment(entry, crate::drawing_stroke(style));
    if ctxt.halo || !ctxt.primary_band {
        return;
    }
    let plate = ctxt.appearance.color(Token::DrawingReadoutPlate);
    if let Some(legs) = frame.legs {
        // Inside the zone's far edge, held on the chart when that edge is off
        // it — a target projected past the top still reads — and absent when
        // the whole zone is: no chip for a level that is not in view.
        let (top, bottom) = (chart_rect.top(), chart_rect.bottom());
        let shown = |y: f32| y.max(frame.entry_y) >= top && y.min(frame.entry_y) <= bottom;
        let inside = |y: f32, other: f32| {
            let below = y > other;
            let offset = if below {
                -LABEL_INSET_PX
            } else {
                LABEL_INSET_PX
            };
            let y = (y + offset).clamp(top, bottom.max(top));
            let anchor = if y <= top {
                Align2::RIGHT_TOP
            } else if y >= bottom || below {
                Align2::RIGHT_BOTTOM
            } else {
                Align2::RIGHT_TOP
            };
            (pos2(right - LABEL_INSET_PX, y), anchor)
        };
        let (at, anchor) = inside(frame.target_y, frame.entry_y);
        if shown(frame.target_y) {
            let target = format!(
                "TP {} · {}",
                quantick_engine::display::display_f64(legs.target),
                points_text(legs.reward())
            );
            label(
                painter,
                ctxt,
                at,
                anchor,
                &target,
                tinted(target_ink, u8::MAX),
            );
        }
        if shown(frame.stop_y) {
            let (at, anchor) = inside(frame.stop_y, frame.entry_y);
            let stop = format!(
                "SL {} · {}",
                quantick_engine::display::display_f64(legs.stop),
                points_text(-legs.risk())
            );
            label(painter, ctxt, at, anchor, &stop, tinted(stop_ink, u8::MAX));
        }
    }
    let tag = plan_tag(side, frame.legs.as_ref());
    let at = pos2(left + LABEL_INSET_PX, frame.entry_y);
    label(painter, ctxt, at, Align2::LEFT_CENTER, &tag, plate);
}

pub(super) fn hit_test(
    points: &[Px],
    position: Px,
    radius_px: f32,
    ctxt: &DrawContext<'_>,
    side: Side,
) -> bool {
    let Some(frame) = frame(points, ctxt, side) else {
        return matches!(points, [a, b, ..] if super::distance_to_segment(position, *a, *b) <= radius_px);
    };
    let top = frame.entry_y.min(frame.stop_y).min(frame.target_y);
    let bottom = frame.entry_y.max(frame.stop_y).max(frame.target_y);
    PxRect::from_min_max(pos2(frame.left, top), pos2(frame.right, bottom))
        .expand(radius_px)
        .contains(position)
}

/// The line under `position`, as the handle a press there grabs: the
/// projected leg first (it is the one the trader adjusts), then the entry,
/// then the measured edge.
fn line_at(points: &[Px], position: Px, ctxt: &DrawContext<'_>, side: Side) -> Option<usize> {
    let frame = frame(points, ctxt, side)?;
    if !frame.spans(position.x) {
        return None;
    }
    let near = |y: Option<f32>| y.is_some_and(|y| (position.y - y).abs() <= GRAB_PX);
    if near(frame.projected_y()) {
        Some(PROJECTED_HANDLE)
    } else if near(Some(frame.entry_y)) {
        Some(ENTRY_LINE_HANDLE)
    } else if near(frame.measured_y()) {
        Some(MEASURED_LINE_HANDLE)
    } else {
        None
    }
}

pub(super) fn pointer_hint(
    points: &[Px],
    position: Px,
    ctxt: &DrawContext<'_>,
    side: Side,
) -> Option<PointerHint> {
    line_at(points, position, ctxt, side).map(|handle| PointerHint::ResizeSide { handle })
}

/// A press on one of the three lines announces the line it grabs.
pub(super) fn line_hint(handle: usize) -> Option<PointerHint> {
    matches!(
        handle,
        PROJECTED_HANDLE | ENTRY_LINE_HANDLE | MEASURED_LINE_HANDLE
    )
    .then_some(PointerHint::ResizeSide { handle })
}

/// A line grab moves that line's price and nothing else: the entry and the
/// measured edge keep their bars, so a press mid-line never jumps an anchor
/// sideways to the pointer. The anchor dots keep the plain move.
fn drag_line(points: &[Px], handle: usize, to: Px) -> Option<Handles> {
    let anchor = match handle {
        ENTRY_LINE_HANDLE => 0,
        MEASURED_LINE_HANDLE => 1,
        _ => return None,
    };
    let mut moved: Handles = points.iter().copied().collect();
    moved.get_mut(anchor)?.y = to.y;
    Some(moved)
}

/// Dragging the projected leg pins it at the price under the pointer, on
/// its own side of the entry; the ratio the tag shows becomes the one the
/// three prices make.
pub(super) fn drag_projected(
    points: &[Px],
    handle: usize,
    to: Px,
    ctxt: &DrawContext<'_>,
    side: Side,
) -> Option<Box<dyn DrawingPayload>> {
    if handle != PROJECTED_HANDLE {
        return None;
    }
    frame(points, ctxt, side)?;
    let [entry, edge, ..] = ctxt.anchors else {
        return None;
    };
    let mut payload = payload_of(ctxt);
    let plan = |pinned| legs(side, entry.price, edge.price, payload.ratio, pinned);
    let held = plan(payload.pinned(entry.price))?.projected();
    let price = ctxt.scale.price_at(to.y);
    // `legs` keeps a pin only on its own side of the entry; past it, the
    // leg stays where it was.
    let dragged = plan(Some(price))?.projected();
    let projected = if dragged == price { price } else { held };
    payload.projected_offset = Some(projected - entry.price);
    Some(Box::new(payload))
}

/// The projected leg across an anchor move. A rigid move (both anchors by
/// the same price) carries it along by construction, its offset being from
/// the entry. A reshape pins it where it stood, so the line the trader
/// grabbed is the only one that moves.
fn reanchored(
    payload: &dyn DrawingPayload,
    before: &[crate::ChartPoint],
    after: &[crate::ChartPoint],
    side: Side,
) -> Option<Box<dyn DrawingPayload>> {
    let mut payload = payload.as_any().downcast_ref::<PositionPayload>()?.clone();
    let ([entry, edge, ..], [moved_entry, moved_edge, ..]) = (before, after) else {
        return None;
    };
    let shift = moved_entry.price - entry.price;
    if shift == moved_edge.price - edge.price {
        return None;
    }
    let held = legs(
        side,
        entry.price,
        edge.price,
        payload.ratio,
        payload.pinned(entry.price),
    )?
    .projected();
    payload.projected_offset = Some(held - moved_entry.price);
    Some(Box::new(payload))
}

/// The plan an object of a position tool states, for a readback.
#[must_use]
pub fn position_plan(
    tool: crate::DrawingTool,
    anchors: &[crate::ChartPoint],
    payload: &dyn DrawingPayload,
) -> Option<Legs> {
    let side = POSITION_TOOLS
        .iter()
        .find(|position| position.id == tool.id())?
        .side;
    let payload = payload.as_any().downcast_ref::<PositionPayload>()?;
    let [entry, edge, ..] = anchors else {
        return None;
    };
    legs(
        side,
        entry.price,
        edge.price,
        payload.ratio,
        payload.pinned(entry.price),
    )
}

/// One click on the plan tag steps the ratio to the next preset.
pub(super) fn click(points: &[Px], position: Px, payload: &mut dyn DrawingPayload) -> bool {
    let ([a, b, ..], Some(payload)) = (
        points,
        payload.as_any_mut().downcast_mut::<PositionPayload>(),
    ) else {
        return false;
    };
    let left = a.x.min(b.x);
    let on_tag = position.x >= left
        && position.x <= left + TAG_HIT_WIDTH_PX
        && (position.y - a.y).abs() <= TAG_HIT_HALF_HEIGHT_PX;
    if !on_tag {
        return false;
    }
    payload.cycle_ratio();
    true
}

/// One side's position tool. Both sides are this one implementation; the
/// registry's `long_position` and `short_position` entries each name one.
pub(super) struct PositionTool {
    pub side: Side,
    pub id: &'static str,
    name: &'static str,
    settings_title: &'static str,
    icon: &'static str,
    icon_strokes: super::IconStrokes,
    hover_text: &'static str,
    placement_hint: &'static str,
}

pub(super) const LONG: PositionTool = PositionTool {
    side: Side::Long,
    id: "long-position",
    name: "Long position",
    settings_title: "Long position settings",
    icon: crate::icons::ARROW_FAT_UP,
    icon_strokes: LONG_ICON,
    hover_text: "Long position - click the entry, then the stop or the target; the other leg follows the ratio",
    placement_hint: "Click the stop (below) or the target (above)",
};

pub(super) const SHORT: PositionTool = PositionTool {
    side: Side::Short,
    id: "short-position",
    name: "Short position",
    settings_title: "Short position settings",
    icon: crate::icons::ARROW_FAT_DOWN,
    icon_strokes: SHORT_ICON,
    hover_text: "Short position - click the entry, then the stop or the target; the other leg follows the ratio",
    placement_hint: "Click the stop (above) or the target (below)",
};

const POSITION_TOOLS: [PositionTool; 2] = [LONG, SHORT];

impl MarkTool for PositionTool {
    fn id(&self) -> &'static str {
        self.id
    }
    fn name(&self) -> &'static str {
        self.name
    }
    fn settings_title(&self) -> &'static str {
        self.settings_title
    }
    fn icon(&self) -> &'static str {
        self.icon
    }
    fn icon_strokes(&self) -> super::IconStrokes {
        self.icon_strokes
    }
    fn hover_text(&self) -> &'static str {
        self.hover_text
    }
    fn placement_hint(&self, placed: usize) -> Option<&'static str> {
        (placed == 1).then_some(self.placement_hint)
    }
    fn required_points(&self) -> usize {
        2
    }
    fn family(&self) -> Option<ToolFamily> {
        Some(POSITION_FAMILY)
    }
    fn supports_fill(&self) -> bool {
        true
    }
    fn price_band_only(&self) -> bool {
        true
    }
    fn default_payload(&self) -> Box<dyn DrawingPayload> {
        Box::new(PositionPayload::default())
    }
    /// The projected zone reaches past the anchors by the pinned offset,
    /// or `ratio` (or `1 / ratio`) times the measured height, on a side the
    /// anchors alone do not say: both sides are reserved, and the width the
    /// plan is painted at, so no popup lands on it.
    fn content_bounds(
        &self,
        payload: &dyn DrawingPayload,
        anchors: PxRect,
        _chart: PxRect,
        spacing: crate::TextSpacing,
    ) -> PxRect {
        let payload = payload
            .as_any()
            .downcast_ref::<PositionPayload>()
            .cloned()
            .unwrap_or_default();
        let reach = match payload.projected_offset {
            Some(offset) => (offset.abs() * spacing.px_per_value) as f32,
            None => anchors.height() * payload.ratio.max(1.0 / payload.ratio) as f32,
        };
        let right = anchors
            .max
            .x
            .max(anchors.min.x + MIN_WIDTH_BARS * spacing.px_per_bar.max(0.0));
        PxRect::from_min_max(
            pos2(anchors.min.x, anchors.min.y - reach),
            pos2(right, anchors.max.y + reach),
        )
    }
    fn placement_refusal(&self, anchors: &[crate::ChartPoint]) -> Option<&'static str> {
        matches!(anchors, [entry, edge, ..] if entry.price == edge.price)
            .then_some("a position plan needs its stop or target at another price than its entry")
    }
    fn paint(
        &self,
        painter: &mut MarkScene,
        chart_rect: PxRect,
        style: DrawingStyle,
        points: &[Px],
        ctxt: &DrawContext<'_>,
    ) {
        paint(painter, chart_rect, style, points, ctxt, self.side);
    }
    fn hit_test(
        &self,
        _chart_rect: PxRect,
        points: &[Px],
        position: Px,
        radius_px: f32,
        ctxt: &DrawContext<'_>,
    ) -> bool {
        hit_test(points, position, radius_px, ctxt, self.side)
    }
    fn drag_handle(
        &self,
        _chart_rect: PxRect,
        points: &[Px],
        handle: usize,
        to: Px,
        _ctxt: &DrawContext<'_>,
        _constrain: Constrain,
    ) -> Option<Handles> {
        drag_line(points, handle, to)
    }
    fn drag_payload_handle(
        &self,
        _chart_rect: PxRect,
        points: &[Px],
        handle: usize,
        to: Px,
        ctxt: &DrawContext<'_>,
    ) -> Option<Box<dyn DrawingPayload>> {
        drag_projected(points, handle, to, ctxt, self.side)
    }
    fn reanchored_payload(
        &self,
        payload: &dyn DrawingPayload,
        before: &[crate::ChartPoint],
        after: &[crate::ChartPoint],
    ) -> Option<Box<dyn DrawingPayload>> {
        reanchored(payload, before, after, self.side)
    }
    fn handle_hint(&self, handle: usize) -> Option<PointerHint> {
        line_hint(handle)
    }
    fn pointer_hint(
        &self,
        _chart_rect: PxRect,
        points: &[Px],
        position: Px,
        ctxt: &DrawContext<'_>,
    ) -> Option<PointerHint> {
        pointer_hint(points, position, ctxt, self.side)
    }
    fn click(
        &self,
        _chart_rect: PxRect,
        points: &[Px],
        position: Px,
        payload: &mut dyn DrawingPayload,
    ) -> bool {
        click(points, position, payload)
    }

    #[cfg(test)]
    fn test_geometry(&self) -> (Vec<Px>, Px) {
        let edge = match self.side {
            Side::Long => 250.0,
            Side::Short => 150.0,
        };
        (
            vec![pos2(100.0, 200.0), pos2(300.0, edge)],
            pos2(200.0, (200.0 + edge) / 2.0),
        )
    }
}

#[cfg(test)]
#[path = "position_core_tests.rs"]
mod tests;
