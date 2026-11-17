//! A tab's panes as the order-flow scopes read them: each pane with its tab
//! and the window's footprint setup, and its view's last published frame.
use super::ChartPane;
use crate::footprint_config::FootprintConfig;
use crate::{control::pane_side_dto, orderflow_view::OrderflowView, tab::Tab};
use quantick_control_handlers::orderflow::{OrderflowEngine, OrderflowPane, OrderflowPaneEntry};
use quantick_control_host::wire::PaneSideDto;
use quantick_control_schema::orderflow::{
    BubblesStateSnapshot, FlowExecutionSnapshot, OpeningScaleSnapshot,
};
use quantick_orderflow::engine::{BookLadder, CaptureStatus, OrderflowHealth};

impl OrderflowEngine for OrderflowView {
    fn config(&self) -> &quantick_orderflow::HeatmapConfig {
        self.cached_config()
    }
    fn book(
        &self,
    ) -> (
        &CaptureStatus,
        Option<&BookLadder>,
        rust_decimal::Decimal,
        &[quantick_orderflow::projection::GapPrimitive],
    ) {
        self.cached_book()
    }
    fn health(&self) -> &OrderflowHealth {
        self.cached_health()
    }
    fn live_end_ms(&self) -> Option<i64> {
        self.cached_live_end_ms()
    }
    fn book_held_at_ms(&self) -> Option<i64> {
        OrderflowView::book_held_at_ms(self)
    }
    fn bubbles(&self) -> BubblesStateSnapshot {
        self.bubbles_snapshot()
    }
    fn flow_execution(&self) -> Option<FlowExecutionSnapshot> {
        self.flow_execution_snapshot_frame()
            .map(|frame| (frame.as_ref(), self.flow_execution_progress()).into())
    }
}

/// One pane read with its tab and the window's footprint setup.
struct OrderflowPaneRead<'a> {
    tab: &'a Tab,
    pane: &'a ChartPane,
    window: &'a FootprintConfig,
}

impl OrderflowPane for OrderflowPaneRead<'_> {
    fn pane_id(&self) -> u64 {
        self.pane.id
    }
    fn footprint_visible(&self) -> bool {
        self.pane.presentation.footprint.visible
    }
    fn footprint_overridden(&self) -> bool {
        self.pane.presentation.footprint.config.is_own()
    }
    fn footprint_config(&self) -> &FootprintConfig {
        self.pane.footprint_config(self.window)
    }
    fn opening_scale(&self) -> OpeningScaleSnapshot {
        let view = self.pane.orderflow.as_ref();
        OpeningScaleSnapshot {
            tape: view.map(|v| v.cached_config().volume_dots.ignore_opening_burst_in_scale),
            candle: view.is_some_and(OrderflowView::ignore_flow_opening),
        }
    }
    fn tape_age_at(&self, now_ms: i64) -> Option<i64> {
        self.tab.tape_age_at(now_ms)
    }
    fn engine(&self) -> Option<&dyn OrderflowEngine> {
        self.pane
            .orderflow
            .as_ref()
            .map(|view| view as &dyn OrderflowEngine)
    }
}

/// Hand `visit` the tab's panes in its order, each read against `window`.
pub(crate) fn visit_orderflow_panes(
    tab: &Tab,
    window: &FootprintConfig,
    visit: impl FnOnce(&[OrderflowPaneEntry<'_>]),
) {
    let panes: Vec<(PaneSideDto, OrderflowPaneRead<'_>)> = tab
        .panes()
        .map(|(pane, side)| (pane_side_dto(side), OrderflowPaneRead { tab, pane, window }))
        .collect();
    let entries: Vec<OrderflowPaneEntry<'_>> = panes
        .iter()
        .map(|(side, pane)| OrderflowPaneEntry { side: *side, pane })
        .collect();
    visit(&entries);
}
