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
    self, CanvasRect, CanvasRefusal, CanvasState, Corner, DropOutcome, Edge, FloatGesture,
    FloatHost, LayoutPreset, PaneKind, PaneMenuItem, PaneMode, RemoveAnswer, WidgetSpec,
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

/// Where a chart popped out without a monitor named goes. Always present,
/// so a window saved on it never falls back to the canvas.
pub const PRIMARY_MONITOR: &str = "primary";

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
        && saved.is_some_and(|saved| canvas::remove_needs_confirm(saved, state, slot))
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
            Some(WidgetSpec {
                kind: Some(kind), ..
            }) => state.add(*kind, area).map(|slot| (true, Some(slot))),
            Some(_) => {
                return Err(ControlError::invalid_request(format!(
                    "{widget_id} opens in a window of its own, which this host does not draw"
                )));
            }
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
/// canvas no longer holds, or with no save behind it, is dropped.
#[must_use]
pub fn menu_step(
    picked: Option<(u32, PaneMenuItem)>,
    escape: bool,
    removing: &mut Option<u32>,
    state: &CanvasState,
    saved: Option<&CanvasState>,
) -> Option<CanvasAct> {
    if removing.is_some_and(|slot| !state.holds(slot) || saved.is_none()) {
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
        PaneMenuItem::OpenInNewWindow => CanvasAct::PopOut {
            slot,
            monitor: None,
            rect: None,
        },
        PaneMenuItem::ReturnToCanvas => CanvasAct::Return { slot },
        PaneMenuItem::Remove
            if saved.is_some_and(|saved| canvas::remove_needs_confirm(saved, state, slot)) =>
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
        PaneMenuItem::OpenInNewWindow => POP_OUT_CAPABILITY_ID,
        PaneMenuItem::ReturnToCanvas => RETURN_CAPABILITY_ID,
        PaneMenuItem::Remove => REMOVE_CAPABILITY_ID,
    }
}

fn dto(rect: CanvasRect) -> CanvasRectDto {
    CanvasRectDto {
        x: rect.x,
        y: rect.y,
        w: rect.w,
        h: rect.h,
    }
}

/// The act letting go of a held floating pane takes: `outcome` is the
/// canvas model's drop rule's answer. A pop-out's rectangle is in window
/// points; it moves by the window's desktop `origin`, and `monitor` names
/// the monitor holding it there.
#[must_use]
pub fn drop_act(
    slot: u32,
    outcome: DropOutcome,
    (x, y): (f32, f32),
    monitor: impl Fn(CanvasRect) -> Option<String>,
) -> CanvasAct {
    match outcome {
        DropOutcome::Dock(edge) => CanvasAct::Dock {
            slot,
            edge: match edge {
                Edge::Left => CanvasEdge::Left,
                Edge::Right => CanvasEdge::Right,
                Edge::Top => CanvasEdge::Top,
                Edge::Bottom => CanvasEdge::Bottom,
            },
        },
        DropOutcome::Park(corner) => CanvasAct::Park {
            slot,
            corner: match corner {
                Corner::TopLeft => CanvasCorner::TopLeft,
                Corner::TopRight => CanvasCorner::TopRight,
                Corner::BottomLeft => CanvasCorner::BottomLeft,
                Corner::BottomRight => CanvasCorner::BottomRight,
            },
        },
        DropOutcome::Float(rect) => CanvasAct::Float {
            slot,
            rect: Some(dto(rect)),
        },
        DropOutcome::PopOut(rect) => {
            let rect = CanvasRect::new(rect.x + x, rect.y + y, rect.w, rect.h);
            CanvasAct::PopOut {
                slot,
                monitor: monitor(rect),
                rect: Some(dto(rect)),
            }
        }
    }
}

