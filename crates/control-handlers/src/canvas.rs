//! The workspace canvas's capabilities over a headless host port.
//!
//! The handler parses and validates each call into one [`CanvasAct`]; the host
//! applies it to the active tab's canvas (the model in
//! `quantick_workspace::canvas`) and answers the canvas as it stands after.
use crate::dock::ActionDock;
use quantick_control::{error::ControlError, registry::RegistryError, wire::ActorContext};
use quantick_control_schema::canvas::{
    ADD_WIDGET_CAPABILITY_ID, AddWidgetInput, CanvasCorner, CanvasEdge, CanvasRectDto,
    CanvasResult, DOCK_CAPABILITY_ID, DockPaneInput, FLOAT_CAPABILITY_ID, FloatPaneInput,
    MAXIMIZE_CAPABILITY_ID, MINIMIZE_CAPABILITY_ID, OPEN_WINDOW_MENU_CAPABILITY_ID,
    PARK_CAPABILITY_ID, PICK_LAYOUT_CAPABILITY_ID, POP_OUT_CAPABILITY_ID, PaneInput, ParkPaneInput,
    PickLayoutInput, PopOutInput, REMOVE_CAPABILITY_ID, RESTORE_CAPABILITY_ID,
    RETURN_CAPABILITY_ID, RemovePaneInput, WindowMenuInput, WindowMenuResult, descriptor,
};
use quantick_control_schema::canvas::{
    CanvasPaneDto, CanvasPaneKind, CanvasPaneMode, CanvasPlaceDto, CanvasView,
};
use quantick_workspace::canvas::{
    self, CanvasRect, CanvasRefusal, CanvasState, Corner, Edge, FloatHost, LayoutPreset, PaneKind,
    PaneMenuItem, PaneMode, RemoveAnswer,
};
use serde::de::DeserializeOwned;
use serde_json::Value;

/// One canvas action, validated, for the host to apply.
#[derive(Clone, Debug, PartialEq)]
pub enum CanvasAct {
    PickLayout {
        preset_id: String,
    },
    Minimize {
        slot: u32,
    },
    Maximize {
        slot: u32,
    },
    Restore {
        slot: u32,
    },
    /// `confirm` stands for the trader's answer to the dialog a saved pane
    /// raises; the host refuses an unconfirmed removal of a saved pane.
    Remove {
        slot: u32,
        confirm: bool,
    },
    AddWidget {
        widget_id: String,
    },
    Float {
        slot: u32,
        rect: Option<CanvasRectDto>,
    },
    Dock {
        slot: u32,
        edge: CanvasEdge,
    },
    Park {
        slot: u32,
        corner: CanvasCorner,
    },
    PopOut {
        slot: u32,
        monitor: Option<String>,
        rect: Option<CanvasRectDto>,
    },
    Return {
        slot: u32,
    },
}

fn unavailable() -> ControlError {
    ControlError::invalid_request("the workspace canvas is unavailable")
}

/// What the window lends the canvas capabilities.
pub trait CanvasHost {
    /// Open the window menu at the toolbar's list door; answers whether it is up.
    fn open_window_menu(&mut self) -> Result<bool, ControlError> {
        Err(unavailable())
    }

    /// Apply `act` to the active tab's canvas and answer the canvas after.
    fn act_on_canvas(&mut self, _act: CanvasAct) -> Result<CanvasResult, ControlError> {
        Err(unavailable())
    }
}

/// Dock every canvas capability. The window menu is presentation and never
/// enters a replay's trace; the canvas edits are traced like layout edits.
pub fn register<H: CanvasHost + ?Sized, A>(
    registry: &mut impl ActionDock<H, A>,
) -> Result<(), RegistryError> {
    let described = |id: &str| descriptor(id).expect("every canvas id is described");
    registry
        .register_outside_replay(described(OPEN_WINDOW_MENU_CAPABILITY_ID), open_window_menu)?;
    registry.register(described(PICK_LAYOUT_CAPABILITY_ID), pick_layout)?;
    registry.register(described(MINIMIZE_CAPABILITY_ID), minimize)?;
    registry.register(described(MAXIMIZE_CAPABILITY_ID), maximize)?;
    registry.register(described(RESTORE_CAPABILITY_ID), restore)?;
    registry.register(described(REMOVE_CAPABILITY_ID), remove)?;
    registry.register(described(ADD_WIDGET_CAPABILITY_ID), add_widget)?;
    registry.register(described(FLOAT_CAPABILITY_ID), float)?;
    registry.register(described(DOCK_CAPABILITY_ID), dock)?;
    registry.register(described(PARK_CAPABILITY_ID), park)?;
    registry.register(described(POP_OUT_CAPABILITY_ID), pop_out)?;
    registry.register(described(RETURN_CAPABILITY_ID), return_to_canvas)
}

