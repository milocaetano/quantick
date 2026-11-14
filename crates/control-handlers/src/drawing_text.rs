//! Validated note editing and style-only repetition through the drawing store.
use crate::{
    annotate::{AnnotateAccess, AnnotatePort, AnnotatedPane},
    dock::ActionDock,
};
use quantick_control::{
    annotation::{ANNOTATE_MODULE_ID, parse_price},
    error::ControlError,
    registry::RegistryError,
    wire::ActorContext,
};
use quantick_control_host::{journal::NewEvent, wire::actor_kind_name};
pub use quantick_control_schema::drawing_text::*;
use quantick_drawings::{
    ChartPoint, DrawingAuthor, DrawingEffect, DrawingId, DrawingIntent, DrawingTool, FontFamily,
    Placement, TextPayload, text::TextAppearance, update,
};
use serde_json::Value;

pub fn register<D, H, A>(registry: &mut D) -> Result<(), RegistryError>
where
    D: ActionDock<H, A>,
    H: AnnotatePort + ?Sized,
    A: AnnotateAccess,
{
    registry.register(set_descriptor(), |host, access, actor, value| {
        let input: SetInput = serde_json::from_value(value.clone()).map_err(invalid)?;
        let result = set(pane(host, &input.target)?, &input)?;
        answer(access, actor, CHANGED, result)
    })?;
    registry.register(repeat_descriptor(), |host, access, actor, value| {
        let input: RepeatInput = serde_json::from_value(value.clone()).map_err(invalid)?;
        let (kind, client) = access
            .recorded_actor()
            .unwrap_or((actor.actor_kind, actor.client_name.as_str()));
        let author = (kind != quantick_control::wire::ActorKind::HumanUi).then(|| DrawingAuthor {
            actor_kind: actor_kind_name(kind).into(),
            client_name: client.into(),
        });
        let result = repeat(pane(host, &input.source)?, &input, author)?;
        answer(access, actor, REPEATED, result)
    })
}

fn invalid(message: impl std::fmt::Display) -> ControlError {
    ControlError::invalid_request(message.to_string())
}

fn pane<'a, H: AnnotatePort + ?Sized>(
    host: &'a mut H,
    target: &TextTarget,
) -> Result<&'a mut dyn AnnotatedPane, ControlError> {
    let index = (0..host.tab_count())
        .find(|index| host.tab_id_at(*index) == target.tab_id.get())
        .ok_or_else(|| invalid("unknown tab"))?;
    let side = host
        .sides(index)
        .into_iter()
        .find(|side| host.pane_mut(index, *side).pane_id() == target.pane_id.get())
        .ok_or_else(|| invalid("unknown pane on tab"))?;
    Ok(host.pane_mut(index, side))
}

fn answer<A: AnnotateAccess>(
    access: &mut A,
    actor: &ActorContext,
    event: &str,
    result: TextResult,
) -> Result<Value, ControlError> {
    let value = serde_json::to_value(result).map_err(invalid)?;
    access.record_event(NewEvent::by(
        ANNOTATE_MODULE_ID,
        event,
        actor,
        value.clone(),
    ));
    Ok(value)
}

fn index(pane: &dyn AnnotatedPane, target: &TextTarget) -> Result<usize, ControlError> {
    let index = pane
        .drawings()
        .index_of(DrawingId(target.drawing_id.get()))
        .ok_or_else(|| invalid("unknown drawing in pane"))?;
    if !pane.drawings().items()[index].tool.holds_text() {
        return Err(invalid("drawing is not a text note"));
    }
    Ok(index)
}

fn anchor(pane: &dyn AnnotatedPane, input: &TextAnchor) -> Result<ChartPoint, ControlError> {
    let slot = pane
        .slot_at_time(input.time_unix_ms)
        .ok_or_else(|| invalid("anchor time is outside loaded chart data"))?;
    Ok(ChartPoint::at_time(
        slot as f32 + 0.5,
        parse_price(&input.price)?,
        pane.slot_open_time(slot),
    ))
}

fn validate_text(text: Option<&str>) -> Result<(), ControlError> {
    if text.is_some_and(|text| text.len() > MAX_TEXT_BYTES) {
        return Err(invalid("text exceeds 16384 UTF-8 bytes"));
    }
    Ok(())
}

fn pixels(
    value: &quantick_control::wire::CanonicalDecimal,
    min: f32,
    max: f32,
) -> Result<f32, ControlError> {
    let value = value.as_str().parse::<f64>().map_err(invalid)?;
    if !value.is_finite() || !(f64::from(min)..=f64::from(max)).contains(&value) {
        return Err(invalid(format!(
            "pixel value must be between {min} and {max}"
        )));
    }
    Ok(value as f32)
}

