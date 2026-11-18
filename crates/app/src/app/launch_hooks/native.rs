//! Native targets for transient scenario commands.
use super::{GatewayPort, OwnersDesk, TabsPort};
use crate::app::menu_bar::MenuCall;
use crate::{
    indicator_worker::IndicatorSource, indicators::state_file::SavedKind, tab::CanvasLayout,
};
use quantick_ui_model::launch_scenario::{self, Op};
use std::path::Path;

pub(super) struct Desk<'a, D>(pub &'a mut D);

impl<D: OwnersDesk + GatewayPort + TabsPort> launch_scenario::Desk for Desk<'_, D> {
    fn flow_address(&mut self) -> (u64, u64) {
        let tabs = self.0.tab_reads().tabs();
        (
            tabs.active_id(),
            tabs.get(tabs.active_index())
                .map(|t| t.flow_pane.id)
                .unwrap_or_default(),
        )
    }
    fn has_layout(&mut self, name: &str) -> bool {
        self.0
            .workspace()
            .layout_state()
            .layouts()
            .by_name(name)
            .is_some()
    }
    fn has_script(&mut self, name: &str) -> bool {
        self.0
            .owners()
            .runtime
            .indicators
            .script_library
            .index_of(name)
            .is_some()
    }
    fn canvas_layout(&mut self) -> CanvasLayout {
        self.0.owners().runtime.active_tab().canvas.layout
    }
    fn workspace_menu(&mut self, value: &str) -> bool {
        let owners = self.0.owners();
        owners
            .chrome
            .window_menu
            .tab_workspace
            .stage(owners.runtime.tabs.active_id(), value)
    }
    fn apply(&mut self, op: Op) {
        let desk = &mut *self.0;
        let owners = desk.owners();
        let runtime = owners.runtime;
        let tz = runtime.tz;
        let tab = runtime.active_tab_mut();
        match op {
            Op::Book => tab.tape_mut().set_depth_visible(true),
            Op::LiveStrip => {
                tab.flow_pane.set_layer_visible(
                    crate::chart_layers::ChartLayer::LiveStrip,
                    true,
                    &mut Default::default(),
                );
            }
            Op::Bubbles => tab
                .tape_mut()
                .hold_for_run(|held| held.bubbles = Some(true)),
            Op::Invert => {
                for pane in tab.panes_mut() {
                    pane.price_view.set_inverted(true);
                }
            }
            Op::LaneDepth(on) => tab.tape_mut().set_lane_depth_visible(on),
            Op::LaneBubbles(on) => tab.tape_mut().set_lane_bubbles_enabled(on),
            Op::Window(window) => tab
                .tape_mut()
                .hold_for_run(|held| held.window = Some(window)),
            Op::Footprint => tab.flow_pane.presentation.footprint.visible = true,
            Op::Style(style) => runtime.footprint_config.style = style,
            Op::CandleWidth(px) => tab.flow_pane.model.viewport.open_at_px_per_bar(px),
            Op::Budget(n) => {
                for tab in runtime.tabs.iter_mut() {
                    tab.tape_mut().set_primitive_budget(n);
                }
            }
            Op::Starve(ms) => {
                for tab in runtime.tabs.iter_mut() {
                    tab.tape_mut().set_starve_tape_after_ms(ms);
                }
            }
            Op::Native(id) => {
                tab.flow_pane.add_indicator(IndicatorSource::Native {
                    id: id.to_owned(),
                    values: Vec::new(),
                });
            }
            Op::CollapseLegend => {
                crate::app::indicator_manager::IndicatorState::set_legend_collapsed(
                    tab.focused_pane_mut(),
                    true,
                );
            }
            Op::Call((id, input)) => {
                desk.run_hook_action(id, input, crate::control::ActionOrigin::LaunchHook);
            }
            Op::PaneLayouts(names) => desk.layouts().apply_pane_layouts_hook(&names),
            Op::RenameLayout => {
                let active = runtime.layout_state().focused_pane_layout();
                desk.layouts().begin_layout_rename(active);
            }
            Op::DeleteLayout => {
                let active = runtime.layout_state().focused_pane_layout();
                desk.layouts()
                    .apply_strip_action(crate::layout_strip::StripAction::Delete(active));
            }
            Op::Script(name) => add_script(desk, name),
            Op::Panel(section) => {
                let _ = MenuCall::OpenOrderFlowPanel(section).run(desk.as_window());
            }
            Op::Dock(name) => owners.surfaces.dock.open_tab(name),
            Op::Report => tab.paper.account_mut().autostart_report(),
            Op::ReportScroll(value) => tab.paper.account_mut().report.arm_scroll_probe(&value),
            Op::Calendar(selection) => tab.paper.account_mut().autostart_calendar(selection),
            Op::LedgerScope(scope) => tab.paper.account_mut().set_ledger_scope(scope),
            Op::LedgerPages(pages) => tab.paper.account_mut().autostart_ledger_pages(pages),
            Op::FoldLedger => tab.paper.account_mut().autostart_folded_days(tz),
            Op::ReportList(open) => tab.paper.account_mut().set_report_list_open(open),
            Op::Layout(layout) => tab.set_layout(layout),
            Op::FoldContext(upper) => tab.canvas.fold_context_pane(upper),
            Op::PaneMenu(intent) => {
                owners
                    .surfaces
                    .remove_chart
                    .update_scoped(runtime.tabs.active_id(), intent);
            }
            Op::Export(path) => desk.workspace_file().export_workspace_to(Path::new(&path)),
            Op::Import(path) => desk
                .workspace_file()
                .import_workspace_from(Path::new(&path)),
            Op::Toast(text) => runtime
                .tabs
                .runtime_mut(0)
                .paper
                .show_toast(text.to_owned()),
        }
    }
}
fn add_script(desk: &mut impl OwnersDesk, name: String) {
    let runtime = desk.owners().runtime;
    let tab = runtime.tabs.active_id();
    let side = runtime.active_tab().focused_side();
    if let Some(slot) = desk
        .layouts()
        .add_indicator_at(tab, side, &SavedKind::Script { name })
    {
        let runtime = desk.owners().runtime;
        let pane = runtime.tabs.by_id(tab).expect("launch tab").pane(side).id;
        let slot = crate::app::TabSlot {
            workspace: runtime.id(),
            pane,
            tab,
            side,
            slot,
        };
        runtime.indicators.registry.exclude_from_layout(slot);
    }
}