fn parse<T: DeserializeOwned>(value: &Value) -> Result<T, ControlError> {
    serde_json::from_value(value.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))
}

fn answer<H: CanvasHost + ?Sized>(app: &mut H, act: CanvasAct) -> Result<Value, ControlError> {
    let result = app.act_on_canvas(act)?;
    serde_json::to_value(result).map_err(|error| ControlError::invalid_request(error.to_string()))
}

fn open_window_menu<H: CanvasHost + ?Sized, A>(
    app: &mut H,
    _: &mut A,
    _: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let _: WindowMenuInput = parse(value)?;
    let open = app.open_window_menu()?;
    serde_json::to_value(WindowMenuResult { open })
        .map_err(|error| ControlError::invalid_request(error.to_string()))
}

fn pick_layout<H: CanvasHost + ?Sized, A>(
    app: &mut H,
    _: &mut A,
    _: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let PickLayoutInput { preset_id } = parse(value)?;
    answer(app, CanvasAct::PickLayout { preset_id })
}

fn minimize<H: CanvasHost + ?Sized, A>(
    app: &mut H,
    _: &mut A,
    _: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let PaneInput { slot } = parse(value)?;
    answer(app, CanvasAct::Minimize { slot })
}

fn maximize<H: CanvasHost + ?Sized, A>(
    app: &mut H,
    _: &mut A,
    _: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let PaneInput { slot } = parse(value)?;
    answer(app, CanvasAct::Maximize { slot })
}

fn restore<H: CanvasHost + ?Sized, A>(
    app: &mut H,
    _: &mut A,
    _: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let PaneInput { slot } = parse(value)?;
    answer(app, CanvasAct::Restore { slot })
}

fn remove<H: CanvasHost + ?Sized, A>(
    app: &mut H,
    _: &mut A,
    _: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let RemovePaneInput { slot, confirm } = parse(value)?;
    answer(app, CanvasAct::Remove { slot, confirm })
}

fn add_widget<H: CanvasHost + ?Sized, A>(
    app: &mut H,
    _: &mut A,
    _: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let AddWidgetInput { widget_id } = parse(value)?;
    answer(app, CanvasAct::AddWidget { widget_id })
}

fn float<H: CanvasHost + ?Sized, A>(
    app: &mut H,
    _: &mut A,
    _: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let FloatPaneInput { slot, rect } = parse(value)?;
    if let Some(rect) = rect {
        check_rect(rect)?;
    }
    answer(app, CanvasAct::Float { slot, rect })
}

fn dock<H: CanvasHost + ?Sized, A>(
    app: &mut H,
    _: &mut A,
    _: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let DockPaneInput { slot, edge } = parse(value)?;
    answer(app, CanvasAct::Dock { slot, edge })
}

fn park<H: CanvasHost + ?Sized, A>(
    app: &mut H,
    _: &mut A,
    _: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let ParkPaneInput { slot, corner } = parse(value)?;
    answer(app, CanvasAct::Park { slot, corner })
}

fn pop_out<H: CanvasHost + ?Sized, A>(
    app: &mut H,
    _: &mut A,
    _: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let PopOutInput {
        slot,
        monitor,
        rect,
    } = parse(value)?;
    if let Some(rect) = rect {
        check_rect(rect)?;
    }
    if monitor
        .as_deref()
        .is_some_and(|name| name.trim().is_empty())
    {
        return Err(ControlError::invalid_request("monitor names no monitor"));
    }
    answer(
        app,
        CanvasAct::PopOut {
            slot,
            monitor,
            rect,
        },
    )
}

fn return_to_canvas<H: CanvasHost + ?Sized, A>(
    app: &mut H,
    _: &mut A,
    _: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let PaneInput { slot } = parse(value)?;
    answer(app, CanvasAct::Return { slot })
}

/// A rectangle a pane can be shown in: finite, with a positive size.
fn check_rect(rect: CanvasRectDto) -> Result<(), ControlError> {
    let finite = [rect.x, rect.y, rect.w, rect.h]
        .iter()
        .all(|v| v.is_finite());
    if finite && rect.w > 0.0 && rect.h > 0.0 {
        Ok(())
    } else {
        Err(ControlError::invalid_request(
            "rect needs finite coordinates and a positive width and height",
        ))
    }
}

