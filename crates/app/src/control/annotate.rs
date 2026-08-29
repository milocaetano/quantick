//! Answering on the chart: a label, an arrow or a zone placed against
//! resolved chart coordinates, attributed to whoever placed it, and removable
//! in one action.
//!
//! This is the half of the loop the observer tier could not carry. Everything
//! here is an *addition*: the actions place new objects through the same
//! [`Drawings::place_with`] door the pointer uses, and the only object any of
//! them can remove is one an operator other than the trader placed. Nothing in
//! this module can discard work done by hand (plan §2.6), which is what keeps
//! the annotate tier below the cockpit.

use std::collections::BTreeSet;

use quantick_control::{
    error::{ControlError, codes},
    id::{CapabilityId, CostClassId, EventKind, ModuleId, PermissionId, RiskFlagId},
    registry::{
        Availability, CapabilityDescriptor, EffectPersistence, ExpectedCost, IdempotencyPolicy,
        RegistryError, RevisionPolicy,
    },
    schema::generated_schema,
    wire::{ActorContext, ActorKind, CanonicalDecimal, WireU64},
};
use rust_decimal::prelude::ToPrimitive;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    app::QuantickApp,
    drawings::{self, ChartPoint, DrawingAuthor, DrawingBand},
    metrics,
    pane::ChartPane,
};

use super::{
    actions::{ANNOTATE_EFFECT_ID, ANNOTATE_PERMISSION_ID, ActionRegistry},
    gateway::ControlAccess,
    journal::{EventActor, NewEvent},
    types::{PaneSideDto, actor_kind_name, canonical_f64, known_error, wire_usize},
};

/// The module every annotation capability belongs to.
pub(crate) const ANNOTATE_MODULE_ID: &str = "annotate";
/// The scope that lets an operator add objects to the chart.
pub(crate) const ANNOTATE_CHART_PERMISSION_ID: &str = "annotate.chart";

pub(crate) const LABEL_CAPABILITY_ID: &str = "annotate.label.create";
pub(crate) const ARROW_CAPABILITY_ID: &str = "annotate.arrow.create";
pub(crate) const ZONE_CAPABILITY_ID: &str = "annotate.zone.create";
pub(crate) const REMOVE_CAPABILITY_ID: &str = "annotate.remove";

pub(crate) const ANNOTATION_CREATED_EVENT_KIND: &str = "annotate.object.created";
pub(crate) const ANNOTATION_REMOVED_EVENT_KIND: &str = "annotate.object.removed";

const CAPABILITY_VERSION: u32 = 1;
const NO_CONFIRMATION_ID: &str = "none";
const UI_BOUNDED_COST_ID: &str = "ui_bounded";

/// The registry ids of the three tools an annotation reaches for. They are
/// looked up by id in `DRAWING_TOOLS`, exactly as the rail does, so a rename
/// in the tool registry is a compile-time-visible lookup failure here rather
/// than a second list of tools.
const LABEL_TOOL_ID: &str = "text";
const ARROW_TOOL_ID: &str = "arrow";
const ZONE_TOOL_ID: &str = "rectangle";

/// The longest label an annotation may carry. A note is a sentence on a
/// chart, not a document; the bound is what keeps one call from covering the
/// tape.
const ANNOTATION_TEXT_MAX_BYTES: usize = 280;
/// The longest trader-facing name an annotation may carry, matching what the
/// object manager's rename accepts.
const ANNOTATION_NAME_MAX_BYTES: usize = 120;
/// Decimal places an anchor price is reported back with. Prices on the wire
/// are exact text (types.rs); eight places is past every venue's tick.
const ANNOTATION_PRICE_DECIMALS: u32 = 8;

/// Where an annotation goes. Omitting both halves means the chart the trader
/// is looking at — the same default the toolbar's own placement uses.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct AnnotationTarget {
    /// The tab, by the id every snapshot reports. Omitted: the active tab.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_id: Option<WireU64>,
    /// The pane within that tab. Omitted: the pane drawings go to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane_side: Option<PaneSideDto>,
    /// Which context chart `pane_side = "time"` means, top to bottom from
    /// `0`. Omitted: the top one. Ignored for the flow pane, which has no
    /// stack to pick from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane_slot: Option<WireU64>,
}

/// One anchor, in the coordinates the cursor and the chart window report:
/// market time and price. Screen pixels are deliberately not accepted — they
/// mean nothing a frame later, and an agent that read a bar knows its time.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct AnnotationAnchor {
    #[schemars(extend("x-unit" = "unix_milliseconds"))]
    pub time_unix_ms: i64,
    pub price: CanonicalDecimal,
}

