//! Frame calls borrow their feature owners; the window only lends them.
use super::{GatewayPort, QuantickApp, owners_desk::OwnersDesk};
use quantick_workspace::market_list::PaletteIntent;

pub(super) trait FrameCalls: GatewayPort + OwnersDesk {
    fn status_owners(
        &self,
    ) -> (
        &super::health::HealthCounters,
        &super::workspace_runtime::WorkspaceRuntime,
        &crate::config::AppConfig,
    );

    fn status_model(&self) -> crate::statusbar::StatusModel {
        let (health, runtime, config) = self.status_owners();
        health.status_model(runtime, config)
    }
    fn drain_tabs(&mut self) {
        self.workspace_calls().drain_sources();
    }
    fn service_indicator_requests(&mut self) {
        let guides = self.owners().into_wiring().service_indicator_requests();
        let id = crate::control::INDICATOR_GUIDE_CAPABILITY_ID;
        for input in guides {
            let _ = self.control_action(id, 1, crate::control::ActionOrigin::Human, input);
        }
    }
    #[cfg(any(feature = "control-harness", test))]
    fn take_scenario_mark(&mut self) {
        let note = self.workspace_calls().control.scenarios.take_mark();
        if let Some(note) = note {
            self.take_mark((!note.is_empty()).then_some(note));
        }
    }
    fn apply_palette_intent(&mut self, intent: PaletteIntent) {
        let outcome = quantick_control_handlers::market_palette::settle(self.as_window(), intent);
        let owners = self.workspace_calls();
        let (palette, toast) = (
            &mut owners.surfaces.market_palette,
            &mut owners.surfaces.toast,
        );
        quantick_ui_model::market_palette::apply_palette_intent(
            outcome,
            |intent| {
                palette.update(intent);
            },
            |note| crate::surfaces::toast::note(toast, note, std::time::Instant::now()),
        );
    }
    fn apply_feed_switch(&mut self) {
        let settled = quantick_control_handlers::contract_roll::settle_auto_rolls(self.as_window());
        if !settled.is_empty() {
            self.workspace_calls()
                .as_hand(|_, journal| settled.journal(journal));
        }
        quantick_ui_model::frame_settlement::apply_feed_switch(&mut self.workspace_calls());
    }
    fn apply_layout_switches(&mut self) {
        quantick_ui_model::frame_settlement::apply_layout_switches(&mut self.workspace_calls());
    }
    #[cfg(test)]
    fn settle_paper_response_test(
        &mut self,
        response: crate::surfaces::SurfaceResponse,
    ) -> Result<(), Box<crate::surfaces::SurfaceResponse>> {
        let owners = self.workspace_calls();
        let paper = super::paper_wiring::panel_effects::begin_paper_panel_effects(
            &mut owners.runtime.tabs,
            &mut owners.surfaces.replay_browser,
            response,
        )?;
        super::paper_wiring::panel_effects::finish_paper_panel_effects(
            &mut owners.runtime.tabs,
            owners.config,
            &mut owners.runtime.workspace,
            paper,
        );
        Ok(())
    }
}

impl FrameCalls for QuantickApp {
    fn status_owners(
        &self,
    ) -> (
        &super::health::HealthCounters,
        &super::workspace_runtime::WorkspaceRuntime,
        &crate::config::AppConfig,
    ) {
        (
            &self.health,
            self.runtime.bound(&self.presentation.binding),
            &self.config,
        )
    }
}

impl super::workspace_calls::WorkspaceCalls<'_> {
    fn drain_sources(&mut self) {
        super::deal_recording_wiring::ensure(self.runtime, self.chrome, self.config);
        self.health.trades_since_summary += self.runtime.drain_tabs(self.config);
    }
}

impl quantick_ui_model::frame_settlement::Host for super::workspace_calls::WorkspaceCalls<'_> {
    fn switch_market(&mut self) -> bool {
        let tab = self.runtime.active_tab_mut();
        let changed = tab.link.replay.is_none()
            && (tab.active.0 != tab.feed_id || tab.active.1 != tab.symbol);
        tab.maybe_switch_feed(self.config);
        changed
    }
    fn apply_spec(&mut self) {
        self.runtime
            .bound_mut(&self.presentation.binding)
            .active_tab_mut()
            .apply_spec_changes();
    }
    fn follow_market(&mut self) {
        self.layout().follow_market_changes();
    }
    fn pending_layouts(&mut self) {
        for (id, tab) in self.runtime.tabs.iter_with_ids_mut() {
            tab.apply_pending_layout(id, self.config, &self.runtime.style, self.pane_ids);
        }
    }
    fn maintain_layouts(&mut self) {
        self.layout().maintain_layouts();
    }
    fn maintain_workspaces(&mut self) {
        self.maintain();
    }
}
