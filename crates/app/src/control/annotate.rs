//! Answering on the chart: a label, an arrow, a zone or a range profile placed against
//! resolved chart coordinates, attributed to whoever placed it, and removable
//! in one action.
//!
//! This is the half of the loop the observer tier could not carry. Everything
//! here is an *addition*: the actions place new objects through the same
//! [`Drawings::place_with`] door the pointer uses, and the only object any of
//! them can remove is one an operator other than the trader placed. Nothing in
//! this module can discard work done by hand (plan §2.6), which is what keeps
//! the annotate tier below the cockpit.

use quantick_control::{
    error::{ControlError, codes},
    id::{EventKind, ModuleId},
    registry::RegistryError,
    wire::{ActorContext, ActorKind, WireU64},
};
use serde::Serialize;
use serde_json::{Value, json};

use crate::{
    app::QuantickApp,
    drawings::{self, ChartPoint, DrawingAuthor, DrawingBand},
    metrics,
    pane::ChartPane,
};

use super::{
    actions::ActionRegistry,
    gateway::ControlAccess,
    journal::{EventActor, NewEvent},
    types::{PaneSideDto, actor_kind_name, canonical_f64, known_error, wire_usize},
};

pub(crate) use quantick_control::annotation::*;

mod series;

/// Dock the annotate tier's actions.
pub(crate) fn register(registry: &mut ActionRegistry) -> Result<(), RegistryError> {
    registry.register(
        annotation_descriptor(
            LABEL_CAPABILITY_ID,
            "Place a label",
            "Places an anchored note at one chart coordinate, attributed to its author and removable in one action.",
        ),
        create_label,
    )?;
    registry.register(
        annotation_descriptor(
            ARROW_CAPABILITY_ID,
            "Place an arrow",
            "Draws an arrow between two chart coordinates, attributed to its author and removable in one action.",
        ),
        create_arrow,
    )?;
    registry.register(
        annotation_descriptor(
            ZONE_CAPABILITY_ID,
            "Place a zone",
            "Draws a rectangular region between two chart coordinates, attributed to its author and removable in one action.",
        ),
        create_zone,
    )?;
    registry.register(
        annotation_descriptor(
            PROFILE_CAPABILITY_ID,
            "Place a fixed-range volume profile",
            "Folds traded volume over the market-time range between two chart coordinates and places the result as an attributed drawing.",
        ),
        create_profile,
    )?;
    registry.register(
        chart_descriptor(
            PROFILE_CAPABILITY_ID,
            PROFILE_CAPABILITY_VERSION,
            "Place a fixed-range volume profile",
            "Folds traded volume over a chart range, including projected space beyond the latest bar.",
        ),
        create_chart_profile,
    )?;
    registry.register(
        fib_descriptor(
            FIB_RETRACEMENT_CAPABILITY_ID,
            "Place a Fibonacci retracement",
            "Draws retracement levels over one move between two chart coordinates.",
        ),
        create_fib_retracement,
    )?;
    registry.register(
        fib_descriptor(
            FIB_PROJECTION_CAPABILITY_ID,
            "Place a Fibonacci projection",
            "Projects one measured move from a third chart coordinate.",
        ),
        create_fib_projection,
    )?;
    registry.register(remove_descriptor(), remove_annotation)?;
    Ok(())
}

fn create_label(
    app: &mut QuantickApp,
    access: &mut ControlAccess,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    place(app, access, actor, input, LABEL_TOOL_ID)
}

fn create_arrow(
    app: &mut QuantickApp,
    access: &mut ControlAccess,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    place(app, access, actor, input, ARROW_TOOL_ID)
}

fn create_zone(
    app: &mut QuantickApp,
    access: &mut ControlAccess,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    place(app, access, actor, input, ZONE_TOOL_ID)
}

fn create_profile(
    app: &mut QuantickApp,
    access: &mut ControlAccess,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    place(app, access, actor, input, crate::frvp::TOOL_ID)
}

fn create_chart_profile(
    app: &mut QuantickApp,
    access: &mut ControlAccess,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    place_chart(app, access, actor, input, crate::frvp::TOOL_ID)
}

