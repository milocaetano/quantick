//! The edits a position's floating bar and `annotate.position.set` make,
//! decided once here: a zone colour, the zone opacity, the ratio and the
//! width, each applied to a copy of the drawing the host stores as one
//! undo step. The bar's risk/reward menu is read from here too.

pub use quantick_appearance::{Ground, PaneCanvas};

use crate::position_text::{RatioButton, custom_row, preset_rows, ratio_button};
use crate::{
    Drawing, DrawingTool, PositionPayload, PositionRefusal, PositionSide, PresetHost, Rgb,
    new_drawing_from_defaults, position_plan, position_preset_of, position_side,
    reset_tool_default, save_tool_default, set_plan_ratio,
};

/// Which zone a colour edit paints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PositionLeg {
    Target,
    Stop,
}

/// One edit; a refused one changes nothing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PositionEdit {
    Colour(PositionLeg, Rgb),
    ZoneOpacity(u8),
    /// Reward per unit of risk; the target moves, nothing else does.
    Ratio(f64),
    WidthBars(f32),
}

impl PositionEdit {
    /// Whether the edit moves a price or a column — what a lock protects.
    /// Colours and opacity stay open on a locked drawing, as on every tool.
    #[must_use]
    pub fn moves_geometry(self) -> bool {
        matches!(self, Self::Ratio(_) | Self::WidthBars(_))
    }
}

/// Why a position edit was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PositionEditRefusal {
    NotAPosition,
    Locked,
    Invalid(PositionRefusal),
}

impl PositionEditRefusal {
    #[must_use]
    pub fn reason(self) -> &'static str {
        match self {
            Self::NotAPosition => "the drawing is not a long or short position",
            Self::Locked => "a locked position keeps its prices and width; unlock it first",
            Self::Invalid(refusal) => refusal.reason(),
        }
    }
}

/// The position's side and payload, or why `drawing` has none.
///
/// # Errors
/// A drawing of any other tool.
pub fn position_of(
    drawing: &Drawing<DrawingTool>,
) -> Result<(PositionSide, &PositionPayload), PositionEditRefusal> {
    let side = position_side(drawing.tool).ok_or(PositionEditRefusal::NotAPosition)?;
    let payload = drawing.payload.as_any().downcast_ref::<PositionPayload>();
    Ok((side, payload.ok_or(PositionEditRefusal::NotAPosition)?))
}

/// `drawing` with `edit` applied, or why not. The copy equals the input
/// when the edit names what already stands.
///
/// # Errors
/// Another tool's drawing, a geometry edit on a locked one, or a value out
/// of range.
pub fn edited_position(
    drawing: &Drawing<DrawingTool>,
    edit: PositionEdit,
) -> Result<Drawing<DrawingTool>, PositionEditRefusal> {
    let (side, payload) = position_of(drawing)?;
    if drawing.locked && edit.moves_geometry() {
        return Err(PositionEditRefusal::Locked);
    }
    let mut payload = payload.clone();
    let mut points = drawing.points.clone();
    match edit {
        PositionEdit::Colour(PositionLeg::Target, rgb) => {
            payload.target_rgb = rgb;
            Ok(())
        }
        PositionEdit::Colour(PositionLeg::Stop, rgb) => {
            payload.stop_rgb = rgb;
            Ok(())
        }
        PositionEdit::ZoneOpacity(pct) => payload.set_zone_opacity(pct),
        PositionEdit::Ratio(ratio) => set_plan_ratio(side, &mut points, &mut payload, ratio),
        PositionEdit::WidthBars(bars) => payload.set_width_bars(&points, bars),
    }
    .map_err(PositionEditRefusal::Invalid)?;
    let mut edited = drawing.clone();
    // A preset that matches the plan already standing moves nothing.
    if points != drawing.points {
        edited.points = points;
    }
    edited.payload = Box::new(payload);
    Ok(edited)
}

