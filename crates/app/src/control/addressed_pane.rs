//! The addressed pane, resolved for the calls that act on it, and the pane,
//! its tape and its marks as `quantick_control_handlers::chart::AddressedPane`
//! acts on them: the state the axis gestures and the context bar change.
use super::layout;
use crate::app::{TabsMutPort, TabsPort};
use crate::orderflow_view::OrderflowView;
use crate::pane::ChartPane;
use quantick_control::{error::ControlError, wire::WireU64};
use quantick_control_handlers::axis_pane::{AxisPane, NativeTape};
use quantick_control_handlers::pane_marks::PaneMarks;
use quantick_control_schema::chart::ViewportSnapshot;
use quantick_control_schema::tape_view::TapeViewSnapshot;
use quantick_drawings::{DrawingTool, Drawings};
use quantick_orderflow::LaneWindow;
use quantick_orderflow::tape_view::TapeEnd;

pub(super) fn invalid(error: impl ToString) -> ControlError {
    ControlError::invalid_request(error.to_string())
}

/// The pane a call names, on the tab it names.
pub(super) fn pane<P: TabsPort + TabsMutPort + ?Sized>(
    app: &mut P,
    tab_id: WireU64,
    pane_id: WireU64,
) -> Result<&mut ChartPane, ControlError> {
    let tab_id = Some(tab_id);
    let index = layout::tab_index(app, layout::TabTarget { tab_id })?;
    let tab = app.tabs_mut().tab_at_mut(index);
    tab.ok_or_else(|| invalid("the tab closed while the call ran"))?
        .panes_mut()
        .find(|pane| pane.id == pane_id.get())
        .ok_or_else(|| invalid("the call names an unknown pane on the requested tab"))
}

impl AxisPane for ChartPane {
    fn frame_price_range(&mut self, low: f64, high: f64) {
        self.sync_price_axis_mode();
        self.price_view.set_manual_range(low, high);
    }
    fn fit_prices(&mut self) {
        self.sync_price_axis_mode();
        self.price_view.reset();
    }
    fn reset_time_strip(&mut self, tape: bool) -> bool {
        self.reset_time_axis(tape)
    }
    fn show_live_overview(&mut self) -> bool {
        self.model.live_overview()
    }
    fn centre_live(&mut self, zoom_in: bool) -> bool {
        self.live_centre(zoom_in)
    }
    fn framing(&self) -> ViewportSnapshot {
        super::chart::viewport_snapshot(self)
    }
    fn native_tape(&mut self) -> Option<&mut dyn NativeTape> {
        let tape = self.orderflow.as_mut();
        Some(tape.filter(|tape| tape.cached_config().native_tape())?)
    }
}

impl PaneMarks for ChartPane {
    fn marks(&mut self) -> &mut Drawings<DrawingTool> {
        &mut self.drawings
    }
    fn marks_bars(&self) -> quantick_engine::bar_registry::BarConfiguration {
        self.layout
            .drawings_spec
            .unwrap_or(*self.series.state.spec())
    }
}

impl NativeTape for OrderflowView {
    fn move_tape_window(&mut self, window: LaneWindow, file: bool) {
        self.move_live_lane_window(window, file);
    }
    fn move_tape_end(&mut self, end: TapeEnd) {
        self.set_tape_end(end);
    }
    fn tape_frame(&self) -> Option<TapeViewSnapshot> {
        self.tape_view_snapshot()
    }
}
