//! The position drawing the long and short tools share: an entry, a stop
//! zone and a target zone, planned on the chart and never sent.
//!
//! Two anchors hold the trader's measurement — the entry where the drag
//! started and the edge where it ended. Which leg that edge is follows from
//! the side alone: on the trade's profit side it is the target, on its loss
//! side the stop. The other leg is projected by the payload's ratio
//! (reward per unit of risk) until the trader drags it, which pins it.
//!
//! Unselected, a position is its two zones and their thin edges. Selected,
//! it adds the entry line, the Target plate beyond the target edge, the Stop
//! plate beyond the stop edge and the plan plate on the entry line, all three
//! centred on the box and solid in their leg's colour, and the handles: a
//! circle at the entry line's left end, squares at the target and stop
//! corners and at the entry line's right end. Every zone and plate colour is
//! the drawing's own ([`PositionPayload`]), never the theme's buy and sell
//! ink.
//!
//! No tick count and no tick snap: a drawing does not know the instrument's
//! tick size (`measure_core.rs` states the same honesty rule), so legs read
//! in points and the projected price is the exact ratio.

use quantick_appearance::{FontRole, ResolvedTheme, Token};
use quantick_chart::style::PremultipliedRgba;

use crate::{Align2, FontSpec, MarkScene, Px, PxRect, Stroke, pos2, vec2};

use super::position_payload::{PositionPayload, Rgb, light_ground};
use super::position_text::plan_plate;
use super::{
    Constrain, DrawContext, DrawingPayload, DrawingStyle, Handles, MarkTool, PointerHint,
    ToolFamily,
};

/// The ratios a saved or imported payload may hold. Outside them a plan is
/// unreadable on screen — one leg collapses onto the entry line.
pub const MIN_RATIO: f64 = 0.1;
pub const MAX_RATIO: f64 = 20.0;
/// The narrowest a position is drawn, in bars: a drag on one bar still
/// leaves zones wide enough to read.
pub const MIN_WIDTH_BARS: f32 = 14.0;

/// The grab points, in [`handles`] order. The circle at the entry line's left
/// end moves the entry: its price and the left edge in time. The square at
/// its right end resizes the width and nothing else. The squares at the
/// projected and measured legs' left corners move that leg's price, as a
/// press on its line does. The anchors have no dot of their own: the entry
/// anchor sits on one end of the entry line, so a dot there would take the
/// press.
pub(super) const ENTRY_HANDLE: usize = 0;
pub(super) const RIGHT_EDGE_HANDLE: usize = 1;
pub(super) const PROJECTED_HANDLE: usize = 2;
pub(super) const MEASURED_LINE_HANDLE: usize = 3;
/// The entry line itself, grabbed anywhere along it; no point of its own.
pub(super) const ENTRY_LINE_HANDLE: usize = 4;
/// The least contrast white text keeps on a plate before the theme's ink
/// replaces it: white on the factory green and black plates.
const WHITE_TEXT_CONTRAST: f32 = 3.0;

const GRAB_PX: f32 = 5.0;
/// The zones' edges and the entry line: thin, so the bars stay the subject.
const EDGE_PX: f32 = 1.0;
const LABEL_TEXT_PX: f32 = 10.0;
const LABEL_PAD_X_PX: f32 = 5.0;
const LABEL_PAD_Y_PX: f32 = 2.0;
const LABEL_RADIUS_PX: f32 = 3.0;
const LABEL_INSET_PX: f32 = 4.0;
/// Relative difference under which two anchors' price shifts are one move.
const RIGID_TOLERANCE: f64 = 1e-9;

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
    /// Reward per unit of risk, as the plan stands — the chosen ratio, or
    /// the one a dragged leg left behind.
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

fn payload_of(ctxt: &DrawContext<'_>) -> PositionPayload {
    ctxt.payload
        .as_any()
        .downcast_ref::<PositionPayload>()
        .cloned()
        .unwrap_or_default()
}

