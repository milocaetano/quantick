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

const APPLY_PRESET_CAPABILITY_ID: &str = "layout.preset.apply";
const MOVE_PANE_CAPABILITY_ID: &str = "layout.pane.move";
const RESIZE_CAPABILITY_ID: &str = "layout.pane.resize";
const COLLAPSE_CAPABILITY_ID: &str = "layout.pane.collapse";
const EXPAND_CAPABILITY_ID: &str = "layout.pane.expand";
const FOCUS_CAPABILITY_ID: &str = "layout.focus.set";
const INTERVAL_CAPABILITY_ID: &str = "layout.pane.set_interval";
const TAB_SWITCH_CAPABILITY_ID: &str = "layout.tab.switch";
const TAB_CREATE_CAPABILITY_ID: &str = "layout.tab.create";
const TAB_RENAME_CAPABILITY_ID: &str = "layout.tab.rename";

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

/// Which layout tab a call is about: by id, by name, or — omitted — the
/// active one. An id and a name that disagree are refused rather than
/// resolved, because a caller that gave both meant both.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
pub(crate) struct LayoutTabTarget {
    /// The layout's id, as `observe.workspace` reports it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout_id: Option<WireU64>,
    /// The layout's name, as the strip shows it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Put a layout on one pane.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
pub(crate) struct SwitchLayoutTabInput {
    #[serde(flatten)]
    pub layout: LayoutTabTarget,
    /// Which tab's pane changes layout. Omitted: the active tab.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_id: Option<WireU64>,
    /// The pane's address (`0` the flow pane, `1..` the context stack).
    /// Omitted: the focused pane — the pane the strip's own click switches.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane: Option<WireU64>,
}

/// Add a layout tab and switch to it.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
pub(crate) struct CreateLayoutTabInput {
    /// What to call it. Omitted: the first free `Layout N`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Rename a layout tab.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub(crate) struct RenameLayoutTabInput {
    #[serde(flatten)]
    pub target: LayoutTabTarget,
    /// The new name.
    pub new_name: String,
}

/// One layout tab, as the strip lists it.
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct LayoutTabSnapshot {
    pub layout_id: WireU64,
    pub name: String,
    pub active: bool,
    /// How many indicators the layout holds.
    pub indicator_count: WireU64,
}

/// What every layout-tab call answers with: the strip as it now stands.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub(crate) struct LayoutTabResult {
    pub active_layout_id: WireU64,
    pub active_layout_name: String,
    pub layouts: Vec<LayoutTabSnapshot>,
    /// Whether the call changed anything. `false` is a real answer:
    /// switching to the layout already showing is a no-op, not a failure.
    pub changed: bool,
}

/// The strip as the control plane reports it — one reading for the layout
/// calls and `observe.workspace` alike.
pub(crate) fn layout_tabs(app: &QuantickApp) -> Vec<LayoutTabSnapshot> {
    // "Active" on the wire is what the strip lights: the focused pane's
    // layout. Every pane's own is in `workspace.summary`.
    layout_tabs_marking(app, app.focused_pane_layout())
}

