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

use crate::app::{LayoutPort, TabsMutPort, TabsPort};
pub(crate) use quantick_control_schema::layout::*;

use quantick_control::{
    error::ControlError,
    registry::RegistryError,
    wire::{ActorContext, WireU64},
};

use serde_json::Value;

use crate::{canvas_layout, tab::CanvasLayout};

use super::{
    actions::{ActionRegistry, CAPABILITY_VERSION},
    gateway::ControlAccess,
};

pub(super) mod stack;

mod v2;

/// The strip as the control plane reports it — one reading for the layout
/// calls and `observe.workspace` alike.
pub(crate) fn layout_tabs<P: LayoutPort + ?Sized>(app: &P) -> Vec<LayoutTabSnapshot> {
    // "Active" on the wire is what the strip lights: the focused pane's
    // layout. Every pane's own is in `workspace.summary`.
    layout_tabs_marking(app, app.layout_state().focused_pane_layout())
}

/// The same reading with `active` on a layout the caller names: a call that
/// addressed another pane answers about it. Counts are what the focused pane
/// shows, its tab's copy, as the chart does.
fn layout_tabs_marking<P: LayoutPort + ?Sized>(
    app: &P,
    active: crate::layouts::LayoutId,
) -> Vec<LayoutTabSnapshot> {
    let state = app.layout_state();
    state
        .layouts()
        .layouts()
        .iter()
        .map(|layout| LayoutTabSnapshot {
            layout_id: WireU64::new(layout.id.0),
            name: layout.name.clone(),
            active: layout.id == active,
            indicator_count: WireU64::new(state.shown_count(layout.id) as u64),
        })
        .collect()
}

pub(crate) fn register(registry: &mut ActionRegistry) -> Result<(), RegistryError> {
    stack::register(registry)?;
    registry.register(apply_preset_descriptor(), apply_preset)?;
    registry.register(move_pane_descriptor(), move_pane)?;
    registry.register(resize_descriptor(), resize)?;
    registry.register(collapse_descriptor(), collapse)?;
    registry.register(expand_descriptor(), expand)?;
    registry.register(flow_collapse_descriptor(), collapse_flow)?;
    registry.register(flow_expand_descriptor(), expand_flow)?;
    registry.register(focus_descriptor(), focus)?;
    registry.register(interval_descriptor(), set_interval)?;
    registry.register(bar_spec_descriptor(), set_bar_spec)?;
    registry.register(tab_switch_descriptor(), tab_switch)?;
    registry.register(tab_create_descriptor(), tab_create)?;
    registry.register(tab_rename_descriptor(), tab_rename)?;
    // `layout.tab.delete` is deliberately not registered: it destroys an
    // indicator set, and no effect policy allows a destructive capability yet
    // (`contract.rs`, `allows_destructive: false`); the strip and menu delete.
    v2::register(registry)
}

