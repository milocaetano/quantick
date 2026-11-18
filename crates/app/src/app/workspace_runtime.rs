//! Concrete workspace state and the native presentation adapter.
//!
//! The process keeps this payload while a view changes presentation. Existing
//! stores remain I/O adapters, including legacy installation-local fields.
use eframe::egui;
use quantick_workspace::lifetime::{
    LifetimeError, PresentationId, ResourceError, ViewId, WorkspaceId, WorkspaceLifetime,
    WorkspaceView,
};

pub(crate) struct WorkspaceRuntime {
    pub(super) alerts: super::alerts_runtime::Runtime,
    pub(super) chart_theme: quantick_chart::style::ChartThemeSync,
    pub(super) appearance: quantick_stores::appearance::AppearanceStore,
    pub(super) lifetime: WorkspaceLifetime,
    pub(super) tabs: super::ArrangementHost,
    pub(super) indicators: super::indicator_manager::IndicatorState,
    pub(super) footprint_config: crate::footprint_config::FootprintConfig,
    pub(super) style: crate::style::ChartStyle,
    pub(super) style_revision: u64,
    pub(super) history: quantick_session::HistorySettings,
    pub(super) tz: crate::timezone::TzOffset,
    pub(crate) workspace: crate::workspace_store::WorkspaceStore,
    /// Where orders go: the simulator or the one broker account.
    pub(super) broker: quantick_paper::route::BrokerSession,
}

impl WorkspaceRuntime {
    pub(super) fn new(
        tab: crate::tab::Tab,
        trades_dir: std::path::PathBuf,
        reach_span_minutes: u32,
        config: &crate::config::AppConfig,
    ) -> Self {
        // Resolve each startup path once: the owned store writes where it loaded.
        let symbols_path = crate::symbols_file::default_path();
        let footprint_settings_path = crate::footprint_config::settings_path();
        let indicator_presets_path = crate::indicators::preset_file::default_path();
        let layouts = super::layout_wiring::open_layout_store(config);
        Self {
            alerts: super::alerts_runtime::Runtime::new(
                super::alerts_runtime::path(),
                crate::metrics::wall_clock_ms(),
            ),
            chart_theme: Default::default(),
            appearance: crate::theme::store::load(),
            lifetime: WorkspaceLifetime::new(super::WORKSPACE_ID),
            tabs: super::ArrangementHost::new(
                super::WORKSPACE_ID,
                quantick_workspace::arrangement::TabId(super::FIRST_TAB_ID),
                tab,
            ),
            indicators: super::indicator_manager::IndicatorState::new(
                super::WORKSPACE_ID,
                &indicator_presets_path,
            ),
            footprint_config: crate::footprint_config::load(&footprint_settings_path),
            style: crate::style::ChartStyle::default(),
            style_revision: 0,
            history: quantick_session::HistorySettings {
                progressive_history: true,
                history_reach: quantick_feed::history_reach::HistoryReach::default(),
                history_reach_span_minutes: reach_span_minutes,
                venue_lead_in: false,
            },
            tz: crate::timezone::TzOffset::default(),
            workspace: crate::workspace_store::WorkspaceStore::new(
                crate::workspace_store::StorePaths {
                    symbols: symbols_path,
                    chart_layers: crate::chart_layers::default_path(),
                    footprint_settings: footprint_settings_path,
                    indicator_presets: indicator_presets_path,
                    ui_state: crate::ui_state::default_path(),
                    bubble_assets: crate::bubble_presets::assets_path(),
                },
                layouts,
                trades_dir,
            ),
            broker: crate::paper_trading::account_route::session(config),
        }
    }

    /// Intake follows workspace ownership even while no native view can paint.
    pub(super) fn drain_tabs(&mut self, config: &crate::config::AppConfig) -> u64 {
        self.alerts.begin_frame();
        let active = self.tabs.active_id();
        let mut trades = 0_u64;
        for (tab_id, tab) in self.tabs.iter_with_ids_mut() {
            let before = tab.link.live_trades;
            let policy = self.history.policy(self.tz, tab_id == active);
            tab.drain_frame_observed(tab_id, config, policy, &mut self.alerts);
            trades += tab.link.live_trades - before;
        }
        self.alerts.end_frame();
        self.route(config).frame();
        trades
    }