/// The same reading with `active` on a layout the caller names.
///
/// A call that addressed *another* pane answers about that pane: reporting
/// the focused pane's layout to a client that just switched a background one
/// tells it its call did not land, when it did.
fn layout_tabs_marking(
    app: &QuantickApp,
    active: crate::layouts::LayoutId,
) -> Vec<LayoutTabSnapshot> {
    app.layouts()
        .layouts()
        .iter()
        .map(|layout| LayoutTabSnapshot {
            layout_id: WireU64::new(layout.id.0),
            name: layout.name.clone(),
            active: layout.id == active,
            indicator_count: WireU64::new(layout.indicators.len() as u64),
        })
        .collect()
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
            APPLY_PRESET_CAPABILITY_ID,
            "Apply a layout preset",
            "Switches the canvas to a named arrangement from the layout registry — the same registry the toolbar picker and the View menu read.",
            generated_schema::<ApplyPresetInput>(),
        ),
        apply_preset,
    )?;
    registry.register(
        descriptor(
            MOVE_PANE_CAPABILITY_ID,
            "Move a chart within the stack",
            "Moves one context chart up or down the column beside the heatmap. The flow pane does not move: its column is the one thing every preset agrees on.",
            generated_schema::<MovePaneInput>(),
        ),
        move_pane,
    )?;
    registry.register(
        descriptor(
            RESIZE_CAPABILITY_ID,
            "Resize the context column",
            "Sets the context column's share of the canvas, held inside the same floor a divider drag is held to.",
            generated_schema::<ResizeInput>(),
        ),
        resize,
    )?;
    registry.register(
        descriptor(
            COLLAPSE_CAPABILITY_ID,
            "Collapse the context column",
            "Puts the context charts away, leaving the rail that brings them back. The width they had is kept, so expanding returns the layout the trader chose.",
            generated_schema::<TabTarget>(),
        ),
        collapse,
    )?;
    registry.register(
        descriptor(
            EXPAND_CAPABILITY_ID,
            "Expand the context column",
            "Brings the context charts back at the width they had before they were collapsed.",
            generated_schema::<TabTarget>(),
        ),
        expand,
    )?;
    registry.register(
        descriptor(
            FOCUS_CAPABILITY_ID,
            "Focus a chart",
            "Moves focus to one pane: the chart the status bar speaks for, and the one an indicator or drawing command lands on.",
            generated_schema::<FocusInput>(),
        ),
        focus,
    )?;
    registry.register(
        descriptor(
            INTERVAL_CAPABILITY_ID,
            "Set a chart's timeframe",
            "Sets one context chart's interval, the same value its own header selector writes.",
            generated_schema::<IntervalInput>(),
        ),
        set_interval,
    )?;
    registry.register(
        tab_descriptor(
            TAB_SWITCH_CAPABILITY_ID,
            "Switch layout tab",
            "Puts one of the workspace's layouts on one pane — the focused pane of the active tab unless `tab_id`/`pane` name another: its indicators replace what that chart shows, and the market's drawings under it come out. Panes on other layouts are untouched. The same call the strip's click and Alt+1..9 make.",
            generated_schema::<SwitchLayoutTabInput>(),
        ),
        tab_switch,
    )?;
    registry.register(
        tab_descriptor(
            TAB_CREATE_CAPABILITY_ID,
            "Create layout tab",
            "Adds an empty layout after the others and puts it on one pane — the focused pane of the active tab, the way the strip's + does. The panes on other layouts are untouched.",
            generated_schema::<CreateLayoutTabInput>(),
        ),
        tab_create,
    )?;
    registry.register(
        tab_descriptor(
            TAB_RENAME_CAPABILITY_ID,
            "Rename layout tab",
            "Renames one layout. Names are unique within the workspace and bounded to what a tab can show.",
            generated_schema::<RenameLayoutTabInput>(),
        ),
        tab_rename,
    )?;
    // `layout.tab.delete` is deliberately not registered. Deleting a layout
    // destroys its indicator set and every drawing kept under it, and no
    // effect policy in the control contract allows a destructive capability
    // yet (`contract.rs`, `allows_destructive: false` on all three). The
    // trader deletes from the strip or the View menu; the operator gets the
    // call the day the contract grows a confirmed-destructive effect.
    Ok(())
}

fn tab_descriptor(
    id: &str,
    title: &str,
    description: &str,
    input_schema: Value,
) -> CapabilityDescriptor {
    let mut descriptor = descriptor(id, title, description, input_schema);
    descriptor.output_schema = generated_schema::<LayoutTabResult>();
    descriptor.stale_input_safety = Some(
        "Switching, creating or renaming a layout removes no work: every layout keeps its indicators and drawings while another is showing. A stale caller can only show the wrong layout, which the result it gets back names."
            .to_owned(),
    );
    descriptor
}