/// Where a chart popped out without a monitor named goes.
const PRIMARY_MONITOR: &str = "primary";

fn refused(refusal: &CanvasRefusal) -> ControlError {
    let mut error = ControlError::invalid_request(refusal.to_string());
    error.context.details = Some(serde_json::json!({ "refusal": refusal.code() }));
    error
}

/// The preset a pick names, or the refusal the wire reads.
pub fn picked(preset_id: &str) -> Result<&'static LayoutPreset, ControlError> {
    canvas::preset(preset_id)
        .ok_or_else(|| refused(&CanvasRefusal::UnknownPreset(preset_id.to_owned())))
}

/// Apply `act` to `state`, a canvas of `area`. An unconfirmed removal of a
/// pane the `saved` workspace holds is refused, as the trader's dialog asks
/// first. Answers whether the canvas changed and the slot acted on. A window
/// whose pick lays out more than the model (a split, its painter) applies
/// [`CanvasAct::PickLayout`] itself.
pub fn act(
    state: &mut CanvasState,
    saved: Option<&CanvasState>,
    area: CanvasRect,
    act: CanvasAct,
) -> Result<(bool, Option<u32>), ControlError> {
    if let CanvasAct::Remove {
        slot,
        confirm: false,
    } = act
        && saved.is_some_and(|saved| canvas::remove_needs_confirm(saved, slot))
        && state.holds(slot)
    {
        let mut error = ControlError::invalid_request(format!(
            "the saved workspace holds chart {slot}: remove it with confirm true, as the trader's dialog asks first"
        ));
        error.context.details = Some(serde_json::json!({ "refusal": "canvas.confirm_required" }));
        return Err(error);
    }
    let rect_of = |dto: CanvasRectDto| CanvasRect::new(dto.x, dto.y, dto.w, dto.h);
    let on = |slot: u32| move |done: bool| (done, Some(slot));
    let acted = match act {
        CanvasAct::PickLayout { preset_id } => Ok((state.apply_preset(picked(&preset_id)?), None)),
        CanvasAct::Minimize { slot } => state.minimize(slot).map(on(slot)),
        CanvasAct::Maximize { slot } => state.maximize(slot).map(on(slot)),
        CanvasAct::Restore { slot } => state.restore(slot).map(on(slot)),
        CanvasAct::Remove { slot, .. } => state.remove(slot).map(on(slot)),
        CanvasAct::AddWidget { widget_id } => match canvas::widget(&widget_id) {
            Some(widget) => state.add(widget.kind, area).map(|slot| (true, Some(slot))),
            None => Err(CanvasRefusal::UnknownWidget(widget_id)),
        },
        CanvasAct::Float { slot, rect } => {
            let half = CanvasRect::new(0.0, 0.0, area.w / 2.0, area.h / 2.0);
            let rect = rect.map_or_else(|| half.centred_in(area), rect_of);
            state.float(slot, rect).map(on(slot))
        }
        CanvasAct::Dock { slot, edge } => {
            let edge = match edge {
                CanvasEdge::Left => Edge::Left,
                CanvasEdge::Right => Edge::Right,
                CanvasEdge::Top => Edge::Top,
                CanvasEdge::Bottom => Edge::Bottom,
            };
            state.dock(slot, edge).map(on(slot))
        }
        CanvasAct::Park { slot, corner } => {
            let corner = match corner {
                CanvasCorner::TopLeft => Corner::TopLeft,
                CanvasCorner::TopRight => Corner::TopRight,
                CanvasCorner::BottomLeft => Corner::BottomLeft,
                CanvasCorner::BottomRight => Corner::BottomRight,
            };
            state.park(slot, corner, area).map(on(slot))
        }
        CanvasAct::PopOut {
            slot,
            monitor,
            rect,
        } => {
            let kept = state.floating().iter().find(|pane| pane.slot == slot);
            let kept = kept.map(|pane| match &pane.host {
                FloatHost::Canvas { rect } | FloatHost::OsWindow { rect, .. } => *rect,
            });
            let rect = rect.map(rect_of).or(kept).unwrap_or(area);
            let monitor = monitor.as_deref().unwrap_or(PRIMARY_MONITOR);
            state.pop_out(slot, monitor, rect).map(on(slot))
        }
        CanvasAct::Return { slot } => state.return_to_canvas(slot, area).map(on(slot)),
    };
    acted.map_err(|refusal| refused(&refusal))
}