/// `subject` is the layout the call acted on, when it named a pane; `None`
/// answers about the focused pane, which is what a rename or a delete moved
/// nothing away from.
fn tab_result<P: LayoutPort + ?Sized>(
    app: &P,
    changed: bool,
    subject: Option<crate::layouts::LayoutId>,
) -> Result<Value, ControlError> {
    let subject = subject.unwrap_or_else(|| app.layout_state().focused_pane_layout());
    let active = app
        .layout_state()
        .layouts()
        .get(subject)
        .unwrap_or_else(|| app.layout_state().layouts().active());
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

fn resolve_layout_tab<P: LayoutPort + ?Sized>(
    app: &P,
    target: &LayoutTabTarget,
) -> Result<crate::layouts::LayoutId, ControlError> {
    let by_id = target
        .layout_id
        .map(|id| crate::layouts::LayoutId(id.get()))
        .map(|id| {
            app.layout_state()
                .layouts()
                .on_strip(id)
                .map(|layout| layout.id)
                .ok_or_else(|| ControlError::invalid_request(format!("no layout has id {}", id.0)))
        })
        .transpose()?;
    let by_name = target
        .name
        .as_deref()
        .map(|name| {
            app.layout_state()
                .layouts()
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
        (None, None) => Ok(app.layout_state().focused_pane_layout()),
    }
}

fn layout_error(error: crate::layouts::LayoutError) -> ControlError {
    ControlError::invalid_request(error.to_string())
}

fn tab_switch<P: TabsPort + LayoutPort + ?Sized>(
    app: &mut P,
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
        .tab_reads()
        .tab_at(index)
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
    let tab_id = app.tab_reads().tabs().id_at(index);
    let changed = app
        .layout_adapter()
        .switch_pane_layout(tab_id, side, id)
        .map_err(layout_error)?;
    tab_result(app, changed, Some(id))
}

fn tab_create<P: LayoutPort + ?Sized>(
    app: &mut P,
    _access: &mut ControlAccess,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: CreateLayoutTabInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let id = app
        .layout_adapter()
        .create_layout(input.name.as_deref())
        .map_err(layout_error)?;
    tab_result(app, true, Some(id))
}

fn tab_rename<P: LayoutPort + ?Sized>(
    app: &mut P,
    _access: &mut ControlAccess,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: RenameLayoutTabInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let id = resolve_layout_tab(app, &input.target)?;
    let changed = app
        .layout_adapter()
        .rename_layout(id, &input.new_name)
        .map_err(layout_error)?;
    tab_result(app, changed, None)
}

/// Which tab a call names, as an index into the open strip.
///
/// A tab id that no longer exists is refused rather than resolved to the
/// active one: a caller that named a tab meant that tab, and quietly acting on
/// a different market is the worst answer available.
pub(crate) fn tab_index<P: TabsPort + ?Sized>(
    app: &P,
    target: TabTarget,
) -> Result<usize, ControlError> {
    quantick_control_handlers::tab_index(&app.tab_reads(), target.tab_id)
}

fn result<P: TabsPort + ?Sized>(
    app: &P,
    index: usize,
    changed: bool,
) -> Result<Value, ControlError> {
    let tab = app
        .tab_reads()
        .tab_at(index)
        .ok_or_else(|| ControlError::invalid_request("the tab closed while the call ran"))?;
    let focused = tab.focused_side().index() as u64;
    let payload = LayoutResult {
        tab_id: WireU64::new(app.tab_reads().tabs().id_at(index)),
        preset_id: tab.canvas.layout.preset().id.to_owned(),
        pane_count: WireU64::new(tab.pane_count() as u64),
        focused_pane: WireU64::new(focused),
        fraction: f64::from(tab.canvas.split_fraction),
        collapsed: tab.canvas.context_collapsed,
        flow_collapsed: tab.canvas.flow_collapsed,
        changed,
    };
    serde_json::to_value(payload).map_err(|error| {
        ControlError::invalid_request(format!("the layout result could not be encoded: {error}"))
    })
}

fn apply_preset<P: TabsPort + TabsMutPort + ?Sized>(
    app: &mut P,
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
    let layout = CanvasLayout::from_preset(preset);
    let tab = app
        .tabs_mut()
        .tab_at_mut(index)
        .ok_or_else(|| ControlError::invalid_request("the tab closed while the call ran"))?;
    let changed = tab.canvas.layout != layout;
    tab.set_layout(layout);
    result(app, index, changed)
}

fn move_pane<P: TabsPort + LayoutPort + ?Sized>(
    app: &mut P,
    _access: &mut ControlAccess,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: MovePaneInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let index = tab_index(app, input.target)?;
    let (from, to) = (input.from.get() as usize, input.to.get() as usize);
    app.tab_reads()
        .tab_at(index)
        .ok_or_else(|| ControlError::invalid_request("the tab closed while the call ran"))?;
    let tab_id = app.tab_reads().tabs().id_at(index);
    // The one reposition path — the same call the window menu takes, which
    // moves the slot bookkeeping and the drawing keys with the pane.
    let changed = app.layout_adapter().move_context_pane_at(tab_id, from, to);
    result(app, index, changed)
}

fn resize<P: TabsPort + TabsMutPort + ?Sized>(
    app: &mut P,
    _access: &mut ControlAccess,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: ResizeInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let index = tab_index(app, input.target)?;
    let tab = app
        .tabs_mut()
        .tab_at_mut(index)
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
    let changed = (tab.canvas.split_fraction - asked).abs() > f32::EPSILON;
    tab.canvas.split_fraction = asked;
    result(app, index, changed)
}

fn collapse<P: TabsPort + TabsMutPort + ?Sized>(
    app: &mut P,
    _access: &mut ControlAccess,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    set_collapsed(app, input, true)
}

fn expand<P: TabsPort + TabsMutPort + ?Sized>(
    app: &mut P,
    _access: &mut ControlAccess,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    set_collapsed(app, input, false)
}

fn set_collapsed<P: TabsPort + TabsMutPort + ?Sized>(
    app: &mut P,
    input: &Value,
    collapsed: bool,
) -> Result<Value, ControlError> {
    let input: TabTarget = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let index = tab_index(app, input)?;
    let tab = app
        .tabs_mut()
        .tab_at_mut(index)
        .ok_or_else(|| ControlError::invalid_request("the tab closed while the call ran"))?;
    // The same call the divider drag, the rail and the menu take.
    let changed = tab.canvas.set_context_collapsed(collapsed);
    result(app, index, changed)
}

fn collapse_flow<P: TabsPort + TabsMutPort + ?Sized>(
    app: &mut P,
    _access: &mut ControlAccess,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    set_flow_collapsed(app, input, true)
}

fn expand_flow<P: TabsPort + TabsMutPort + ?Sized>(
    app: &mut P,
    _access: &mut ControlAccess,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    set_flow_collapsed(app, input, false)
}

fn set_flow_collapsed<P: TabsPort + TabsMutPort + ?Sized>(
    app: &mut P,
    input: &Value,
    collapsed: bool,
) -> Result<Value, ControlError> {
    let input: TabTarget = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let index = tab_index(app, input)?;
    let tab = app
        .tabs_mut()
        .tab_at_mut(index)
        .ok_or_else(|| ControlError::invalid_request("the tab closed while the call ran"))?;
    if !tab.canvas.layout.shows_flow() || !tab.canvas.layout.shows_time() {
        return Err(ControlError::invalid_request(
            "the flow pane has no right rail in this layout",
        ));
    }
    let changed = tab.canvas.set_flow_collapsed(collapsed);
    result(app, index, changed)
}

fn focus<P: TabsPort + TabsMutPort + ?Sized>(
    app: &mut P,
    _access: &mut ControlAccess,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: FocusInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let index = tab_index(app, input.target)?;
    let pane = input.pane.get() as usize;
    let tab = app
        .tabs_mut()
        .tab_at_mut(index)
        .ok_or_else(|| ControlError::invalid_request("the tab closed while the call ran"))?;
    if tab.pane_at(pane).is_none() {
        return Err(ControlError::invalid_request(format!(
            "this tab has no pane at address {pane}"
        )));
    }
    let wanted = crate::pane::PaneSide::from_index(pane);
    // A hidden pane cannot hold focus (`focused_side` would answer another),
    // so saying yes to it would report a change that never happened.
    if !tab.drawn_panes().shows(wanted) {
        return Err(ControlError::invalid_request(format!(
            "the pane at address {pane} is not on screen: expand it before focusing it"
        )));
    }
    let changed = tab.focus != wanted;
    tab.focus = wanted;
    result(app, index, changed)
}

fn set_interval<P: TabsPort + TabsMutPort + ?Sized>(
    app: &mut P,
    _access: &mut ControlAccess,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: IntervalInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let asked = quantick_engine::bar_registry::BUILTIN_BARS
        .find("time")
        .expect("registered time definition")
        .configure(input.interval_ms.into(), None)
        .map_err(|_| {
            ControlError::invalid_request(format!(
                "an interval of {} ms is outside the range a chart accepts",
                input.interval_ms
            ))
        })?;
    let index = tab_index(app, input.target)?;
    let pane = input.pane.get() as usize;
    if pane == 0 {
        return Err(ControlError::invalid_request(
            "the flow pane's bars are set by its header's bars control or layout.pane.set_bar_spec, not by an interval"
                .to_owned(),
        ));
    }
    let tab = app
        .tabs_mut()
        .tab_at_mut(index)
        .ok_or_else(|| ControlError::invalid_request("the tab closed while the call ran"))?;
    let Some(chart) = tab.pane_at_mut(pane) else {
        return Err(ControlError::invalid_request(format!(
            "this tab has no context chart at address {pane}"
        )));
    };
    let changed = chart.spec.retained(crate::state::BarKind::Time) != &asked;
    chart
        .spec
        .update(
            quantick_engine::bar_selection::SelectionCommand::Replace(asked),
            quantick_engine::bar_selection::BarInputAvailability::PRINTS,
        )
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    result(app, index, changed)
}

fn set_bar_spec<P: TabsPort + TabsMutPort + ?Sized>(
    app: &mut P,
    _access: &mut ControlAccess,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: BarSpecInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let spec = quantick_engine::bar_registry::BUILTIN_BARS
        .parse(&input.spec)
        .map_err(|error| ControlError::invalid_request(format!("invalid bar spec: {error}")))?;
    let index = tab_index(app, input.target)?;
    let pane = input.pane.get() as usize;
    let tab = app
        .tabs_mut()
        .tab_at_mut(index)
        .ok_or_else(|| ControlError::invalid_request("the tab closed while the call ran"))?;
    if tab.pane_at(pane).is_none() {
        return Err(ControlError::invalid_request(format!(
            "this tab has no pane at address {pane}"
        )));
    }
    let changed = tab
        .set_pane_bar_spec(pane, spec)
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    result(app, index, changed)
}
