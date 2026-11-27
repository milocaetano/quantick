//! `annotate.position.set` and `annotate.position.default`: a position's
//! floating-bar edits and its per-tool default, through the same headless
//! edits the bar makes (`quantick_drawings::position_edit`). The window
//! lends the addressed pane's marks and the preset bank
//! ([`DrawingDefaultsPort`]).

use crate::wire::chart::PRICE_DECIMAL_PLACES;
pub use crate::wire::drawing_position::*;
use quantick_control::annotation::ANNOTATE_MODULE_ID;
use quantick_control::error::ControlError;
use quantick_control::registry::RegistryError;
use quantick_control::wire::{ActorContext, CanonicalDecimal, WireU64};
use quantick_control_host::journal::NewEvent;
use quantick_control_host::wire::canonical_f64;
use quantick_drawings::position_edit::{
    PositionDefault, PositionEdit, PositionLeg, apply_position_default, edited_position,
    opening_position, position_of,
};
use quantick_drawings::position_text::ratio_button;
use quantick_drawings::{
    Drawing, DrawingId, DrawingTool, Drawings, PositionPayload, PositionSide, PresetContext,
    PresetHost, parse_rgb_hex, position_plan, rgb_hex,
};
use serde_json::Value;

use crate::JournalAccess;
use crate::addressed_pane::AddressedPanePort;
use crate::dock::ActionDock;
use crate::invalid;
use crate::pane_marks::PaneMarks;

/// The window's bank of saved drawing defaults and named presets.
pub trait DrawingDefaultsPort {
    fn drawing_defaults(&mut self) -> &mut dyn PresetHost;
}

/// Dock both calls.
///
/// # Errors
/// A capability already docked.
pub fn register<D, H, A>(registry: &mut D) -> Result<(), RegistryError>
where
    D: ActionDock<H, A>,
    H: DrawingDefaultsPort + AddressedPanePort<Pane: PaneMarks> + ?Sized,
    A: JournalAccess,
{
    registry.register(set_descriptor(), |host, access, actor, input| {
        let input: PositionSetInput = serde_json::from_value(input.clone()).map_err(invalid)?;
        let pane = host.addressed_pane(input.tab_id, input.pane_id)?;
        let result = set(pane.marks(), &input)?;
        // A width moves an edge at an estimated pace; the pane names its time.
        let id = DrawingId(input.drawing_id.get());
        if let Some(index) = pane.marks().shown_index_of(id).filter(|_| result.changed) {
            pane.restamp_mark(index);
        }
        journal(access, actor, SET_EVENT_KIND, "position", &result)
    })?;
    registry.register(default_descriptor(), |host, access, actor, input| {
        let input: PositionDefaultInput = serde_json::from_value(input.clone()).map_err(invalid)?;
        let marks = host.addressed_pane(input.tab_id, input.pane_id)?.marks();
        let (drawing, pane) = (
            drawing(marks, &input.drawing_id)?.clone(),
            marks.preset_context(()),
        );
        let result = default(
            &mut pane.on(host.drawing_defaults()),
            &drawing,
            input.action,
        )?;
        journal(
            access,
            actor,
            DEFAULT_EVENT_KIND,
            "position_default",
            &result,
        )
    })
}

fn journal<A: JournalAccess, T: serde::Serialize>(
    access: &mut A,
    actor: &ActorContext,
    kind: &'static str,
    key: &str,
    result: &T,
) -> Result<Value, ControlError> {
    let value = serde_json::to_value(result).map_err(invalid)?;
    let payload = serde_json::json!({ key: value.clone() });
    access.record_event(NewEvent::by(ANNOTATE_MODULE_ID, kind, actor, payload));
    Ok(value)
}

fn drawing<'a>(
    marks: &'a Drawings<DrawingTool>,
    id: &WireU64,
) -> Result<&'a Drawing<DrawingTool>, ControlError> {
    let index = marks
        .shown_index_of(DrawingId(id.get()))
        .ok_or_else(|| invalid("unknown drawing in pane"))?;
    Ok(&marks.items()[index])
}

/// Prices at the chart contract's precision.
fn price(value: f64) -> Result<CanonicalDecimal, ControlError> {
    canonical_f64(value, PRICE_DECIMAL_PLACES).ok_or_else(|| invalid("a leg price is not finite"))
}

/// A ratio or a width: six places keep float noise such as
/// 2.4999999999999996 out of the readback.
fn measure(value: f64) -> Result<CanonicalDecimal, ControlError> {
    canonical_f64(value, MEASURE_DECIMAL_PLACES).ok_or_else(|| invalid("a measure is not finite"))
}

const MEASURE_DECIMAL_PLACES: u32 = 6;

fn number(value: &CanonicalDecimal) -> Result<f64, ControlError> {
    value.as_str().parse::<f64>().map_err(invalid)
}

fn colour(text: &str) -> Result<[u8; 3], ControlError> {
    parse_rgb_hex(text).ok_or_else(|| invalid("a colour is #RRGGBB"))
}

