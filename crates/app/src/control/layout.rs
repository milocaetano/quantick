//! The cockpit tier's canvas capabilities: rearranging the trader's charts.
//!
//! Every one of these calls the same function the menu and the keyboard call.
//! That is the point rather than a tidiness preference: a capability with its
//! own copy of "apply a layout" would drift from the one a click takes, and
//! the drift would be an assistant and a trader disagreeing about what the
//! canvas is currently showing.
//!
//! They live under the `cockpit` effect, which the `annotator` profile does
//! **not** inherit. The annotate tier's consent text tells the trader that
//! nothing granted there can rearrange their window; a capability that arrived
//! under a grant whose own words deny it would be a trust bug with no surface
//! to find it on.

use std::collections::BTreeSet;

use quantick_control::{
    error::ControlError,
    id::{CapabilityId, CostClassId, ModuleId, PermissionId, RiskFlagId},
    registry::{
        Availability, CapabilityDescriptor, EffectPersistence, ExpectedCost, IdempotencyPolicy,
        RegistryError, RevisionPolicy,
    },
    schema::generated_schema,
    wire::{ActorContext, WireU64},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{app::QuantickApp, canvas_layout, tab::CanvasLayout};

use super::{
    actions::{ActionRegistry, CAPABILITY_VERSION, NO_CONFIRMATION_ID, UI_BOUNDED_COST_ID},
    contract::{COCKPIT_EFFECT_ID, COCKPIT_LAYOUT_PERMISSION_ID, COCKPIT_PERMISSION_ID},
    gateway::ControlAccess,
};

/// The module every layout capability belongs to.
pub(crate) const LAYOUT_MODULE_ID: &str = "layout";

const APPLY_PRESET_ID: &str = "layout.preset.apply";
const MOVE_PANE_ID: &str = "layout.pane.move";
const RESIZE_ID: &str = "layout.pane.resize";
const COLLAPSE_ID: &str = "layout.pane.collapse";
const EXPAND_ID: &str = "layout.pane.expand";
const FOCUS_ID: &str = "layout.focus.set";
const INTERVAL_ID: &str = "layout.pane.set_interval";

/// Which tab a call is about. Omitted means the one the trader is looking at —
/// the same default the chrome's own commands take.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, JsonSchema)]
pub(crate) struct TabTarget {
    /// The tab's id, as `observe.workspace` reports it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_id: Option<WireU64>,
}

/// Apply a named arrangement from the layout registry.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub(crate) struct ApplyPresetInput {
    #[serde(flatten)]
    pub target: TabTarget,
    /// A preset id from the registry — `describe` lists them.
    pub preset_id: String,
}

/// Move one context chart within the stack.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub(crate) struct MovePaneInput {
    #[serde(flatten)]
    pub target: TabTarget,
    /// The pane's address now. `0` is the flow pane and cannot be moved.
    pub from: WireU64,
    /// Where it should sit.
    pub to: WireU64,
}

/// Set the context column's share of the canvas.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub(crate) struct ResizeInput {
    #[serde(flatten)]
    pub target: TabTarget,
    /// The share, 0..1. Held inside the same floor a drag is held to, so a
    /// call cannot reach a width a hand could not.
    pub fraction: f64,
}

/// Focus one pane: the chart the chrome speaks for and commands land on.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub(crate) struct FocusInput {
    #[serde(flatten)]
    pub target: TabTarget,
    /// The pane's address. `0` is the flow pane.
    pub pane: WireU64,
}

/// Set one context chart's timeframe.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub(crate) struct IntervalInput {
    #[serde(flatten)]
    pub target: TabTarget,
    /// The pane's address. `0` is the flow pane, which the toolbar governs.
    pub pane: WireU64,
    /// The interval in milliseconds.
    pub interval_ms: i64,
}

/// What every layout call answers with: the arrangement as it now stands.
///
/// Echoed back rather than assumed, so a client never has to guess whether a
/// call it made is the reason the canvas looks the way it does.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub(crate) struct LayoutResult {
    /// The tab that changed.
    pub tab_id: WireU64,
    /// The preset the canvas now matches.
    pub preset_id: String,
    /// How many panes it draws.
    pub pane_count: WireU64,
    /// The focused pane's address.
    pub focused_pane: WireU64,
    /// The context column's share of the canvas.
    pub fraction: f64,
    /// Whether the context column is collapsed to its rail.
    pub collapsed: bool,
    /// Whether the call changed anything. `false` is a real answer: applying
    /// the layout that is already showing is a no-op, not a failure.
    pub changed: bool,
}