/// What a chart's own window asks for: closing it returns the chart to the
/// canvas; moving or resizing it keeps it out, at its new place, on the
/// monitor that now holds it.
#[must_use]
pub fn window_act(
    slot: u32,
    (moved, closed): (Option<(CanvasRect, Option<String>)>, bool),
) -> Option<CanvasAct> {
    if closed {
        return Some(CanvasAct::Return { slot });
    }
    let (rect, monitor) = moved?;
    let rect = Some(dto(rect));
    Some(CanvasAct::PopOut {
        slot,
        monitor,
        rect,
    })
}

/// Whether a chart's window now standing `at` has left `saved` by a point
/// or more on any side.
#[must_use]
pub fn moved(at: CanvasRect, saved: CanvasRect) -> bool {
    let deltas = [
        at.x - saved.x,
        at.y - saved.y,
        at.w - saved.w,
        at.h - saved.h,
    ];
    deltas.iter().any(|delta| delta.abs() >= 1.0)
}

/// A chart window seen `at`, last seen at `seen`: where it settled, once it
/// stood still for a frame away from `saved`, so a drag saves once at its
/// end rather than every frame of it.
pub fn settled(at: CanvasRect, seen: &mut CanvasRect, saved: CanvasRect) -> Option<CanvasRect> {
    let still = std::mem::replace(seen, at) == at;
    (still && moved(at, saved)).then_some(at)
}

/// A pop-out that named no place opens where the chart stands now, moved by
/// the window's desktop `origin`; every other act passes through.
#[must_use]
pub fn placed_pop_out(
    act: CanvasAct,
    (state, area): (&CanvasState, CanvasRect),
    (x, y): (f32, f32),
    monitor_of: impl Fn(CanvasRect) -> Option<String>,
) -> CanvasAct {
    let CanvasAct::PopOut {
        slot,
        monitor,
        rect: None,
    } = act
    else {
        return act;
    };
    let floating = state.host(slot).and_then(|host| match host {
        FloatHost::Canvas { rect } => Some(*rect),
        FloatHost::OsWindow { .. } => None,
    });
    let docked = state
        .rects(area)
        .into_iter()
        .find(|(held, _)| *held == slot);
    let rect = floating
        .or(docked.map(|(_, rect)| rect))
        .map(|rect| CanvasRect::new(rect.x + x, rect.y + y, rect.w, rect.h));
    let monitor = monitor.or_else(|| rect.and_then(&monitor_of));
    CanvasAct::PopOut {
        slot,
        monitor,
        rect: rect.map(dto),
    }
}

/// The canvas's drop targets while a chart floats over it: each edge band
/// docks, each corner parks. Answers each target's scene id, label, place
/// and the capability that does what a drop there does.
#[must_use]
pub fn drop_targets(
    state: &CanvasState,
    area: CanvasRect,
) -> Vec<(String, String, CanvasRect, &'static str)> {
    let over = |pane: &canvas::FloatingPane| matches!(pane.host, FloatHost::Canvas { .. });
    if !state.floating().iter().any(over) {
        return Vec::new();
    }
    let (band, corner) = (canvas::DOCK_EDGE_BAND_PX, canvas::PARK_CORNER_PX);
    let (right, bottom) = (area.right() - band, area.bottom() - band);
    let (far_x, far_y) = (area.right() - corner, area.bottom() - corner);
    let rect = |x, y, w, h| CanvasRect::new(x, y, w, h);
    let docks = [
        ("left", rect(area.x, area.y, band, area.h)),
        ("right", rect(right, area.y, band, area.h)),
        ("top", rect(area.x, area.y, area.w, band)),
        ("bottom", rect(area.x, bottom, area.w, band)),
    ];
    let parks = [
        ("top_left", rect(area.x, area.y, corner, corner)),
        ("top_right", rect(far_x, area.y, corner, corner)),
        ("bottom_left", rect(area.x, far_y, corner, corner)),
        ("bottom_right", rect(far_x, far_y, corner, corner)),
    ];
    let named = |verb: &str, words: &str, at: &str| {
        (
            format!("canvas.{verb}.{at}"),
            format!("{words} {}", at.replace('_', " ")),
        )
    };
    let docks = docks
        .into_iter()
        .map(|(at, place)| (named("dock", "Dock at", at), place, DOCK_CAPABILITY_ID));
    let parks = parks
        .into_iter()
        .map(|(at, place)| (named("park", "Park in", at), place, PARK_CAPABILITY_ID));
    (docks.chain(parks))
        .map(|((id, label), place, capability)| (id, label, place, capability))
        .collect()
}

