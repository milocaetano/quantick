//! Native geometry conversion and the control adapter over the retained workspace.
use super::{NativeWindows, QuantickApp, WindowId, present};
use eframe::egui;
use quantick_control::{error::ControlError, wire::WireU64};
use quantick_control_handlers::native_window::NativeWindowHost;
use quantick_control_schema::{
    market_windows::*,
    native_window::{WindowInput, WindowState},
};
use quantick_workspace::arrangement::{TabId, windows::WindowError};

fn invalid(error: WindowError) -> ControlError {
    ControlError::invalid_request(match error {
        WindowError::MissingTab => "market_window.missing_tab",
        WindowError::MissingWindow => "market_window.missing_window",
        WindowError::InvalidPosition => "market_window.invalid_position",
        WindowError::StaleDrag => "market_window.stale_drag",
        WindowError::IdentityExhausted => "market_window.identity_exhausted",
    })
}
fn index(value: WireU64) -> Result<usize, ControlError> {
    usize::try_from(value.get())
        .map_err(|_| ControlError::invalid_request("market_window.invalid_position"))
}
fn drag(native: &NativeWindows) -> DragSnapshot {
    let active = native.gesture.active();
    DragSnapshot {
        pending: active.is_some(),
        cancelled: native.gesture.cancelled(),
        tab_id: active.map(|drag| WireU64::new(drag.tab().0)),
        source_window_id: active.map(|drag| WireU64::new(drag.source().0)),
    }
}
fn target(app: &QuantickApp, id: Option<WireU64>) -> Result<WindowId, ControlError> {
    let id = id.map_or(app.runtime.tabs.windows().focused(), |id| {
        WindowId(id.get())
    });
    app.runtime
        .tabs
        .windows()
        .window(id)
        .ok_or_else(|| invalid(WindowError::MissingWindow))?;
    Ok(id)
}
pub(super) fn observe(native: &mut NativeWindows, ctx: &egui::Context, window: WindowId) {
    let scale = ctx.pixels_per_point();
    let rect = |rect: egui::Rect| {
        let value = PhysicalPlacement {
            x_px: (rect.min.x * scale).round() as i32,
            y_px: (rect.min.y * scale).round() as i32,
            width_px: (rect.width() * scale).round() as u32,
            height_px: (rect.height() * scale).round() as u32,
        };
        (rect.is_finite() && rect.is_positive()).then_some(value)
    };
    ctx.input(|input| {
        let info = input.viewport();
        native.observed.insert(
            window,
            NativeObservation {
                source: "native_viewport".to_owned(),
                outer: info.outer_rect.and_then(rect),
                inner: info.inner_rect.and_then(rect),
                pixels_per_point: quantick_control_host::wire::canonical_f32(scale, 6),
                focused: info.focused,
                minimized: info.minimized,
                maximized: info.maximized,
            },
        );
    });
}

impl NativeWindowHost for QuantickApp {
    fn pin_capture_window(&self, requested: Option<WireU64>) -> Result<WireU64, ControlError> {
        target(self, requested).map(|window| WireU64::new(window.0))
    }
    fn present_capture_window(&mut self, window: WireU64) -> Result<WireU64, ControlError> {
        let window = target(self, Some(window))?;
        let previous = self.runtime.tabs.windows().presented();
        present(self, window);
        Ok(WireU64::new(previous.0))
    }
    fn capture_scene(&self) -> Option<&quantick_control_schema::scene::SceneSnapshot> {
        self.presentation.native.capture_scene.as_ref()
    }
    fn set_capture_scene(&mut self, scene: Option<quantick_control_schema::scene::SceneSnapshot>) {
        self.presentation.native.capture_scene = scene;
    }

    fn request_native_window(&mut self, state: WindowState) -> Result<(), ControlError> {
        self.chrome.title_bar.requests.request(state)
    }
    fn request_targeted_native_window(
        &mut self,
        input: WindowInput,
    ) -> Result<WireU64, ControlError> {
        request_targeted_native_window(self, input)
    }
    fn market_windows(&self) -> WindowsSnapshot {
        market_windows(self)
    }
    fn place_native_window(
        &mut self,
        input: PlacementInput,
    ) -> Result<PlacementResult, ControlError> {
        place_native_window(self, input)
    }
    fn operate_market_tabs(
        &mut self,
        operation: TabOperation,
    ) -> Result<TabOperationResult, ControlError> {
        operate_market_tabs(self, operation)
    }
}