fn create_fib_retracement(
    app: &mut QuantickApp,
    access: &mut ControlAccess,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    place_chart(app, access, actor, input, "fib-retracement")
}

fn create_fib_projection(
    app: &mut QuantickApp,
    access: &mut ControlAccess,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    place_chart(app, access, actor, input, "fib-extension")
}

fn place_chart(
    app: &mut QuantickApp,
    access: &mut ControlAccess,
    actor: &ActorContext,
    input: &Value,
    tool_id: &str,
) -> Result<Value, ControlError> {
    let input: ChartAnnotationInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let tool = drawings::DrawingTool::by_id(tool_id).ok_or_else(|| {
        capability_unavailable(format!(
            "the `{tool_id}` drawing tool is not registered in this build"
        ))
    })?;
    let required = tool.required_points();
    let (tab_id, pane_side) = resolve_target(app, input.target.as_ref())?;
    let author = annotation_author(access, actor);
    let fresh = app.control_reads().new_drawing(tool);
    let pane = control_pane_mut(app, tab_id, pane_side)?;

    let validated = series::resolve(pane, tab_id, &input, required)?;
    let points: Vec<_> = validated
        .iter()
        .map(|anchor| {
            ChartPoint::at_time(anchor.point.bar, anchor.point.price, anchor.point.time_ms)
        })
        .collect();
    let resolved = validated
        .iter()
        .map(|anchor| {
            let price = canonical_f64(anchor.point.price, ANNOTATION_PRICE_DECIMALS)
                .expect("validated finite price");
            match anchor.slot {
                Some(slot) => ChartResolvedAnchor::Market(ResolvedAnchor {
                    slot: wire_usize(slot),
                    time_unix_ms: anchor
                        .point
                        .time_ms
                        .expect("a resolved market anchor has time"),
                    price,
                }),
                None => ChartResolvedAnchor::Future {
                    bar_position: canonical_bar_position(anchor.point.bar)
                        .expect("validated finite bar position"),
                    price,
                },
            }
        })
        .collect();
    let (annotation_id, label) = install(pane, tool, points, fresh, author, input.name, None)?;
    let result = ChartAnnotationResult {
        annotation_id: WireU64::new(annotation_id),
        tab_id: WireU64::new(tab_id),
        pane_id: WireU64::new(pane.id),
        pane_side: pane_side.into(),
        tool_id: tool.id().to_owned(),
        anchors: resolved,
        author: AnnotationAuthor {
            actor_kind: actor_kind_name(actor.actor_kind).to_owned(),
            client_name: actor.client_name.clone(),
        },
        label,
    };
    journal_annotation(access, actor, ANNOTATION_CREATED_EVENT_KIND, &result)?;
    serde_json::to_value(result)
        .map_err(|error| ControlError::invalid_request(format!("annotation result: {error}")))
}