/// The widget surface an add names, which the window opens in a window of
/// its own rather than on the canvas.
#[must_use]
pub fn surface_widget(act: &CanvasAct) -> Option<&'static str> {
    let CanvasAct::AddWidget { widget_id } = act else {
        return None;
    };
    canvas::widget(widget_id)
        .filter(|widget| widget.kind.is_none())
        .map(|widget| widget.id)
}

/// The add menu's capability, and the act its row `widget` takes.
#[must_use]
pub fn add_act(widget: &WidgetSpec) -> (&'static str, CanvasAct) {
    let widget_id = widget.id.to_owned();
    (ADD_WIDGET_CAPABILITY_ID, CanvasAct::AddWidget { widget_id })
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

/// A `QUANTICK_CANVAS_ACTS` value: canvas acts split by `;`, each
/// `add:<widget>`, `float:<slot>`, `dock:<slot>:<edge>`,
/// `park:<slot>:<corner>`, `pop_out:<slot>`, `return:<slot>`,
/// `minimize:<slot>`, `maximize:<slot>`, `restore:<slot>` or
/// `remove:<slot>`, edges and corners as the wire names them. An act that
/// does not parse is left out.
#[must_use]
pub fn hook_acts(value: &str) -> Vec<CanvasAct> {
    fn named<T: DeserializeOwned>(word: &str) -> Option<T> {
        serde_json::from_value(Value::String(word.trim().to_owned())).ok()
    }
    let one = |act: &str| -> Option<CanvasAct> {
        let mut words = act.trim().split(':');
        let verb = words.next()?;
        if verb == "add" {
            let widget_id = words.next()?.trim().to_owned();
            return Some(CanvasAct::AddWidget { widget_id });
        }
        let slot: u32 = words.next()?.trim().parse().ok()?;
        Some(match (verb, words.next()) {
            ("float", None) => CanvasAct::Float { slot, rect: None },
            ("dock", Some(edge)) => CanvasAct::Dock {
                slot,
                edge: named(edge)?,
            },
            ("park", Some(corner)) => CanvasAct::Park {
                slot,
                corner: named(corner)?,
            },
            ("pop_out", None) => CanvasAct::PopOut {
                slot,
                monitor: None,
                rect: None,
            },
            ("return", None) => CanvasAct::Return { slot },
            ("minimize", None) => CanvasAct::Minimize { slot },
            ("maximize", None) => CanvasAct::Maximize { slot },
            ("restore", None) => CanvasAct::Restore { slot },
            ("remove", None) => CanvasAct::Remove {
                slot,
                confirm: true,
            },
            _ => return None,
        })
    };
    value.split(';').filter_map(one).collect()
}

/// A `QUANTICK_CANVAS_HOLD` value, `<slot>:<x>,<y>`, on `state`: once that
/// chart floats over the canvas, its grip held, and the share of the canvas
/// the pointer is pinned at.
#[must_use]
pub fn held_by_hook(state: &CanvasState, value: &str) -> Option<(FloatGesture, (f32, f32))> {
    let (slot, at) = value.split_once(':')?;
    let (x, y) = at.split_once(',')?;
    let share = |value: &str| {
        value
            .trim()
            .parse::<f32>()
            .ok()
            .filter(|v| (0.0..=1.5).contains(v))
    };
    let slot = slot.trim().parse().ok()?;
    let Some(FloatHost::Canvas { rect }) = state.host(slot) else {
        return None;
    };
    let start = canvas::CanvasPoint::new(rect.x + 10.0, rect.y + 10.0);
    let held = FloatGesture::Move {
        slot,
        from: *rect,
        start,
    };
    Some((held, (share(x)?, share(y)?)))
}