/// What a label, an arrow or a zone takes. The anchor count is the tool's
/// (one for a label, two for an arrow and a zone) and is checked against the
/// registry rather than restated here.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct AnnotationInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<AnnotationTarget>,
    #[schemars(length(min = 1, max = 2))]
    pub anchors: Vec<AnnotationAnchor>,
    /// The words a label carries. Ignored by the tools that have none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(max = ANNOTATION_TEXT_MAX_BYTES))]
    pub text: Option<String>,
    /// The name the object manager and the inspector show.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(max = ANNOTATION_NAME_MAX_BYTES))]
    pub name: Option<String>,
}

/// What an annotation returns: the object's stable id, where it actually
/// landed, and the authorship the trader sees on it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct AnnotationResult {
    pub annotation_id: WireU64,
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    pub pane_side: PaneSideDto,
    pub tool_id: String,
    /// Where each anchor landed after resolution: the slot it fell on and the
    /// market time of that slot, which is not always the time asked for.
    pub anchors: Vec<ResolvedAnchor>,
    pub author: AnnotationAuthor,
    pub label: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct ResolvedAnchor {
    pub slot: WireU64,
    #[schemars(extend("x-unit" = "unix_milliseconds"))]
    pub time_unix_ms: i64,
    pub price: CanonicalDecimal,
}

/// Who placed an object, as the interface shows it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct AnnotationAuthor {
    /// The actor kind on the wire: `agent`, `automation`, `human_ui`.
    pub actor_kind: String,
    /// The client's own name from its handshake.
    pub client_name: String,
}

/// What a removal takes: the annotation's id, and nothing else. There is no
/// "remove all" here on purpose — an operator that can sweep the chart is a
/// cockpit capability, and the trader's own sweep lives in the object manager.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct RemoveInput {
    pub annotation_id: WireU64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct RemoveResult {
    pub annotation_id: WireU64,
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    pub removed: bool,
}

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
    registry.register(remove_descriptor(), remove_annotation)?;
    Ok(())
}

fn annotate_permissions(scope: &str) -> BTreeSet<PermissionId> {
    [ANNOTATE_PERMISSION_ID, scope]
        .into_iter()
        .map(|id| PermissionId::new(id).expect("static permission ID is valid"))
        .collect()
}

fn annotation_descriptor(id: &str, title: &str, description: &str) -> CapabilityDescriptor {
    CapabilityDescriptor {
        id: CapabilityId::new(id).expect("static capability ID is valid"),
        version: CAPABILITY_VERSION,
        title: title.to_owned(),
        description: description.to_owned(),
        module: ModuleId::new(ANNOTATE_MODULE_ID).expect("static module ID is valid"),
        input_schema: generated_schema::<AnnotationInput>(),
        output_schema: generated_schema::<AnnotationResult>(),
        examples: Vec::new(),
        effect: quantick_control::id::EffectId::new(ANNOTATE_EFFECT_ID)
            .expect("static effect ID is valid"),
        risk_flags: BTreeSet::<RiskFlagId>::new(),
        read_only: false,
        idempotency: IdempotencyPolicy::Forbidden,
        // An annotation adds an object and overwrites none, so a stale caller
        // can only place the wrong thing — visible, attributed, and removed
        // in one action.
        revision_policy: RevisionPolicy::OptionalForAdditive,
        stale_input_safety: Some(
            "An annotation adds one object and edits nothing; a stale caller places an object that is visibly attributed and removed in one action."
                .to_owned(),
        ),
        dry_run_supported: false,
        persistence: EffectPersistence::Durable,
        reversible: true,
        destructive: false,
        risk_reducing: false,
        required_permissions: annotate_permissions(ANNOTATE_CHART_PERMISSION_ID),
        preconditions: Vec::new(),
        confirmation_class: quantick_control::id::ConfirmationClassId::new(NO_CONFIRMATION_ID)
            .expect("static confirmation class is valid"),
        availability: Availability::available(),
        expected_cost: ExpectedCost {
            class: CostClassId::new(UI_BOUNDED_COST_ID).expect("static cost ID is valid"),
            max_items: None,
            max_response_bytes: Some(quantick_control::limits::CONTROL_MAX_RESPONSE_BYTES),
        },
        pagination: None,
    }
}

