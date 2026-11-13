//! The window's adapter onto `quantick_control_handlers::layout`: the layout
//! calls validate and answer there, and every act below is the one the
//! window's own strip, divider, rail, menu or header makes.

use crate::app::control_host::ControlPort;
use crate::app::{LayoutPort, TabsPort};
use crate::pane::PaneSide;
use crate::tab::Tab;
pub(crate) use quantick_control_schema::layout::*;

use quantick_chart::state::{BarConfiguration, SpecSelector};
use quantick_control::{error::ControlError, wire::ActorKind};
use quantick_control_handlers::layout::{CanvasReading, LayoutHost, LayoutTab};
use quantick_workspace::canvas_layout::LayoutPreset;
use quantick_workspace::layout_document::{LayoutBook, LayoutError, LayoutId};

/// The strip as `observe.workspace` reports it: the focused pane's layout lit.
pub(crate) fn layout_tabs<P: LayoutPort + ?Sized>(app: &P) -> Vec<LayoutTabSnapshot> {
    let state = app.layout_state();
    let active = state.focused_pane_layout();
    quantick_control_handlers::layout::layout_tabs(state.layouts(), active, |id| {
        state.shown_count(id)
    })
}

/// Which tab a call names, as an index into the open strip.
pub(crate) fn tab_index<P: TabsPort + ?Sized>(
    app: &P,
    target: TabTarget,
) -> Result<usize, ControlError> {
    quantick_control_handlers::tab_index(&app.tab_reads(), target.tab_id)
}

impl LayoutHost for dyn ControlPort {
    fn layout_book(&self) -> &LayoutBook {
        self.layout_state().layouts()
    }
    fn focused_pane_layout(&self) -> LayoutId {
        self.layout_state().focused_pane_layout()
    }
    fn shown_count(&self, layout: LayoutId) -> usize {
        self.layout_state().shown_count(layout)
    }
    fn switch_pane_layout(
        &mut self,
        tab_id: u64,
        side: PaneSide,
        layout: LayoutId,
    ) -> Result<bool, LayoutError> {
        self.layout_adapter()
            .switch_pane_layout(tab_id, side, layout)
    }
    fn create_layout(&mut self, name: Option<&str>) -> Result<LayoutId, LayoutError> {
        self.layout_adapter().create_layout(name)
    }
    fn rename_layout(&mut self, layout: LayoutId, name: &str) -> Result<bool, LayoutError> {
        self.layout_adapter().rename_layout(layout, name)
    }
    fn move_context_pane(&mut self, tab_id: u64, from: usize, to: usize) -> bool {
        self.layout_adapter().move_context_pane_at(tab_id, from, to)
    }
    fn layout_tab(&mut self, index: usize) -> Option<&mut dyn LayoutTab> {
        Some(self.tabs_mut().tab_at_mut(index)?)
    }
}

impl LayoutTab for Tab {
    fn reading(&self) -> CanvasReading {
        let (collapsed, flow_collapsed) = self.canvas.folds();
        CanvasReading {
            preset_id: self.canvas.layout.preset().id,
            pane_count: self.pane_count(),
            focused_pane: self.focused_side().index(),
            fraction: self.canvas.split_fraction,
            collapsed,
            flow_collapsed,
        }
    }
    fn has_pane(&self, pane: usize) -> bool {
        self.pane_at(pane).is_some()
    }
    fn focused_side(&self) -> PaneSide {
        Tab::focused_side(self)
    }
    fn last_canvas_width(&self) -> f32 {
        Tab::last_canvas_width(self)
    }
    fn pick_preset(&mut self, preset: &'static LayoutPreset) -> bool {
        crate::tab::layout::pick(self, preset)
    }
    fn set_split_fraction(&mut self, fraction: f32) -> bool {
        let changed = (self.canvas.split_fraction - fraction).abs() > f32::EPSILON;
        self.canvas.split_fraction = fraction;
        self.canvas.mirror_split(true);
        changed
    }
    fn set_context_collapsed(&mut self, collapsed: bool) -> bool {
        self.canvas.fold(false, collapsed)
    }
    fn flow_has_rail(&self) -> bool {
        self.canvas.layout.shows_flow() && self.canvas.layout.shows_time()
    }
    fn set_flow_collapsed(&mut self, collapsed: bool) -> bool {
        self.canvas.fold(true, collapsed)
    }
    fn shows(&self, side: PaneSide) -> bool {
        self.drawn_panes().shows(side)
    }
    fn focus(&mut self, side: PaneSide) -> bool {
        std::mem::replace(&mut self.focus, side) != side
    }
    fn bars_mut(&mut self, pane: usize) -> Option<&mut SpecSelector> {
        self.pane_at_mut(pane).map(|chart| &mut chart.spec)
    }
    fn set_pane_bar_spec(&mut self, pane: usize, spec: BarConfiguration) -> Result<bool, String> {
        Tab::set_pane_bar_spec(self, pane, spec).map_err(|error| error.to_string())
    }
    fn resize_context_pair(
        &mut self,
        tab_id: u64,
        (upper_pane_id, lower_pane_id): (u64, u64),
        fraction: f32,
        actor: ActorKind,
    ) -> Option<Result<(f32, bool), String>> {
        let (column, _) = self.canvas.context_stack_geometry(&self.time_panes)?;
        let request = crate::tab::context_resize::ResizeContextPair {
            upper_pane_id,
            lower_pane_id,
            wanted_y: eframe::egui::lerp(column.y_range(), fraction),
        };
        let result = self
            .canvas
            .resize_context_pair(tab_id, request, actor, &self.time_panes);
        Some(
            result
                .map(|done| (done.fraction, done.changed))
                .map_err(|error| error.to_string()),
        )
    }
}