    /// The route session, joined with the tabs it routes for.
    pub(super) fn route<'a>(
        &'a mut self,
        config: &'a crate::config::AppConfig,
    ) -> quantick_paper::route::RouteCall<'a> {
        let tabs = crate::paper_trading::account_route::RouteTabs(&mut self.tabs, config);
        quantick_paper::route::RouteCall::new(&mut self.broker, Box::new(tabs))
    }

    /// The paper settings' owner, over this workspace's tabs and store.
    pub(super) fn paper_settings(&mut self) -> super::paper_wiring::PaperSettingsAdapter<'_> {
        let (tabs, workspace) = (&mut self.tabs, &mut self.workspace);
        super::paper_wiring::PaperSettingsAdapter { tabs, workspace }
    }

    /// The pane-layout owners, to read.
    pub(crate) fn layout_state(&self) -> super::layout_wiring::LayoutRead<'_> {
        let (tabs, layouts) = (&self.tabs, self.workspace.layouts());
        let (session, market) = (layouts.session(), layouts.market());
        let active = tabs.active_index();
        super::layout_wiring::LayoutRead {
            tabs,
            active,
            session,
            market,
        }
    }

    /// Raise the export dialog, suggesting the active chart's name.
    pub(super) fn open_export(&mut self) {
        let tab = &self.tabs[self.tabs.active_index()];
        let (symbol, spec) = (&tab.symbol, tab.flow_pane.series.state.spec());
        self.workspace.picker_mut().open_export(symbol, spec);
    }

    pub(super) fn id(&self) -> WorkspaceId {
        self.lifetime.id()
    }
    /// This workspace as `view` reaches it; a view that no longer resolves is
    /// a broken binding, so it panics rather than answering for another view.
    pub(super) fn bound(&self, view: &WorkspaceView) -> &Self {
        self.resolve(view).expect("bound workspace")
    }
    pub(super) fn bound_mut(&mut self, view: &WorkspaceView) -> &mut Self {
        self.resolve_mut(view).expect("bound workspace")
    }
    pub(super) fn active_tab(&self) -> &crate::tab::Tab {
        &self.tabs[self.tabs.active_index()]
    }
    pub(super) fn active_tab_mut(&mut self) -> &mut crate::tab::Tab {
        self.tabs.runtime_mut(self.tabs.active_index())
    }
    /// The slot a command from the chrome addresses: the active tab, its
    /// focused pane, that slot.
    pub(super) fn target_slot(&self, slot: crate::indicator_worker::SlotId) -> super::TabSlot {
        let tab = self.active_tab();
        let (pane, side) = (tab.focused_pane().id, tab.focused_side());
        let (workspace, tab) = (self.id(), self.tabs.active_id());
        super::TabSlot {
            workspace,
            pane,
            tab,
            side,
            slot,
        }
    }
    pub(super) fn resolve(&self, view: &WorkspaceView) -> Result<&Self, LifetimeError> {
        self.lifetime.resolve(view)?;
        Ok(self)
    }
    pub(super) fn resolve_mut(&mut self, view: &WorkspaceView) -> Result<&mut Self, LifetimeError> {
        self.lifetime.resolve(view)?;
        Ok(self)
    }

    /// Resolve a captured owner/tab/pane/slot, independently of current focus.
    pub(super) fn validate_target(&self, target: super::TabSlot) -> Result<(), ResourceError> {
        let address = target.address();
        self.lifetime.validate_address(address)?;
        let tab = self
            .tabs
            .by_id(address.tab)
            .ok_or(ResourceError::MissingTab)?;
        let pane = tab
            .pane_at(target.side.index())
            .filter(|pane| pane.id == address.pane)
            .ok_or(ResourceError::MissingPane)?;
        if !self.indicators.registry.contains(target)
            && !pane
                .indicators
                .all()
                .iter()
                .any(|view| view.slot.0 == address.slot)
        {
            return Err(ResourceError::MissingSlot);
        }
        Ok(())
    }
}

/// Native rendering identity stays outside the process-owned workspace payload.
pub(super) struct WindowPresentation {
    pub(super) context: Option<egui::Context>,
    pub(super) binding: WorkspaceView,
    pub(super) native: super::native_windows::NativeWindows,
}
impl WindowPresentation {
    pub(super) fn new(owner: &mut WorkspaceRuntime) -> Self {
        let binding = owner
            .lifetime
            .bind_view(ViewId(0))
            .expect("initial workspace view");
        let mut presentation = Self {
            binding,
            native: Default::default(),
            context: None,
        };
        presentation
            .rebind(owner, egui::ViewportId::ROOT)
            .expect("initial native binding");
        presentation
    }
    /// Change native placement and the headless binding together, before paint.
    pub(super) fn rebind(
        &mut self,
        owner: &WorkspaceRuntime,
        destination: egui::ViewportId,
    ) -> Result<(), LifetimeError> {
        let source = self.binding.presentation();
        let destination_id = PresentationId(destination.0.value());
        let plan = owner
            .lifetime
            .plan_rebind(&self.binding, source, destination_id)?;
        owner.lifetime.rebind(&mut self.binding, plan)
    }
    pub(super) fn resolve<'a>(
        &self,
        owner: &'a WorkspaceRuntime,
        ctx: &egui::Context,
    ) -> Result<&'a WorkspaceRuntime, LifetimeError> {
        if self.binding.presentation() != Some(PresentationId(ctx.viewport_id().0.value())) {
            return Err(LifetimeError::ForeignView);
        }
        owner.resolve(&self.binding)
    }
}