pub fn set(pane: &mut dyn AnnotatedPane, input: &SetInput) -> Result<TextResult, ControlError> {
    validate_text(input.text.as_deref())?;
    let index = index(pane, &input.target)?;
    let before = pane.drawings().items()[index].clone();
    let editing = input.properties.is_some() || input.text.is_some() || input.anchor.is_some();
    if editing && before.locked {
        return Err(invalid("locked text cannot be edited"));
    }
    if editing && pane.drawings().draft().is_some() {
        return Err(invalid("finish the active drawing gesture first"));
    }
    let mut after = before.clone();
    if let Some(at) = &input.anchor {
        after.points = vec![anchor(pane, at)?];
    }
    let payload = after
        .payload
        .as_any_mut()
        .downcast_mut::<TextPayload>()
        .ok_or_else(|| invalid("invalid text payload"))?;
    if let Some(properties) = &input.properties {
        let size_px = pixels(
            &properties.size_px,
            quantick_drawings::MIN_TEXT_PX,
            quantick_drawings::MAX_TEXT_PX,
        )?;
        let appearance = TextAppearance {
            bold: properties.bold,
            italic: properties.italic,
            background: properties.background,
            border: properties.border,
            wrap_width_px: properties
                .wrap_width_px
                .as_ref()
                .map(|width| {
                    pixels(
                        width,
                        quantick_drawings::text::MIN_WRAP_PX,
                        quantick_drawings::text::MAX_WRAP_PX,
                    )
                })
                .transpose()?,
        };
        payload.size_px = size_px;
        payload.family = match properties.family {
            Family::Proportional => FontFamily::Proportional,
            Family::Monospace => FontFamily::Monospace,
        };
        payload.appearance = appearance;
        let [r, g, b, a] = properties.color;
        after.style.color = quantick_drawings::rgba_unmultiplied(r, g, b, a);
    }
    if let Some(text) = &input.text {
        payload.text.clone_from(text);
    }
    let changed = after != before;
    if changed {
        pane.drawings_mut().items_mut()[index] = after;
        pane.drawings_mut().record_edit_of(index, before);
    }
    result(pane, &input.target, changed)
}

pub fn repeat(
    pane: &mut dyn AnnotatedPane,
    input: &RepeatInput,
    author: Option<DrawingAuthor>,
) -> Result<TextResult, ControlError> {
    validate_text(input.text.as_deref())?;
    let source = index(pane, &input.source)?;
    let point = anchor(pane, &input.anchor)?;
    let drawing = &pane.drawings().items()[source];
    let band = drawing.band.clone();
    if let Some(reason) = pane.band_refusal(&band) {
        return Err(invalid(reason));
    }
    let placement = Placement {
        tool: DrawingTool::by_id("text").expect("registered text"),
        points: vec![point],
        band,
        opening: quantick_drawings::repeat_opening(drawing),
        author,
        name: None,
        text: input.text.clone(),
    };
    let effects = update(pane.drawings_mut(), DrawingIntent::Place(placement));
    let id = effects
        .iter()
        .find_map(|effect| match effect {
            DrawingEffect::Placed { id, .. } => Some(*id),
            _ => None,
        })
        .ok_or_else(|| invalid("finish the active drawing gesture before repeating text"))?;
    let mut target = input.source.clone();
    target.drawing_id = quantick_control::wire::WireU64::new(id.0);
    result(pane, &target, true)
}

fn result(
    pane: &dyn AnnotatedPane,
    target: &TextTarget,
    changed: bool,
) -> Result<TextResult, ControlError> {
    let drawing = &pane.drawings().items()[index(pane, target)?];
    let payload = drawing
        .payload
        .as_any()
        .downcast_ref::<TextPayload>()
        .ok_or_else(|| invalid("invalid text payload"))?;
    let [r, g, b, _] = quantick_drawings::opaque(drawing.style.color).to_array();
    let alpha = drawing.style.color.to_array()[3];
    Ok(TextResult {
        target: target.clone(),
        properties: TextProperties {
            size_px: quantick_control::wire::CanonicalDecimal::new(payload.size_px.to_string())
                .map_err(invalid)?,
            family: match payload.family {
                FontFamily::Proportional => Family::Proportional,
                FontFamily::Monospace => Family::Monospace,
            },
            bold: payload.appearance.bold,
            italic: payload.appearance.italic,
            color: [r, g, b, alpha],
            background: payload.appearance.background,
            border: payload.appearance.border,
            wrap_width_px: payload
                .appearance
                .wrap_width_px
                .map(|width| {
                    quantick_control::wire::CanonicalDecimal::new(width.to_string())
                        .map_err(invalid)
                })
                .transpose()?,
        },
        text_present: !payload.text.is_empty(),
        changed,
    })
}