pub(crate) fn register(registry: &mut ActionRegistry) -> Result<(), RegistryError> {
    registry.register(
        descriptor(
            APPLY_PRESET_ID,
            "Apply a layout preset",
            "Switches the canvas to a named arrangement from the layout registry — the same registry the toolbar picker and the View menu read.",
            generated_schema::<ApplyPresetInput>(),
        ),
        apply_preset,
    )?;
    registry.register(
        descriptor(
            MOVE_PANE_ID,
            "Move a chart within the stack",
            "Moves one context chart up or down the column beside the heatmap. The flow pane does not move: its column is the one thing every preset agrees on.",
            generated_schema::<MovePaneInput>(),
        ),
        move_pane,
    )?;
    registry.register(
        descriptor(
            RESIZE_ID,
            "Resize the context column",
            "Sets the context column's share of the canvas, held inside the same floor a divider drag is held to.",
            generated_schema::<ResizeInput>(),
        ),
        resize,
    )?;
    registry.register(
        descriptor(
            COLLAPSE_ID,
            "Collapse the context column",
            "Puts the context charts away, leaving the rail that brings them back. The width they had is kept, so expanding returns the layout the trader chose.",
            generated_schema::<TabTarget>(),
        ),
        collapse,
    )?;
    registry.register(
        descriptor(
            EXPAND_ID,
            "Expand the context column",
            "Brings the context charts back at the width they had before they were collapsed.",
            generated_schema::<TabTarget>(),
        ),
        expand,
    )?;
    registry.register(
        descriptor(
            FOCUS_ID,
            "Focus a chart",
            "Moves focus to one pane: the chart the status bar speaks for, and the one an indicator or drawing command lands on.",
            generated_schema::<FocusInput>(),
        ),
        focus,
    )?;
    registry.register(
        descriptor(
            INTERVAL_ID,
            "Set a chart's timeframe",
            "Sets one context chart's interval, the same value its own header selector writes.",
            generated_schema::<IntervalInput>(),
        ),
        set_interval,
    )?;
    Ok(())
}

fn layout_permissions() -> BTreeSet<PermissionId> {
    [COCKPIT_PERMISSION_ID, COCKPIT_LAYOUT_PERMISSION_ID]
        .into_iter()
        .map(|id| PermissionId::new(id).expect("static permission ID is valid"))
        .collect()
}