/// The ratio a position's prices make, `None` for a flat one.
#[must_use]
pub fn plan_ratio(drawing: &Drawing<DrawingTool>) -> Option<f64> {
    position_plan(drawing.tool, &drawing.points, drawing.payload.as_ref()).map(|plan| plan.ratio())
}

/// One row of the risk/reward menu.
#[derive(Debug, Clone, PartialEq)]
pub struct RatioRow {
    pub label: String,
    /// `None` for the read-only custom row.
    pub pick: Option<f64>,
    pub checked: bool,
}

/// What the bar shows for a position's ratio: the button and the menu.
#[derive(Debug, Clone, PartialEq)]
pub struct RatioMenu {
    pub button: RatioButton,
    /// The checked, read-only `Custom 1:N` row on top while the plan is no
    /// preset, then the presets, none checked in that case.
    pub rows: Vec<RatioRow>,
}

/// The risk/reward button and menu for `drawing`, `None` for another tool
/// or a flat plan.
#[must_use]
pub fn ratio_menu(drawing: &Drawing<DrawingTool>) -> Option<RatioMenu> {
    let ratio = plan_ratio(drawing)?;
    let preset = position_preset_of(ratio);
    let mut rows = Vec::new();
    if preset.is_none() {
        rows.push(RatioRow {
            label: custom_row(ratio),
            pick: None,
            checked: true,
        });
    }
    rows.extend(preset_rows().into_iter().map(|(value, label)| RatioRow {
        label,
        pick: Some(value),
        checked: preset == Some(value),
    }));
    Some(RatioMenu {
        button: ratio_button(ratio, preset.is_none()),
        rows,
    })
}

/// The per-tool default a position's menus end with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PositionDefault {
    /// New positions of this side open with this one's colours, opacity
    /// and ratio — the ratio its prices make, custom or not.
    Save,
    /// New positions of this side open with the factory look again.
    Reset,
}

/// What a position's bar asked for this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PositionAsk {
    Edit(PositionEdit),
    Default(PositionDefault),
}

/// Apply `action` to `drawing`'s tool on `canvas`'s ground through the
/// existing per-tool default. Drawings already placed never change.
///
/// # Errors
/// A drawing of any other tool.
pub fn apply_position_default(
    host: &mut dyn PresetHost,
    canvas: PaneCanvas,
    drawing: &Drawing<DrawingTool>,
    action: PositionDefault,
) -> Result<(), PositionEditRefusal> {
    position_of(drawing)?;
    match action {
        PositionDefault::Save => save_tool_default(host, canvas, drawing),
        PositionDefault::Reset => reset_tool_default(host, canvas, drawing.tool),
    }
    Ok(())
}

/// The payload a new position of `tool` opens with on `canvas`.
#[must_use]
pub fn opening_position(
    host: &dyn PresetHost,
    canvas: PaneCanvas,
    tool: DrawingTool,
) -> Option<PositionPayload> {
    let opening = new_drawing_from_defaults(host, canvas, tool);
    let payload = opening.payload.as_any().downcast_ref::<PositionPayload>();
    payload.cloned()
}

/// Everything a selected position's bar shows.
#[derive(Debug, Clone, PartialEq)]
pub struct PositionFace {
    pub target_rgb: Rgb,
    pub stop_rgb: Rgb,
    pub zone_opacity_pct: u8,
    pub ratio: RatioMenu,
}

/// The bar's face for `drawing`, `None` for another tool or a flat plan.
#[must_use]
pub fn position_face(drawing: &Drawing<DrawingTool>) -> Option<PositionFace> {
    let (_, payload) = position_of(drawing).ok()?;
    Some(PositionFace {
        target_rgb: payload.target_rgb,
        stop_rgb: payload.stop_rgb,
        zone_opacity_pct: payload.zone_opacity_pct,
        ratio: ratio_menu(drawing)?,
    })
}

#[cfg(test)]
#[path = "position_edit_tests.rs"]
mod tests;
