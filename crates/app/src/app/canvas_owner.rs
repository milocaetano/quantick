//! Lend the canvas family its tab, saved workspace and presentation owners.
use quantick_control::error::ControlError;
use quantick_control_handlers::canvas::{self, CanvasAct, CanvasHost, Drawn, owner::CanvasOwner};
use quantick_control_schema::canvas::LayoutPickerResult;
use quantick_workspace::canvas::{CanvasRect, CanvasState, LayoutPreset};
use quantick_workspace::canvas_layout::recipe_canvas;

use super::{QuantickApp, chrome::ChromeState, workspace_runtime::WorkspaceRuntime};

struct CanvasAdapter<'a> {
    runtime: &'a mut WorkspaceRuntime,
    config: &'a crate::config::AppConfig,
    surfaces: &'a mut crate::surfaces::Surfaces,
    chrome: &'a mut ChromeState,
}

impl CanvasHost for QuantickApp {
    fn canvas_owner(&mut self) -> Option<Box<dyn CanvasOwner + '_>> {
        Some(Box::new(CanvasAdapter {
            runtime: self.runtime.bound_mut(&self.presentation.binding),
            config: &self.config,
            surfaces: &mut self.surfaces,
            chrome: &mut self.chrome,
        }))
    }
}

impl CanvasOwner for CanvasAdapter<'_> {
    fn state(&self) -> &CanvasState {
        &self.runtime.active_tab().canvas.state
    }
    fn drawn(&self) -> Drawn {
        self.runtime.active_tab().canvas.drawn()
    }
    fn saved(&self) -> Option<CanvasState> {
        self.runtime
            .workspace
            .book
            .saved_recipe(self.runtime.tabs.active_id())
            .map(recipe_canvas)
    }
    fn unsaved(&self) -> bool {
        let tab = self.runtime.tabs.active_id();
        self.runtime
            .workspace
            .book
            .unsaved(tab, super::tab_workspaces::live_recipe(self.runtime, tab))
    }
    fn origin(&self) -> (f32, f32) {
        self.runtime.active_tab().canvas.floats.origin
    }
    fn monitor(&self, rect: CanvasRect) -> Option<String> {
        crate::floating_panes::monitor_of(rect, self.runtime.active_tab().canvas.floats.scale)
    }
    fn pick_layout(&mut self, preset: &'static LayoutPreset) {
        crate::tab::layout::pick(self.runtime.active_tab_mut(), preset);
    }
    fn show_widget(&mut self, id: &'static str) -> Result<(), String> {
        use quantick_control_handlers::widgets::WidgetOwner;
        crate::widget_surfaces::WidgetDesk {
            surfaces: &mut self.surfaces.widgets,
            tabs: &mut self.runtime.tabs,
            reads: (self.config, self.runtime.tz),
        }
        .widget(id, Some(true))
        .map(|_| ())
    }
    fn act(
        &mut self,
        saved: Option<&CanvasState>,
        act: CanvasAct,
    ) -> Result<(bool, Option<u32>), ControlError> {
        let tab = &mut self.runtime.active_tab_mut().canvas;
        let drawn = tab.drawn();
        tab.mirror_split(tab.column_painted());
        let acted = canvas::act(&mut tab.state, saved, drawn, act);
        tab.mirror_split(false);
        acted
    }
    fn open_window_menu(&mut self) -> bool {
        self.chrome.window_menu.open_from_toolbar()
    }
    fn picker_read(&self) -> LayoutPickerResult {
        let (workspace, _, marks) = self
            .runtime
            .workspace
            .book
            .canvas_layout(self.runtime.tabs.active_id(), self.state());
        let popovers = &self.surfaces.canvas_popovers.model_state;
        quantick_ui_model::layout_picker::read(
            &workspace,
            marks,
            popovers.picker(),
            popovers.projection().layout_open,
        )
    }
}
