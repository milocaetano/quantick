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
    schema::generated_schema,
    wire::{ActorContext, WireU64},
};

use schemars::JsonSchema;

use serde::{Deserialize, Serialize};

use serde_json::Value;

use crate::{canvas_layout, tab::CanvasLayout};

use super::{
    actions::{ActionRegistry, CAPABILITY_VERSION},
    gateway::ControlAccess,
};

pub(super) mod stack;

mod v2;

#[cfg(test)]
pub(crate) use v2::{LayoutResultV2, ResizeInputV2};

/// The strip as the control plane reports it — one reading for the layout
/// calls and `observe.workspace` alike.
pub(crate) fn layout_tabs<P: LayoutPort + ?Sized>(app: &P) -> Vec<LayoutTabSnapshot> {
    // "Active" on the wire is what the strip lights: the focused pane's
    // layout. Every pane's own is in `workspace.summary`.
    layout_tabs_marking(app, app.layout_state().focused_pane_layout())
}

/// The same reading with `active` on a layout the caller names.
///
/// A call that addressed *another* pane answers about that pane: reporting
/// the focused pane's layout to a client that just switched a background one
/// tells it its call did not land, when it did.
fn layout_tabs_marking<P: LayoutPort + ?Sized>(
    app: &P,
    active: crate::layouts::LayoutId,
) -> Vec<LayoutTabSnapshot> {
    app.layout_state()
        .layouts()
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

pub(crate) fn register(registry: &mut ActionRegistry) -> Result<(), RegistryError> {
    stack::register(registry)?;
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
        descriptor(
            BAR_SPEC_CAPABILITY_ID,
            "Set a chart's bar rule",
            "Sets one pane's complete alternative-bar rule through the same recut path the toolbar and recording popover use.",
            generated_schema::<BarSpecInput>(),
        ),
        set_bar_spec,
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
                .get(id)
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
pub(super) fn tab_index<P: TabsPort + ?Sized>(
    app: &P,
    target: TabTarget,
) -> Result<usize, ControlError> {
    let Some(id) = target.tab_id else {
        return Ok(app.tab_reads().active_tab_index());
    };
    app.tab_reads()
        .tabs()
        .position(id.get())
        .ok_or_else(|| ControlError::invalid_request(format!("no open tab has id {}", id.get())))
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
    let layout = CanvasLayout::from_preset(preset).ok_or_else(|| {
        ControlError::invalid_request(format!(
            "the canvas cannot draw the preset {} yet",
            preset.id
        ))
    })?;
    let tab = app
        .tabs_mut()
        .tab_at_mut(index)
        .ok_or_else(|| ControlError::invalid_request("the tab closed while the call ran"))?;
    let changed = tab.layout != layout;
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
    // The one reposition path — the same call the View menu takes, which
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
    let changed = (tab.split_fraction - asked).abs() > f32::EPSILON;
    tab.split_fraction = asked;
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
    let changed = tab.set_context_collapsed(collapsed);
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
            "the flow pane's bars are set by the toolbar's BARS group, not by an interval"
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