/// `subject` is the layout the call acted on, when it named a pane; `None`
/// answers about the focused pane, which is what a rename or a delete moved
/// nothing away from.
fn tab_result(
    app: &QuantickApp,
    changed: bool,
    subject: Option<crate::layouts::LayoutId>,
) -> Result<Value, ControlError> {
    let subject = subject.unwrap_or_else(|| app.focused_pane_layout());
    let active = app
        .layouts()
        .get(subject)
        .unwrap_or_else(|| app.layouts().active());
    let payload = LayoutTabResult {
        active_layout_id: WireU64::new(active.id.0),
        active_layout_name: active.name.clone(),
        layouts: layout_tabs_marking(app, active.id),
        changed,
    };
    serde_json::to_value(payload).map_err(|error| {
        ControlError::invalid_request(format!(
            "the layout tab result could not be encoded: {error}"
        ))
    })
}

fn resolve_layout_tab(
    app: &QuantickApp,
    target: &LayoutTabTarget,
) -> Result<crate::layouts::LayoutId, ControlError> {
    let by_id = target
        .layout_id
        .map(|id| crate::layouts::LayoutId(id.get()))
        .map(|id| {
            app.layouts()
                .get(id)
                .map(|layout| layout.id)
                .ok_or_else(|| ControlError::invalid_request(format!("no layout has id {}", id.0)))
        })
        .transpose()?;
    let by_name = target
        .name
        .as_deref()
        .map(|name| {
            app.layouts()
                .by_name(name)
                .map(|layout| layout.id)
                .ok_or_else(|| {
                    ControlError::invalid_request(format!("no layout is called {name:?}"))
                })
        })
        .transpose()?;
    match (by_id, by_name) {
        (Some(id), Some(named)) if id != named => Err(ControlError::invalid_request(
            "layout_id and name name different layouts",
        )),
        (Some(id), _) | (None, Some(id)) => Ok(id),
        // Omitted: the layout the focused pane shows — the one the strip
        // lights, never the book's own default.
        (None, None) => Ok(app.focused_pane_layout()),
    }
}

fn layout_error(error: crate::layouts::LayoutError) -> ControlError {
    ControlError::invalid_request(error.to_string())
}

fn tab_switch(
    app: &mut QuantickApp,
    _access: &mut ControlAccess,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: SwitchLayoutTabInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let id = resolve_layout_tab(app, &input.layout)?;
    let index = tab_index(
        app,
        TabTarget {
            tab_id: input.tab_id,
        },
    )?;
    let tab = app
        .control_tab_at(index)
        .ok_or_else(|| ControlError::invalid_request("the tab closed while the call ran"))?;
    let side = match input.pane {
        Some(pane) => {
            let pane = pane.get() as usize;
            if tab.pane_at(pane).is_none() {
                return Err(ControlError::invalid_request(format!(
                    "this tab has no pane at address {pane}"
                )));
            }
            crate::pane::PaneSide::from_index(pane)
        }
        None => tab.focused_side(),
    };
    let tab_id = tab.id;
    let changed = app
        .switch_pane_layout(tab_id, side, id)
        .map_err(layout_error)?;
    tab_result(app, changed, Some(id))
}

fn tab_create(
    app: &mut QuantickApp,
    _access: &mut ControlAccess,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: CreateLayoutTabInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let id = app
        .create_layout(input.name.as_deref())
        .map_err(layout_error)?;
    tab_result(app, true, Some(id))
}

fn tab_rename(
    app: &mut QuantickApp,
    _access: &mut ControlAccess,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: RenameLayoutTabInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let id = resolve_layout_tab(app, &input.target)?;
    let changed = app
        .rename_layout(id, &input.new_name)
        .map_err(layout_error)?;
    tab_result(app, changed, None)
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
    let tab_id = app
        .control_tab_at(index)
        .ok_or_else(|| ControlError::invalid_request("the tab closed while the call ran"))?
        .id;
    // The one reposition path — the same call the View menu takes, which
    // moves the slot bookkeeping and the drawing keys with the pane.
    let changed = app.move_context_pane_at(tab_id, from, to);
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
    let asked = crate::state::BarSpec::Time(input.interval_ms);
    let changed = chart.spec.retained(crate::state::BarKind::Time) != &asked;
    chart.spec.set(asked);
    result(app, index, changed)
}