/// Serialize the model's exact operation through the existing admitted capability.
pub(crate) fn quick_range_input(
    request: &quantick_chart_interaction::quick_range::PlaceRequest,
    pane_side: crate::pane::PaneSide,
) -> Option<Value> {
    use quantick_chart_interaction::quick_range::Action;
    let pane_slot = match pane_side {
        crate::pane::PaneSide::Flow => None,
        crate::pane::PaneSide::Time(slot) => Some(WireU64::new(u64::try_from(slot).ok()?)),
    };
    let mut points = request.anchors.to_vec();
    if request.action == Action::Projection {
        points.push(points[1]);
    }
    let anchors = points
        .into_iter()
        .map(|anchor| {
            Some(ChartAnnotationAnchor {
                time_unix_ms: anchor.time_ms,
                bar_position: canonical_bar_position(anchor.bar)?,
                price: canonical_f64(anchor.price, ANNOTATION_PRICE_DECIMALS)?,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    serde_json::to_value(ChartAnnotationInput {
        target: Some(AnnotationTarget {
            tab_id: Some(WireU64::new(request.context.owner.tab)),
            pane_side: Some(pane_side.into()),
            pane_slot,
        }),
        anchors,
        name: None,
        chart_reference: Some(ChartReference {
            pane_id: WireU64::new(request.context.owner.pane),
            series_revision: WireU64::new(request.context.revision),
            layout_id: request.context.owner.layout.map(WireU64::new),
        }),
    })
    .ok()
}

/// The one placement path: resolve the target pane, resolve every anchor
/// against that pane's series, then place through the tool registry exactly
/// as a click does.
fn place(
    app: &mut QuantickApp,
    access: &mut ControlAccess,
    actor: &ActorContext,
    input: &Value,
    tool_id: &str,
) -> Result<Value, ControlError> {
    let input: AnnotationInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let tool = drawings::DrawingTool::by_id(tool_id).ok_or_else(|| {
        capability_unavailable(format!(
            "the `{tool_id}` drawing tool is not registered in this build"
        ))
    })?;
    let required = tool.required_points();
    if input.anchors.len() != required {
        return Err(ControlError::invalid_request(format!(
            "`{}` takes exactly {required} anchor(s), not {}",
            tool.name(),
            input.anchors.len()
        )));
    }
    let (tab_id, pane_side) = resolve_target(app, input.target.as_ref())?;
    // Only an operator other than the trader is stamped. `None` is the
    // trader's own hand, which is what every surface reads to decide whether
    // to say anything at all: an object the trader placed through this very
    // action — the hotkey path, a test — is theirs, not an assistant's.
    //
    // A replay attributes to whoever the recorded run named, so a rerun of a
    // session reproduces its authorship instead of stamping everything as the
    // automation that is replaying it.
    let author = annotation_author(access, actor);
    // The look a fresh object opens with is read before the pane is borrowed
    // mutably, through the app's own door: an annotation looks like what the
    // trader would have drawn. One is enough for the whole placement —
    // `place_with` asks for the opening only when it installs the draft, so
    // the second anchor of an arrow or a zone never calls for another.
    let fresh = app.control_reads().new_drawing(tool);
    let pane = control_pane_mut(app, tab_id, pane_side)?;
    let pane_id = pane.id;
    // The trader is mid-gesture: `place_with` would push this call's anchor
    // onto *their* draft (same tool) or replace it outright (different tool),
    // which is exactly the "discards work done by hand" this tier may not do.
    // Refused, retryable, with the reason — the assistant tries again when
    // the hand has finished.
    if pane.drawings.draft().is_some() {
        return Err(capability_unavailable(
            "the trader is drawing on that pane right now; an annotation would land in their unfinished object",
        ));
    }

    let mut resolved = Vec::with_capacity(input.anchors.len());
    let mut points = Vec::with_capacity(input.anchors.len());
    for anchor in &input.anchors {
        let (slot, time_ms) = resolve_slot(pane, anchor.time_unix_ms)?;
        let price = parse_price(&anchor.price)?;
        points.push(ChartPoint::at_time(
            slot as f32 + 0.5,
            price,
            pane.slot_open_time(slot),
        ));
        resolved.push(ResolvedAnchor {
            slot: wire_usize(slot),
            time_unix_ms: time_ms,
            price: canonical_f64(price, ANNOTATION_PRICE_DECIMALS).ok_or_else(|| {
                ControlError::invalid_request("an anchor price is not a finite decimal")
            })?,
        });
    }

    let (annotation_id, label) =
        install(pane, tool, points, fresh, author, input.name, input.text)?;
    let result = AnnotationResult {
        annotation_id: WireU64::new(annotation_id),
        tab_id: WireU64::new(tab_id),
        pane_id: WireU64::new(pane_id),
        pane_side: pane_side.into(),
        tool_id: tool.id().to_owned(),
        anchors: resolved,
        // The result always says who acted, even when the object carries no
        // author because the trader placed it themselves.
        author: AnnotationAuthor {
            actor_kind: actor_kind_name(actor.actor_kind).to_owned(),
            client_name: actor.client_name.clone(),
        },
        label,
    };
    journal_annotation(access, actor, ANNOTATION_CREATED_EVENT_KIND, &result)?;
    serde_json::to_value(&result)
        .map_err(|error| ControlError::invalid_request(format!("annotation result: {error}")))
}

/// Both wire versions commit through one store operation, after complete validation.
fn install(
    pane: &mut ChartPane,
    tool: drawings::DrawingTool,
    points: Vec<ChartPoint>,
    fresh: drawings::NewDrawing,
    author: Option<DrawingAuthor>,
    name: Option<String>,
    text: Option<String>,
) -> Result<(u64, String), ControlError> {
    let required = points.len();
    let mut fresh = Some(fresh);
    let mut completed = false;
    for point in points {
        completed = pane
            .drawings
            .place_with(tool, &DrawingBand::Price, point, |_| {
                fresh.take().expect("one opening look per placement")
            });
    }
    if !completed {
        // Never leave a partial draft which would block later admitted operations.
        pane.drawings.cancel_draft();
        return Err(capability_unavailable(format!(
            "the `{}` tool did not complete from {required} anchor(s)",
            tool.name()
        )));
    }
    let drawing = pane
        .drawings
        .selected_mut()
        .ok_or_else(|| capability_unavailable("the placed annotation could not be read back"))?;
    drawing.author = author;
    drawing.name = name;
    if let Some(text) = text
        && drawing.tool.holds_text()
    {
        drawing.tool.set_inline_text(drawing.payload.as_mut(), text);
    }
    let id = drawing.id.0;
    let index = pane.drawings.selected().unwrap_or_default();
    let label = pane
        .drawings
        .items()
        .get(index)
        .map_or_else(String::new, |drawing| drawing.display_label(index));
    Ok((id, label))
}

/// Remove one annotation — and only an annotation. An object the trader drew
/// stays where it is, whatever id was asked for (plan §2.6: this tier cannot
/// discard work done by hand).
fn remove_annotation(
    app: &mut QuantickApp,
    access: &mut ControlAccess,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: RemoveInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let annotation_id = input.annotation_id.get();
    let mut found = None;
    for (tab_index, side) in annotated_panes(app) {
        let pane = app.control_actions().pane_mut(tab_index, side);
        let Some(index) = pane
            .drawings
            .items()
            .iter()
            .position(|drawing| drawing.id.0 == annotation_id)
        else {
            continue;
        };
        if pane.drawings.items()[index].author.is_none() {
            return Err(permission_denied(
                "that object was drawn by the trader; the annotate tier removes only what an operator placed",
            ));
        }
        let pane_id = pane.id;
        let id = pane.drawings.items()[index].id;
        pane.drawings.remove_by_id(id);
        // An annotation can be the region a strategy is armed on, like any
        // other object: the same sweep every removal path in the interface
        // does, so no resting simulated order outlives the mark it names.
        pane.strategies.sweep_orphans(&pane.drawings);
        found = Some((tab_index, pane_id));
        break;
    }
    let Some((tab_index, pane_id)) = found else {
        return serde_json::to_value(RemoveResult {
            annotation_id: input.annotation_id,
            tab_id: WireU64::new(0),
            pane_id: WireU64::new(0),
            removed: false,
        })
        .map_err(|error| ControlError::invalid_request(format!("removal result: {error}")));
    };
    let tab_id = app.control_reads().tabs().id_at(tab_index);
    let result = RemoveResult {
        annotation_id: input.annotation_id,
        tab_id: WireU64::new(tab_id),
        pane_id: WireU64::new(pane_id),
        removed: true,
    };
    journal_annotation(access, actor, ANNOTATION_REMOVED_EVENT_KIND, &result)?;
    serde_json::to_value(&result)
        .map_err(|error| ControlError::invalid_request(format!("removal result: {error}")))
}

/// Every (tab, pane) an annotation could be sitting on.
fn annotated_panes(app: &QuantickApp) -> Vec<(usize, crate::pane::PaneSide)> {
    let mut panes = Vec::new();
    for (index, tab) in app.control_reads().tabs().iter().enumerate() {
        panes.extend(tab.sides().map(|side| (index, side)));
    }
    panes
}

fn journal_annotation<T: Serialize>(
    access: &mut ControlAccess,
    actor: &ActorContext,
    kind: &str,
    payload: &T,
) -> Result<(), ControlError> {
    let event_actor = EventActor {
        kind: actor.actor_kind,
        client_name: actor.client_name.clone(),
    };
    let payload = serde_json::to_value(payload)
        .map_err(|error| ControlError::invalid_request(format!("annotation event: {error}")))?;
    access.journal_mut().record(
        NewEvent {
            module_id: ModuleId::new(ANNOTATE_MODULE_ID).expect("static module ID is valid"),
            kind: EventKind::new(kind).expect("static event kind is valid"),
            actor: Some(event_actor),
            payload: json!({ "annotation": payload }),
        },
        metrics::wall_clock_ms(),
    );
    Ok(())
}

/// Which tab and pane an annotation addresses, defaulting to the chart the
/// trader is looking at.
fn resolve_target(
    app: &QuantickApp,
    target: Option<&AnnotationTarget>,
) -> Result<(u64, crate::pane::PaneSide), ControlError> {
    let tabs = app.control_reads().tabs();
    if tabs.is_empty() {
        return Err(capability_unavailable("this window has no chart open"));
    }
    let active = app.control_reads().active_tab_index().min(tabs.len() - 1);
    let tab_index = match target.and_then(|target| target.tab_id) {
        None => active,
        Some(tab_id) => tabs.position(tab_id.get()).ok_or_else(|| {
            ControlError::invalid_request(format!("no open tab has id {}", tab_id.get()))
        })?,
    };
    let tab_id = tabs.id_at(tab_index);
    let tab = &tabs[tab_index];
    let side = match target.and_then(|target| target.pane_side) {
        None => tab.drawing_side(),
        Some(PaneSideDto::Flow) => crate::pane::PaneSide::Flow,
        Some(PaneSideDto::Time) => {
            let slot = target
                .and_then(|target| target.pane_slot)
                .map_or(0, |slot| slot.get() as usize);
            if slot >= tab.time_panes.len() {
                return Err(capability_unavailable(format!(
                    "that tab has no time pane at slot {slot} ({} open)",
                    tab.time_panes.len()
                )));
            }
            crate::pane::PaneSide::Time(slot)
        }
    };
    Ok((tab_id, side))
}

fn control_pane_mut(
    app: &mut QuantickApp,
    tab_id: u64,
    side: crate::pane::PaneSide,
) -> Result<&mut ChartPane, ControlError> {
    let index = app
        .control_reads()
        .tabs()
        .position(tab_id)
        .ok_or_else(|| ControlError::invalid_request("the target tab closed"))?;
    Ok(app.control_actions().pane_mut(index, side))
}

/// The slot a market time falls on, and the time that slot actually opened.
fn resolve_slot(pane: &ChartPane, time_unix_ms: i64) -> Result<(usize, i64), ControlError> {
    if pane.slots() == 0 {
        return Err(capability_unavailable(
            "that chart has no bars yet, so an anchor has nothing to land on",
        ));
    }
    let slot = pane.slot_at_time(time_unix_ms).ok_or_else(|| {
        let mut error = ControlError::invalid_request(
            "no bar on that chart covers the anchor time",
        );
        error.context.next_steps = vec![
            "Read a bar's open_time_unix_ms from chart.window.read or the cursor, and anchor to that."
                .to_owned(),
        ];
        error
    })?;
    let time = pane.slot_open_time(slot).unwrap_or(time_unix_ms);
    Ok((slot, time))
}

fn annotation_author(access: &ControlAccess, actor: &ActorContext) -> Option<DrawingAuthor> {
    match access.recorded_author() {
        Some(recorded) => (recorded.actor_kind != ActorKind::HumanUi).then(|| DrawingAuthor {
            actor_kind: actor_kind_name(recorded.actor_kind).to_owned(),
            client_name: recorded.client_name.clone(),
        }),
        None => (actor.actor_kind != ActorKind::HumanUi).then(|| DrawingAuthor {
            actor_kind: actor_kind_name(actor.actor_kind).to_owned(),
            client_name: actor.client_name.clone(),
        }),
    }
}

/// A capability that exists but cannot act right now — a pane that is not
/// open, a chart with no bars yet. Retryable: the condition is the session's,
/// not the request's.
fn capability_unavailable(message: impl AsRef<str>) -> ControlError {
    known_error(codes::CAPABILITY_UNAVAILABLE, message, true)
}

/// The refusal that keeps this tier under the cockpit: an operator asked for
/// something only the trader's own hand may do.
fn permission_denied(message: impl AsRef<str>) -> ControlError {
    known_error(codes::PERMISSION_DENIED, message, false)
}