/// The plan `payload` and `anchors` make on `side`.
fn plan_of(side: Side, anchors: &[crate::ChartPoint], payload: &PositionPayload) -> Option<Legs> {
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

/// The columns a position spans: its anchors', never narrower than
/// [`MIN_WIDTH_BARS`].
fn columns(a: Px, b: Px, px_per_bar: f32) -> (f32, f32) {
    let left = a.x.min(b.x);
    (
        left,
        a.x.max(b.x)
            .max(left + MIN_WIDTH_BARS * px_per_bar.max(0.0)),
    )
}

fn frame(points: &[Px], ctxt: &DrawContext<'_>, side: Side) -> Option<Frame> {
    let [a, b, ..] = points else {
        return None;
    };
    let payload = payload_of(ctxt);
    let legs = plan_of(side, ctxt.anchors, &payload);
    let (left, right) = columns(*a, *b, ctxt.px_per_bar);
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

fn ink([r, g, b]: Rgb, alpha: u8) -> PremultipliedRgba {
    crate::rgba_unmultiplied(r, g, b, alpha)
}

fn points_text(delta: f64) -> String {
    format!(
        "{} pts",
        quantick_engine::display::display_signed_f64(delta)
    )
}

/// White on a plate it reads on, else the theme's better ink for it.
fn plate_ink(appearance: &ResolvedTheme, plate: PremultipliedRgba) -> PremultipliedRgba {
    let white = crate::rgba_unmultiplied(u8::MAX, u8::MAX, u8::MAX, u8::MAX);
    if quantick_appearance::contrast::contrast_ratio(white, plate) >= WHITE_TEXT_CONTRAST {
        white
    } else {
        appearance.ink_on(plate)
    }
}

/// The entry line: black on a light ground, the theme's text ink on a dark
/// one, where black would vanish.
fn entry_ink(appearance: &ResolvedTheme) -> PremultipliedRgba {
    if light_ground(appearance) {
        crate::rgba_unmultiplied(0, 0, 0, u8::MAX)
    } else {
        appearance.color(Token::TextPrimary)
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
        plate_ink(appearance, plate),
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
    let payload = payload_of(ctxt);
    let (left, right) = (frame.left, frame.right);
    let zone = |y: f32| PxRect::from_two_pos(pos2(left, frame.entry_y), pos2(right, y));
    let Some(legs) = frame.legs else {
        painter.line_segment(
            [pos2(left, frame.entry_y), pos2(right, frame.entry_y)],
            crate::drawing_stroke(style),
        );
        return;
    };
    let zones = [
        (frame.stop_y, payload.stop_rgb),
        (frame.target_y, payload.target_rgb),
    ];
    if !ctxt.halo {
        for (y, rgb) in zones {
            painter.rect_filled(zone(y), 0.0, ink(rgb, payload.zone_alpha()));
        }
    }
    for (y, rgb) in zones {
        // The halo pass traces the edges in the selection's own ink.
        let edge = if ctxt.halo {
            Stroke::new(style.width_px, style.color)
        } else {
            Stroke::new(EDGE_PX, ink(rgb, u8::MAX))
        };
        painter.line_segment([pos2(left, y), pos2(right, y)], edge);
    }
    if ctxt.halo || !ctxt.primary_band || !ctxt.selected {
        return;
    }
    painter.line_segment(
        [pos2(left, frame.entry_y), pos2(right, frame.entry_y)],
        Stroke::new(EDGE_PX, entry_ink(ctxt.appearance)),
    );
    paint_leg_plates(painter, chart_rect, &frame, &legs, ctxt, &payload);
    // Centred on the entry line, in the target's colour; the handles paint
    // over it at the line's ends.
    let tag = plan_plate(side, legs.ratio(), legs.entry);
    let at = pos2((left + right) / 2.0, frame.entry_y);
    let plate = ink(payload.target_rgb, u8::MAX);
    label(painter, ctxt, at, Align2::CENTER_CENTER, &tag, plate);
}

/// The Target plate beyond the target edge and the Stop plate beyond the
/// stop edge, outside the box, held on the chart when that edge is off it
/// and absent when the whole zone is: no plate for a level not in view.
fn paint_leg_plates(
    painter: &mut MarkScene,
    chart_rect: PxRect,
    frame: &Frame,
    legs: &Legs,
    ctxt: &DrawContext<'_>,
    payload: &PositionPayload,
) {
    let (top, bottom) = (chart_rect.top(), chart_rect.bottom());
    let shown = |y: f32| y.max(frame.entry_y) >= top && y.min(frame.entry_y) <= bottom;
    let center = (frame.left + frame.right) / 2.0;
    let beyond = |y: f32| {
        if y < frame.entry_y {
            (
                pos2(center, (y - LABEL_INSET_PX).max(top)),
                Align2::CENTER_BOTTOM,
            )
        } else {
            (
                pos2(center, (y + LABEL_INSET_PX).min(bottom)),
                Align2::CENTER_TOP,
            )
        }
    };
    let price = quantick_engine::display::display_f64;
    let plates = [
        (
            frame.target_y,
            format!(
                "Target {} · {}",
                price(legs.target),
                points_text(legs.reward())
            ),
            payload.target_rgb,
        ),
        (
            frame.stop_y,
            format!("Stop {} · {}", price(legs.stop), points_text(-legs.risk())),
            payload.stop_rgb,
        ),
    ];
    for (y, text, rgb) in plates {
        if shown(y) {
            let (at, anchor) = beyond(y);
            // Pushed back onto the chart, the plate hangs inside it instead.
            let anchor = if at.y <= top {
                Align2::CENTER_TOP
            } else if at.y >= bottom {
                Align2::CENTER_BOTTOM
            } else {
                anchor
            };
            label(painter, ctxt, at, anchor, &text, ink(rgb, u8::MAX));
        }
    }
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

/// The entry line's two ends, then the projected and measured legs' left
/// corners; a flat measurement, no plan, has only the entry line's ends.
pub(super) fn handles(points: &[Px], ctxt: &DrawContext<'_>, side: Side) -> Option<Handles> {
    let frame = frame(points, ctxt, side)?;
    let mut handles = Handles::from_slice(&[
        pos2(frame.left, frame.entry_y),
        pos2(frame.right, frame.entry_y),
    ]);
    if let (Some(projected), Some(measured)) = (frame.projected_y(), frame.measured_y()) {
        handles.push(pos2(frame.left, projected));
        handles.push(pos2(frame.left, measured));
    }
    Some(handles)
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

/// A press on a line or a handle announces what it grabs.
pub(super) fn line_hint(handle: usize) -> Option<PointerHint> {
    (ENTRY_HANDLE..=ENTRY_LINE_HANDLE)
        .contains(&handle)
        .then_some(PointerHint::ResizeSide { handle })
}

/// A line grab moves that line's price and nothing else: the entry and the
/// measured edge keep their bars, so a press mid-line never jumps an anchor
/// sideways to the pointer.
///
/// The width handle moves the right-hand anchor in time and nothing else;
/// the entry circle moves the entry's price and the left-hand anchor in
/// time. The width lives in the anchors, so it keeps its length in market
/// time on any timeframe and on a pane mirroring the drawing. The other edge
/// holds, never closer than [`MIN_WIDTH_BARS`].
fn drag_line(points: &[Px], handle: usize, to: Px, px_per_bar: f32) -> Option<Handles> {
    let [a, b, ..] = points else {
        return None;
    };
    let mut moved: Handles = points.iter().copied().collect();
    let (left, right) = columns(*a, *b, px_per_bar);
    let floor = MIN_WIDTH_BARS * px_per_bar.max(0.0);
    let (left_anchor, right_anchor) = if a.x <= b.x { (0, 1) } else { (1, 0) };
    match handle {
        ENTRY_LINE_HANDLE => moved.get_mut(0)?.y = to.y,
        MEASURED_LINE_HANDLE => moved.get_mut(1)?.y = to.y,
        ENTRY_HANDLE => {
            moved.get_mut(0)?.y = to.y;
            moved.get_mut(left_anchor)?.x = to.x.min(right - floor);
        }
        RIGHT_EDGE_HANDLE => moved.get_mut(right_anchor)?.x = to.x.max(left + floor),
        _ => return None,
    }
    Some(moved)
}

/// Dragging the projected leg pins it at the price under the pointer, on
/// its own side of the entry, and the ratio becomes the one the three
/// prices make.
pub(super) fn drag_payload(
    points: &[Px],
    handle: usize,
    to: Px,
    ctxt: &DrawContext<'_>,
    side: Side,
) -> Option<Box<dyn DrawingPayload>> {
    match handle {
        PROJECTED_HANDLE => drag_projected(points, to, ctxt, side),
        _ => None,
    }
}

fn drag_projected(
    points: &[Px],
    to: Px,
    ctxt: &DrawContext<'_>,
    side: Side,
) -> Option<Box<dyn DrawingPayload>> {
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
    let dragged = plan(Some(price))?;
    let projected = if dragged.projected() == price {
        price
    } else {
        held
    };
    payload.projected_offset = Some(projected - entry.price);
    if let Some(plan) = plan(Some(projected)) {
        payload.follow(&plan);
    }
    Some(Box::new(payload))
}

/// The projected leg across an anchor move. A rigid move (both anchors by
/// the same price) carries it along by construction, its offset being from
/// the entry. A reshape pins it where it stood, so the line the trader
/// grabbed is the only one that moves, and the ratio follows.
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
    // Rounding makes `(price + d) - price` differ between two prices moved
    // by the same `d`, so a rigid move is equal shifts within that noise.
    let (entry_shift, edge_shift) = (
        moved_entry.price - entry.price,
        moved_edge.price - edge.price,
    );
    let scale = entry.price.abs().max(edge.price.abs()).max(1.0);
    if (entry_shift - edge_shift).abs() <= RIGID_TOLERANCE * scale {
        return None;
    }
    let held = plan_of(side, before, &payload)?.projected();
    payload.projected_offset = Some(held - moved_entry.price);
    if let Some(plan) = plan_of(side, after, &payload) {
        payload.follow(&plan);
    }
    Some(Box::new(payload))
}

/// The side a position tool's objects take; `None` for any other tool.
#[must_use]
pub fn position_side(tool: crate::DrawingTool) -> Option<Side> {
    POSITION_TOOLS
        .iter()
        .find(|position| position.id == tool.id())
        .map(|position| position.side)
}

/// The plan an object of a position tool states, for a readback.
#[must_use]
pub fn position_plan(
    tool: crate::DrawingTool,
    anchors: &[crate::ChartPoint],
    payload: &dyn DrawingPayload,
) -> Option<Legs> {
    let payload = payload.as_any().downcast_ref::<PositionPayload>()?;
    plan_of(position_side(tool)?, anchors, payload)
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
    hover_text: "Long position - click the entry, then the stop or the target; the other leg follows the risk/reward",
    placement_hint: "Click the stop (below) or the target (above)",
};

pub(super) const SHORT: PositionTool = PositionTool {
    side: Side::Short,
    id: "short-position",
    name: "Short position",
    settings_title: "Short position settings",
    icon: crate::icons::ARROW_FAT_DOWN,
    icon_strokes: SHORT_ICON,
    hover_text: "Short position - click the entry, then the stop or the target; the other leg follows the risk/reward",
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
    fn price_band_only(&self) -> bool {
        true
    }
    fn square_handle(&self, handle: usize) -> bool {
        handle != ENTRY_HANDLE
    }
    fn default_payload(&self) -> Box<dyn DrawingPayload> {
        Box::new(PositionPayload::default())
    }
    /// The projected zone reaches past the anchors by the pinned offset,
    /// or `ratio` (or `1 / ratio`) times the measured height, on a side the
    /// anchors alone do not say: both sides are reserved, and the columns
    /// the plan is painted across, so no popup lands on it.
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
        let px_per_bar = spacing.px_per_bar.max(0.0);
        let (left, right) = (
            anchors.min.x,
            anchors
                .max
                .x
                .max(anchors.min.x + MIN_WIDTH_BARS * px_per_bar),
        );
        // The Target and Stop plates hang outside the zones' far edges; a
        // flat measurement is no plan and shows none.
        let plate = if anchors.height() > 0.0 || payload.projected_offset.is_some() {
            LABEL_TEXT_PX + 2.0 * LABEL_PAD_Y_PX + LABEL_INSET_PX
        } else {
            0.0
        };
        PxRect::from_min_max(
            pos2(left, anchors.min.y - reach - plate),
            pos2(right, anchors.max.y + reach + plate),
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
    fn handles(
        &self,
        _chart_rect: PxRect,
        points: &[Px],
        ctxt: &DrawContext<'_>,
    ) -> Option<Handles> {
        handles(points, ctxt, self.side)
    }
    fn drag_handle(
        &self,
        _chart_rect: PxRect,
        points: &[Px],
        handle: usize,
        to: Px,
        ctxt: &DrawContext<'_>,
        _constrain: Constrain,
    ) -> Option<Handles> {
        drag_line(points, handle, to, ctxt.px_per_bar)
    }
    fn drag_payload_handle(
        &self,
        _chart_rect: PxRect,
        points: &[Px],
        handle: usize,
        to: Px,
        ctxt: &DrawContext<'_>,
    ) -> Option<Box<dyn DrawingPayload>> {
        drag_payload(points, handle, to, ctxt, self.side)
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