/// The edits `input` asks for, in a fixed order, or why one is malformed.
fn edits(input: &PositionSetInput) -> Result<Vec<PositionEdit>, ControlError> {
    let mut edits = Vec::new();
    if let Some(text) = &input.target_color {
        edits.push(PositionEdit::Colour(PositionLeg::Target, colour(text)?));
    }
    if let Some(text) = &input.stop_color {
        edits.push(PositionEdit::Colour(PositionLeg::Stop, colour(text)?));
    }
    if let Some(pct) = input.zone_opacity_pct {
        edits.push(PositionEdit::ZoneOpacity(pct));
    }
    if let Some(ratio) = &input.ratio {
        edits.push(PositionEdit::Ratio(number(ratio)?));
    }
    if let Some(bars) = &input.width_bars {
        edits.push(PositionEdit::WidthBars(number(bars)? as f32));
    }
    Ok(edits)
}

/// Apply `input` to the named position as one undo step; every edit lands
/// or none does.
///
/// # Errors
/// Malformed input, an unknown drawing, another tool's drawing, a geometry
/// edit on a locked position, an open drawing gesture, or a value out of
/// range.
pub fn set(
    marks: &mut Drawings<DrawingTool>,
    input: &PositionSetInput,
) -> Result<PositionSetResult, ControlError> {
    let edits = edits(input)?;
    let before = drawing(marks, &input.drawing_id)?.clone();
    position_of(&before).map_err(|refusal| invalid(refusal.reason()))?;
    if !edits.is_empty() && marks.draft().is_some() {
        return Err(invalid("finish the active drawing gesture first"));
    }
    let mut after = before.clone();
    for edit in edits {
        after = edited_position(&after, edit).map_err(|refusal| invalid(refusal.reason()))?;
    }
    let changed = after != before;
    // Read back before writing: an edit whose result cannot be read back
    // (a flat plan) is refused with nothing changed.
    let result = read_back(input, &after, changed)?;
    if changed {
        let index = marks
            .shown_index_of(before.id)
            .ok_or_else(|| invalid("unknown drawing in pane"))?;
        marks.items_mut()[index] = after.clone();
        marks.record_edit_of(index, before);
    }
    Ok(result)
}

fn look(payload: &PositionPayload, ratio: f64) -> Result<PositionLook, ControlError> {
    let custom = quantick_drawings::position_preset_of(ratio).is_none();
    Ok(PositionLook {
        target_color: rgb_hex(payload.target_rgb),
        stop_color: rgb_hex(payload.stop_rgb),
        zone_opacity_pct: payload.zone_opacity_pct,
        ratio: measure(ratio)?,
        custom,
        ratio_text: ratio_button(ratio),
    })
}

fn read_back(
    input: &PositionSetInput,
    drawing: &Drawing<DrawingTool>,
    changed: bool,
) -> Result<PositionSetResult, ControlError> {
    let (side, payload) = position_of(drawing).map_err(|refusal| invalid(refusal.reason()))?;
    let plan = position_plan(drawing.tool, &drawing.points, drawing.payload.as_ref())
        .ok_or_else(|| invalid("the position's entry and edge are at one price"))?;
    Ok(PositionSetResult {
        tab_id: input.tab_id,
        pane_id: input.pane_id,
        drawing_id: input.drawing_id,
        side: match side {
            PositionSide::Long => PositionSideDto::Long,
            PositionSide::Short => PositionSideDto::Short,
        },
        look: look(payload, plan.ratio())?,
        entry: price(plan.entry)?,
        stop: price(plan.stop)?,
        target: price(plan.target)?,
        width_bars: measure(f64::from(quantick_drawings::position_width_bars(
            &drawing.points,
        )))?,
        changed,
    })
}

/// Save, reset or read the default of `drawing`'s tool on the pane `presets`
/// speaks for.
///
/// # Errors
/// Another tool's drawing.
pub fn default(
    presets: &mut PresetContext<&mut dyn PresetHost>,
    drawing: &Drawing<DrawingTool>,
    action: DefaultAction,
) -> Result<PositionDefaultResult, ControlError> {
    let refused =
        |refusal: quantick_drawings::position_edit::PositionEditRefusal| invalid(refusal.reason());
    position_of(drawing).map_err(refused)?;
    let action = match action {
        DefaultAction::Save => Some(PositionDefault::Save),
        DefaultAction::Reset => Some(PositionDefault::Reset),
        DefaultAction::Inspect => None,
    };
    if let Some(action) = action {
        apply_position_default(presets, drawing, action).map_err(refused)?;
    }
    let opening = opening_position(&presets.on(presets.host()), drawing.tool)
        .ok_or_else(|| invalid("the tool opens no position"))?;
    Ok(PositionDefaultResult {
        tool_id: drawing.tool.id().to_owned(),
        opening: look(&opening, opening.ratio)?,
        saved: presets.has_saved_default(drawing.tool),
    })
}

#[cfg(test)]
#[path = "drawing_position_tests.rs"]
mod tests;
