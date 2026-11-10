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