fn remove_descriptor() -> CapabilityDescriptor {
    CapabilityDescriptor {
        id: CapabilityId::new(REMOVE_CAPABILITY_ID).expect("static capability ID is valid"),
        version: CAPABILITY_VERSION,
        title: "Remove an annotation".to_owned(),
        description: "Removes one object placed by an operator other than the trader. An object the trader drew by hand is never removable through this tier.".to_owned(),
        module: ModuleId::new(ANNOTATE_MODULE_ID).expect("static module ID is valid"),
        input_schema: generated_schema::<RemoveInput>(),
        output_schema: generated_schema::<RemoveResult>(),
        examples: Vec::new(),
        effect: quantick_control::id::EffectId::new(ANNOTATE_EFFECT_ID)
            .expect("static effect ID is valid"),
        risk_flags: BTreeSet::<RiskFlagId>::new(),
        read_only: false,
        idempotency: IdempotencyPolicy::Forbidden,
        revision_policy: RevisionPolicy::OptionalForAdditive,
        stale_input_safety: Some(
            "Removing an already-removed annotation reports that it was not there; no other object can be reached."
                .to_owned(),
        ),
        dry_run_supported: false,
        persistence: EffectPersistence::Durable,
        reversible: true,
        destructive: false,
        risk_reducing: false,
        required_permissions: annotate_permissions(ANNOTATE_CHART_PERMISSION_ID),
        preconditions: Vec::new(),
        confirmation_class: quantick_control::id::ConfirmationClassId::new(NO_CONFIRMATION_ID)
            .expect("static confirmation class is valid"),
        availability: Availability::available(),
        expected_cost: ExpectedCost {
            class: CostClassId::new(UI_BOUNDED_COST_ID).expect("static cost ID is valid"),
            max_items: None,
            max_response_bytes: Some(quantick_control::limits::CONTROL_MAX_RESPONSE_BYTES),
        },
        pagination: None,
    }
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
    let author = match access.recorded_author() {
        Some(recorded) => (recorded.actor_kind != ActorKind::HumanUi).then(|| DrawingAuthor {
            actor_kind: actor_kind_name(recorded.actor_kind).to_owned(),
            client_name: recorded.client_name.clone(),
        }),
        None => (actor.actor_kind != ActorKind::HumanUi).then(|| DrawingAuthor {
            actor_kind: actor_kind_name(actor.actor_kind).to_owned(),
            client_name: actor.client_name.clone(),
        }),
    };
    // The look a fresh object opens with is read before the pane is borrowed
    // mutably, through the app's own door: an annotation looks like what the
    // trader would have drawn. One is enough for the whole placement —
    // `place_with` asks for the opening only when it installs the draft, so
    // the second anchor of an arrow or a zone never calls for another.
    let mut fresh = Some(app.control_new_drawing(tool));
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

    let mut completed = false;
    for point in points {
        completed = pane
            .drawings
            .place_with(tool, &DrawingBand::Price, point, |_| {
                fresh.take().expect("one opening look per placement")
            });
    }
    if !completed {
        // The anchors that did land are sitting in a draft nobody owns. Left
        // there, the pane reads as "the trader is drawing right now" for the
        // rest of the session and every later annotation on it is refused —
        // and a half-drawn object the trader never started is on their chart.
        pane.drawings.cancel_draft();
        return Err(capability_unavailable(format!(
            "the `{}` tool did not complete from {required} anchor(s)",
            tool.name()
        )));
    }
    let Some(drawing) = pane.drawings.selected_mut() else {
        return Err(capability_unavailable(
            "the placed annotation could not be read back",
        ));
    };
    // Authorship before anything else touches it: an object that reaches the
    // chart without saying who placed it is indistinguishable from the
    // trader's own hand, which is the one thing this tier may not do.
    drawing.author = author;
    drawing.name = input.name.clone();
    if let Some(text) = &input.text
        && drawing.tool.holds_text()
    {
        drawing
            .tool
            .set_inline_text(drawing.payload.as_mut(), text.clone());
    }
    let annotation_id = drawing.id.0;
    let index = pane.drawings.selected().unwrap_or_default();
    let label = pane
        .drawings
        .items()
        .get(index)
        .map_or_else(String::new, |drawing| drawing.display_label(index));

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
        let pane = app.control_pane_mut(tab_index, side);
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
        pane.sweep_strategy_orphans();
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
    let tab_id = app.control_tabs()[tab_index].id;
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
    for (index, tab) in app.control_tabs().iter().enumerate() {
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
    let tabs = app.control_tabs();
    if tabs.is_empty() {
        return Err(capability_unavailable("this window has no chart open"));
    }
    let active = app.control_active_tab_index().min(tabs.len() - 1);
    let tab_index = match target.and_then(|target| target.tab_id) {
        None => active,
        Some(tab_id) => tabs
            .iter()
            .position(|tab| tab.id == tab_id.get())
            .ok_or_else(|| {
                ControlError::invalid_request(format!("no open tab has id {}", tab_id.get()))
            })?,
    };
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
    Ok((tab.id, side))
}

fn control_pane_mut(
    app: &mut QuantickApp,
    tab_id: u64,
    side: crate::pane::PaneSide,
) -> Result<&mut ChartPane, ControlError> {
    let index = app
        .control_tabs()
        .iter()
        .position(|tab| tab.id == tab_id)
        .ok_or_else(|| ControlError::invalid_request("the target tab closed"))?;
    Ok(app.control_pane_mut(index, side))
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

fn parse_price(price: &CanonicalDecimal) -> Result<f64, ControlError> {
    price
        .as_str()
        .parse::<rust_decimal::Decimal>()
        .ok()
        .and_then(|value| value.to_f64())
        .filter(|value| value.is_finite())
        .ok_or_else(|| ControlError::invalid_request("an anchor price is not a finite decimal"))
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