fn descriptor(
    id: &str,
    title: &str,
    description: &str,
    input_schema: Value,
) -> CapabilityDescriptor {
    CapabilityDescriptor {
        id: CapabilityId::new(id).expect("static capability ID is valid"),
        version: CAPABILITY_VERSION,
        title: title.to_owned(),
        description: description.to_owned(),
        module: ModuleId::new(LAYOUT_MODULE_ID).expect("static module ID is valid"),
        input_schema,
        output_schema: generated_schema::<LayoutResult>(),
        examples: Vec::new(),
        effect: quantick_control::id::EffectId::new(COCKPIT_EFFECT_ID)
            .expect("static effect ID is valid"),
        risk_flags: BTreeSet::<RiskFlagId>::new(),
        read_only: false,
        // Applying the same arrangement twice leaves the same arrangement, so
        // a client may retry a dropped call without wondering what the first
        // one did.
        idempotency: IdempotencyPolicy::Optional,
        revision_policy: RevisionPolicy::OptionalForAdditive,
        stale_input_safety: Some(
            "Rearranging a canvas removes no work: a chart taken off the screen keeps its drawings, its indicators and its bars, and comes back with them. A stale caller can only show the wrong charts, which the result it gets back names."
                .to_owned(),
        ),
        dry_run_supported: false,
        persistence: EffectPersistence::Durable,
        reversible: true,
        destructive: false,
        risk_reducing: false,
        required_permissions: layout_permissions(),
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

/// Which tab a call names, as an index into the open strip.
///
/// A tab id that no longer exists is refused rather than resolved to the
/// active one: a caller that named a tab meant that tab, and quietly acting on
/// a different market is the worst answer available.
fn tab_index(app: &QuantickApp, target: TabTarget) -> Result<usize, ControlError> {
    let Some(id) = target.tab_id else {
        return Ok(app.control_active_tab_index());
    };
    app.control_tabs()
        .iter()
        .position(|tab| tab.id == id.get())
        .ok_or_else(|| ControlError::invalid_request(format!("no open tab has id {}", id.get())))
}

fn result(app: &QuantickApp, index: usize, changed: bool) -> Result<Value, ControlError> {
    let tab = app
        .control_tab_at(index)
        .ok_or_else(|| ControlError::invalid_request("the tab closed while the call ran"))?;
    let focused = tab.focused_side().index() as u64;
    let payload = LayoutResult {
        tab_id: WireU64::new(tab.id),
        preset_id: tab.layout.preset().id.to_owned(),
        pane_count: WireU64::new(tab.pane_count() as u64),
        focused_pane: WireU64::new(focused),
        fraction: f64::from(tab.split_fraction),
        collapsed: tab.context_collapsed,
        changed,
    };
    serde_json::to_value(payload).map_err(|error| {
        ControlError::invalid_request(format!("the layout result could not be encoded: {error}"))
    })
}

fn apply_preset(
    app: &mut QuantickApp,
    _access: &mut ControlAccess,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: ApplyPresetInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let index = tab_index(app, input.target)?;
    let preset = canvas_layout::preset(&input.preset_id).ok_or_else(|| {
        ControlError::invalid_request(format!(
            "no layout preset is named {}; `describe` lists them",
            input.preset_id
        ))
    })?;
    let layout = CanvasLayout::from_preset(preset).ok_or_else(|| {
        ControlError::invalid_request(format!(
            "the canvas cannot draw the preset {} yet",
            preset.id
        ))
    })?;
    let tab = app
        .control_tab_at_mut(index)
        .ok_or_else(|| ControlError::invalid_request("the tab closed while the call ran"))?;
    let changed = tab.layout != layout;
    tab.set_layout(layout);
    result(app, index, changed)
}

fn move_pane(
    app: &mut QuantickApp,
    _access: &mut ControlAccess,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: MovePaneInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let index = tab_index(app, input.target)?;
    let (from, to) = (input.from.get() as usize, input.to.get() as usize);
    let tab = app
        .control_tab_at_mut(index)
        .ok_or_else(|| ControlError::invalid_request("the tab closed while the call ran"))?;
    // The one reposition path — the same call the View menu takes.
    let changed = tab.move_context_pane(from, to);
    result(app, index, changed)
}

fn resize(
    app: &mut QuantickApp,
    _access: &mut ControlAccess,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: ResizeInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let index = tab_index(app, input.target)?;
    let tab = app
        .control_tab_at_mut(index)
        .ok_or_else(|| ControlError::invalid_request("the tab closed while the call ran"))?;
    // The descriptor promises a call cannot reach a width a hand could not,
    // and that promise moved when the floor did: `clamp_pane_fraction` is a
    // 0..1 sanity clamp now, and the width floor lives in the splitter, which
    // a stored fraction never passes through. Left as it was, `fraction: 0.0`
    // stored a zero the canvas did not draw — and the trader's next divider
    // nudge, computing from that zero, collapsed the column instead of
    // widening it.
    //
    // A width the trader could not drag to is refused rather than clamped: a
    // caller that asked for a fifth of a pane meant something, and silently
    // giving them a different width is how a client and a canvas come to
    // disagree about what is on screen.
    let asked = crate::pane::clamp_pane_fraction(input.fraction as f32);
    let floor = canvas_layout::MIN_PANE_WIDTH_PX;
    let canvas = tab.last_canvas_width();
    if canvas > 0.0 {
        let wanted_px = asked * canvas;
        if wanted_px < floor || canvas - wanted_px < floor {
            return Err(ControlError::invalid_request(format!(
                "a share of {asked} leaves a pane under the {floor}px floor on this                  {canvas}px canvas; collapse the column instead of squeezing it away"
            )));
        }
    }
    let changed = (tab.split_fraction - asked).abs() > f32::EPSILON;
    tab.split_fraction = asked;
    result(app, index, changed)
}

fn collapse(
    app: &mut QuantickApp,
    _access: &mut ControlAccess,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    set_collapsed(app, input, true)
}

fn expand(
    app: &mut QuantickApp,
    _access: &mut ControlAccess,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    set_collapsed(app, input, false)
}

fn set_collapsed(
    app: &mut QuantickApp,
    input: &Value,
    collapsed: bool,
) -> Result<Value, ControlError> {
    let input: TabTarget = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let index = tab_index(app, input)?;
    let tab = app
        .control_tab_at_mut(index)
        .ok_or_else(|| ControlError::invalid_request("the tab closed while the call ran"))?;
    // The same call the divider drag, the rail and the menu take.
    let changed = tab.set_context_collapsed(collapsed);
    result(app, index, changed)
}

fn focus(
    app: &mut QuantickApp,
    _access: &mut ControlAccess,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: FocusInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let index = tab_index(app, input.target)?;
    let pane = input.pane.get() as usize;
    let tab = app
        .control_tab_at_mut(index)
        .ok_or_else(|| ControlError::invalid_request("the tab closed while the call ran"))?;
    if tab.pane_at(pane).is_none() {
        return Err(ControlError::invalid_request(format!(
            "this tab has no pane at address {pane}"
        )));
    }
    let wanted = crate::pane::PaneSide::from_index(pane);
    let changed = tab.focus != wanted;
    tab.focus = wanted;
    result(app, index, changed)
}

fn set_interval(
    app: &mut QuantickApp,
    _access: &mut ControlAccess,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: IntervalInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    if input.interval_ms < crate::state::MIN_TIME_INTERVAL_MS
        || input.interval_ms > crate::state::MAX_TIME_INTERVAL_MS
    {
        return Err(ControlError::invalid_request(format!(
            "an interval of {} ms is outside the range a chart accepts",
            input.interval_ms
        )));
    }
    let index = tab_index(app, input.target)?;
    let pane = input.pane.get() as usize;
    if pane == 0 {
        return Err(ControlError::invalid_request(
            "the flow pane's bars are set by the toolbar's BARS group, not by an interval"
                .to_owned(),
        ));
    }
    let tab = app
        .control_tab_at_mut(index)
        .ok_or_else(|| ControlError::invalid_request("the tab closed while the call ran"))?;
    let Some(chart) = tab.pane_at_mut(pane) else {
        return Err(ControlError::invalid_request(format!(
            "this tab has no context chart at address {pane}"
        )));
    };
    let changed = chart.time_interval_ms != input.interval_ms;
    chart.kind = crate::state::BarKind::Time;
    chart.time_interval_ms = input.interval_ms;
    result(app, index, changed)
}