fn request_targeted_native_window(
    app: &mut QuantickApp,
    input: WindowInput,
) -> Result<WireU64, ControlError> {
    let window = target(app, input.window_id)?;
    let focused = app.runtime.tabs.windows().focused();
    present(app, window);
    let outcome = app.chrome.title_bar.requests.request(input.state);
    present(app, focused);
    outcome?;
    Ok(WireU64::new(window.0))
}
fn market_windows(app: &QuantickApp) -> WindowsSnapshot {
    let model = app.runtime.tabs.windows();
    WindowsSnapshot {
        native_drop_targeting: if cfg!(windows) {
            "exposed_same_process_window"
        } else {
            "same_window_only"
        }
        .to_owned(),
        focused_window_id: Some(WireU64::new(model.focused().0)),
        presented_window_id: Some(WireU64::new(model.presented().0)),
        omitted_windows: WireU64::new(model.windows().len().saturating_sub(MAX_WINDOWS) as u64),
        drag: drag(&app.presentation.native),
        windows: model
            .windows()
            .iter()
            .take(MAX_WINDOWS)
            .map(|window| MarketWindowSnapshot {
                window_id: WireU64::new(window.id.0),
                selected_tab_id: WireU64::new(window.selected().0),
                ordered_tab_ids: window
                    .tabs()
                    .iter()
                    .take(MAX_TABS_PER_WINDOW)
                    .map(|id| WireU64::new(id.0))
                    .collect(),
                omitted_tabs: WireU64::new(
                    window.tabs().len().saturating_sub(MAX_TABS_PER_WINDOW) as u64
                ),
                observed: app.presentation.native.observed.get(&window.id).cloned(),
            })
            .collect(),
    }
}
fn place_native_window(
    app: &mut QuantickApp,
    input: PlacementInput,
) -> Result<PlacementResult, ControlError> {
    let placement = input.placement.validate()?;
    let window = target(app, input.window_id)?;
    if app
        .presentation
        .native
        .placements
        .get(&window)
        .is_some_and(|pending| *pending != placement)
    {
        return Err(ControlError::invalid_request(
            "market_window.placement_pending",
        ));
    }
    app.presentation.native.placements.insert(window, placement);
    Ok(PlacementResult {
        window_id: WireU64::new(window.0),
        requested: placement,
    })
}
fn operate_market_tabs(
    app: &mut QuantickApp,
    operation: TabOperation,
) -> Result<TabOperationResult, ControlError> {
    use quantick_workspace::arrangement::window_commands::{
        WindowCommand as Command, WindowCommandError,
    };
    let selected_window = if let TabOperation::Select(input) = &operation {
        Some(WindowId(input.window_id.get()))
    } else {
        None
    };
    let command = match operation {
        TabOperation::CancelDrag => Command::CancelDrag,
        TabOperation::BeginDrag(input) => Command::BeginDrag {
            tab: TabId(input.tab_id.get()),
        },
        TabOperation::Select(input) => Command::Select {
            tab: TabId(input.tab_id.get()),
            window: WindowId(input.window_id.get()),
        },
        TabOperation::Close(input) => Command::Close {
            window: target(app, input.window_id)?,
        },
        TabOperation::Reorder(input) => Command::Reorder {
            tab: TabId(input.tab_id.get()),
            position: index(input.position)?,
        },
        TabOperation::Detach(input) => Command::Detach {
            tab: TabId(input.tab_id.get()),
        },
        TabOperation::Attach(input) => Command::Attach {
            tab: TabId(input.tab_id.get()),
            window: WindowId(input.window_id.get()),
            position: index(input.position)?,
        },
    };
    let outcome = app
        .runtime
        .tabs
        .apply_window_command(command, &mut app.presentation.native.gesture)
        .map_err(|error| match error {
            WindowCommandError::Window(error) => invalid(error),
            error => ControlError::invalid_request(match error {
                WindowCommandError::DragPending => "market_window.drag_pending",
                WindowCommandError::OtherDragPending => "market_window.other_drag_pending",
                WindowCommandError::TabNotOwned => "market_window.tab_not_owned_by_window",
                WindowCommandError::FinalWindow => "market_window.final_window_exit_forbidden",
                WindowCommandError::Window(_) => unreachable!("window error handled above"),
            }),
        })?;
    let focused = app.runtime.tabs.windows().focused();
    if let Some(window) = selected_window {
        present(app, window);
        app.chrome.title_bar.surface.show_market();
    }
    present(app, focused);
    Ok(TabOperationResult {
        changed: outcome.changed,
        tab_id: outcome.tab.map(|id| WireU64::new(id.0)),
        source_window_id: outcome.source.map(|id| WireU64::new(id.0)),
        window_id: outcome.window.map(|id| WireU64::new(id.0)),
        position: outcome.position.map(|value| WireU64::new(value as u64)),
        drag: drag(&app.presentation.native),
    })
}