/// What the window does after its canvas drew: the act `picked`, a ⋯ row
/// or a strip's restore, takes, or, to remove a chart the `saved` workspace
/// holds, the dialog raised into `removing`; with nothing picked and no
/// dialog up, Esc restores a maximized chart. A dialog over a chart the
/// canvas no longer holds is dropped.
#[must_use]
pub fn menu_step(
    picked: Option<(u32, PaneMenuItem)>,
    escape: bool,
    removing: &mut Option<u32>,
    state: &CanvasState,
    saved: Option<&CanvasState>,
) -> Option<CanvasAct> {
    if removing.is_some_and(|slot| !state.holds(slot)) {
        *removing = None;
    }
    let Some((slot, item)) = picked else {
        let maximized = state.maximized().filter(|_| escape && removing.is_none());
        return maximized.map(|slot| CanvasAct::Restore { slot });
    };
    Some(match item {
        PaneMenuItem::Minimize => CanvasAct::Minimize { slot },
        PaneMenuItem::Maximize => CanvasAct::Maximize { slot },
        PaneMenuItem::Restore => CanvasAct::Restore { slot },
        PaneMenuItem::Remove
            if saved.is_some_and(|saved| canvas::remove_needs_confirm(saved, slot)) =>
        {
            *removing = Some(slot);
            return None;
        }
        PaneMenuItem::Remove => CanvasAct::Remove {
            slot,
            confirm: true,
        },
    })
}

/// The capability that does what `item` does.
#[must_use]
pub const fn item_capability(item: PaneMenuItem) -> &'static str {
    match item {
        PaneMenuItem::Minimize => MINIMIZE_CAPABILITY_ID,
        PaneMenuItem::Maximize => MAXIMIZE_CAPABILITY_ID,
        PaneMenuItem::Restore => RESTORE_CAPABILITY_ID,
        PaneMenuItem::Remove => REMOVE_CAPABILITY_ID,
    }
}

/// The capability a dialog answer runs; Cancel runs none.
#[must_use]
pub const fn answer_capability(answer: RemoveAnswer) -> Option<&'static str> {
    match answer {
        RemoveAnswer::MinimizeInstead => Some(MINIMIZE_CAPABILITY_ID),
        RemoveAnswer::Cancel => None,
        RemoveAnswer::Remove => Some(REMOVE_CAPABILITY_ID),
    }
}

/// What the remove dialog's `answer` does to `slot`; Cancel does nothing.
#[must_use]
pub fn confirmed(answer: RemoveAnswer, slot: u32) -> Option<CanvasAct> {
    match answer {
        RemoveAnswer::MinimizeInstead => Some(CanvasAct::Minimize { slot }),
        RemoveAnswer::Cancel => None,
        RemoveAnswer::Remove => Some(CanvasAct::Remove {
            slot,
            confirm: true,
        }),
    }
}

/// The answer to a canvas act: whether it changed, the slot, and the canvas
/// after, with whether its workspace now differs from the save.
#[must_use]
pub fn result(
    state: &CanvasState,
    changed: bool,
    slot: Option<u32>,
    unsaved: bool,
) -> CanvasResult {
    let view = view(state);
    CanvasResult {
        changed,
        slot,
        preset_id: view.preset_id,
        unsaved,
        panes: view.panes,
    }
}

/// One tab's canvas as the wire reads it.
#[must_use]
pub fn view(state: &CanvasState) -> CanvasView {
    let panes = (state.panes().into_iter())
        .map(|(slot, kind, mode)| CanvasPaneDto {
            slot,
            kind: match kind {
                PaneKind::Time => CanvasPaneKind::Time,
                _ => CanvasPaneKind::Flow,
            },
            mode: match mode {
                PaneMode::Docked => CanvasPaneMode::Docked,
                PaneMode::Minimized => CanvasPaneMode::Minimized,
                PaneMode::Maximized => CanvasPaneMode::Maximized,
                PaneMode::Floating => CanvasPaneMode::Floating,
                PaneMode::OsWindow => CanvasPaneMode::OsWindow,
            },
        })
        .collect();
    let floating = (state.floating().iter())
        .map(|pane| {
            let (monitor, rect) = match &pane.host {
                FloatHost::Canvas { rect } => (None, rect),
                FloatHost::OsWindow { monitor, rect } => (Some(monitor.clone()), rect),
            };
            let point = |value: f32| value.round() as i32;
            CanvasPlaceDto {
                slot: pane.slot,
                monitor,
                x: point(rect.x),
                y: point(rect.y),
                w: point(rect.w),
                h: point(rect.h),
            }
        })
        .collect();
    CanvasView {
        preset_id: state.preset().map(|preset| preset.id.to_owned()),
        panes,
        floating,
    }
}
